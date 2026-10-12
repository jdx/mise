use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::config::Settings;
use crate::config::settings::SystemPackagesOstree;
use crate::result::Result;
use crate::system::sudo;

/// RedHat-family (Fedora, RHEL, CentOS, Rocky, Alma) via dnf
pub(crate) struct DnfManager {}

impl DnfManager {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

/// Set by systemd's ostree integration on a host booted from an ostree image
/// (Silverblue, Kinoite, Bazzite, secureblue, ...). It is absent inside a
/// bootc/Containerfile build, where `dnf` works normally.
const OSTREE_BOOTED: &str = "/run/ostree-booted";

fn ostree_booted() -> bool {
    ostree_booted_at(Path::new(OSTREE_BOOTED))
}

fn ostree_booted_at(marker: &Path) -> bool {
    marker.exists()
}

fn layering_on(ostree: bool) -> bool {
    ostree && Settings::get().system_packages.ostree == SystemPackagesOstree::Layer
}

fn ostree_error(verb: &str) -> eyre::Report {
    eyre::eyre!(
        "cannot {verb} dnf packages: this host is booted from an ostree image, so /usr is read-only\n\
         Bake the packages into the image, install them in a toolbox, or let mise layer them with rpm-ostree:\n  \
         mise settings set system_packages.ostree layer"
    )
}

const PENDING_REBOOT: &str = "pending reboot";

/// One transaction for the whole batch: each rpm-ostree call builds a new
/// deployment. Pins do not apply (see [`SystemPackageManager::supports_version_pins`]).
fn ostree_install_args(pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec![
        "install".to_string(),
        "--idempotent".to_string(),
        "--allow-inactive".to_string(),
    ];
    args.extend(pkgs.iter().map(|p| p.name.clone()));
    args
}

fn ostree_uninstall_args(pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec!["uninstall".to_string()];
    args.extend(pkgs.iter().map(|p| p.name.clone()));
    args
}

/// Layered package requests from `rpm-ostree status --json`.
#[derive(Debug, Default, PartialEq, Eq)]
struct OstreeRequests {
    /// requested by the booted deployment
    booted: Vec<String>,
    /// requested by the deployment the next boot uses: the staged one when a
    /// reboot is pending, otherwise the booted one
    next: Vec<String>,
}

fn parse_requests(json: &str) -> OstreeRequests {
    let Ok(status) = serde_json::from_str::<serde_json::Value>(json) else {
        return OstreeRequests::default();
    };
    let Some(deployments) = status["deployments"].as_array() else {
        return OstreeRequests::default();
    };
    let requested = |d: &serde_json::Value| -> Vec<String> {
        d["requested-packages"]
            .as_array()
            .map(|pkgs| {
                pkgs.iter()
                    .filter_map(|p| p.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let flag = |d: &serde_json::Value, key: &str| d[key].as_bool().unwrap_or(false);
    let booted = deployments.iter().find(|d| flag(d, "booted"));
    let next = deployments.iter().find(|d| flag(d, "staged")).or(booted);
    OstreeRequests {
        booted: booted.map(requested).unwrap_or_default(),
        next: next.map(requested).unwrap_or_default(),
    }
}

async fn ostree_requests() -> OstreeRequests {
    let output = tokio::process::Command::new("rpm-ostree")
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => parse_requests(&String::from_utf8_lossy(&o.stdout)),
        _ => OstreeRequests::default(),
    }
}

/// `rpm -q` sees only the booted image, so fold in what the next deployment
/// will hold:
/// - a package requested for the next deployment but not yet booted counts as
///   installed (`pending reboot`), so later runs do not request it again; a
///   pinned request is left alone, since the pin cannot be checked or honored
/// - a package layered in the booted deployment whose request was dropped is
///   pending removal and counts as missing, so declaring it present requests
///   it again and declaring it absent does not uninstall it twice
fn apply_requests(statuses: &mut [PackageStatus], requests: &OstreeRequests) {
    for status in statuses {
        let name = &status.request.name;
        let wanted_next = requests.next.contains(name);
        match &status.state {
            PackageState::Missing if wanted_next && status.request.version.is_none() => {
                status.state = PackageState::Installed {
                    version: PENDING_REBOOT.to_string(),
                };
            }
            PackageState::Installed { .. } if requests.booted.contains(name) && !wanted_next => {
                status.state = PackageState::Missing;
            }
            _ => {}
        }
    }
}

// Never add `--` here: DNF5 rejects it on subcommands like install/upgrade.
// Pins use rpm NEVRA syntax (name-version) which dnf accepts positionally.
fn pkg_operand(p: &PackageRequest) -> String {
    match &p.version {
        Some(v) => format!("{}-{v}", p.name),
        None => p.name.clone(),
    }
}

fn install_args(pkgs: &[PackageRequest], opts: &InstallOpts) -> Vec<String> {
    let mut args = vec!["install".to_string(), "-y".to_string()];
    if opts.update {
        args.push("--refresh".to_string());
    }
    args.extend(pkgs.iter().map(pkg_operand));
    args
}

fn upgrade_args(pkgs: &[PackageRequest]) -> Vec<String> {
    // --refresh: expire cached metadata so "upgrade" actually sees new
    // versions; `dnf upgrade <pkg>` only touches already-installed
    // packages (a pin downgrade would need `dnf install name-version`,
    // which the install path already provides)
    let mut args = vec![
        "upgrade".to_string(),
        "-y".to_string(),
        "--refresh".to_string(),
    ];
    args.extend(pkgs.iter().map(pkg_operand));
    args
}

fn parse_rpm_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let mut installed: HashMap<&str, &str> = HashMap::new();
    for line in output.lines() {
        if let Some((name, version)) = line.split_once('\t') {
            installed.insert(name, version);
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(req.name.as_str()) {
                // a pin must match the installed version-release exactly, or
                // its version part (a version-only pin matches any release)
                Some(version) => match &req.version {
                    Some(requested)
                        if *version != requested
                            && !version.starts_with(&format!("{requested}-")) =>
                    {
                        PackageState::VersionMismatch {
                            installed: version.to_string(),
                        }
                    }
                    _ => PackageState::Installed {
                        version: version.to_string(),
                    },
                },
                None => PackageState::Missing,
            };
            PackageStatus {
                request: req.clone(),
                state,
                display_name: None,
            }
        })
        .collect()
}

#[async_trait(?Send)]
impl SystemPackageManager for DnfManager {
    fn name(&self) -> &str {
        "dnf"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux")
            && (crate::file::which("dnf").is_some()
                // layering needs rpm-ostree, not dnf, which an atomic host may lack
                || (layering_on(ostree_booted()) && crate::file::which("rpm-ostree").is_some()))
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "dnf not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let mut args = vec![
            "-q".to_string(),
            "--qf".to_string(),
            "%{NAME}\\t%{VERSION}-%{RELEASE}\\n".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        debug!("$ rpm {}", args.join(" "));
        let output = tokio::process::Command::new("rpm")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // rpm -q exits nonzero when any package is not installed; "package X
        // is not installed" goes to stdout or stderr depending on rpm version
        // and won't match the \t format either way — absent packages parse as
        // Missing. Only fail on rpm errors unrelated to missing packages.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success()
            && !stderr.is_empty()
            && !stderr.lines().all(|l| {
                l.trim().is_empty() || l.contains("is not installed") || l.contains("no packages")
            })
        {
            bail!("rpm -q failed: {}", stderr.trim());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut statuses = parse_rpm_query(&stdout, pkgs);
        if ostree_booted() {
            apply_requests(&mut statuses, &ostree_requests().await);
        }
        Ok(statuses)
    }

    fn supports_version_pins(&self) -> bool {
        // rpm-ostree layering cannot pin: it would fight base-image updates.
        // Without layering, pins must reach `install` so it can explain the error.
        !layering_on(ostree_booted())
    }

    fn supports_remove(&self) -> bool {
        layering_on(ostree_booted())
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // only packages layered for the next deployment can be uninstalled;
        // packages in the base image are never touched
        let layered = ostree_requests().await.next;
        let (pkgs, base): (Vec<_>, Vec<_>) = pkgs
            .iter()
            .cloned()
            .partition(|p| layered.contains(&p.name));
        for p in &base {
            warn!("dnf: not removing {p}: it is part of the base image, not a layered package");
        }
        if pkgs.is_empty() {
            return Ok(());
        }
        let args = ostree_uninstall_args(&pkgs);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("rpm-ostree", &args).join(" "));
            return Ok(());
        }
        sudo::run("rpm-ostree", &args, &[])?;
        miseprintln!("dnf: reboot to finish removing the layered packages");
        Ok(())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install_on(ostree_booted(), pkgs, opts)
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.upgrade_on(ostree_booted(), pkgs, opts)
    }
}

impl DnfManager {
    fn install_on(&self, ostree: bool, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if ostree {
            return Self::layer(pkgs, opts);
        }
        let args = install_args(pkgs, opts);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            return Ok(());
        }
        sudo::run("dnf", &args, &[])
    }

    fn layer(pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if !layering_on(true) {
            return Err(ostree_error("install"));
        }
        // the driver already skips pinned entries (supports_version_pins)
        let refresh = vec!["refresh-md".to_string()];
        let args = ostree_install_args(pkgs);
        if opts.dry_run {
            if opts.update {
                miseprintln!("{}", sudo::argv("rpm-ostree", &refresh).join(" "));
            }
            miseprintln!("{}", sudo::argv("rpm-ostree", &args).join(" "));
            return Ok(());
        }
        if opts.update {
            sudo::run("rpm-ostree", &refresh, &[])?;
        }
        sudo::run("rpm-ostree", &args, &[])?;
        miseprintln!("dnf: reboot to start using the layered packages");
        Ok(())
    }

    fn upgrade_on(&self, ostree: bool, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if ostree {
            if !layering_on(true) {
                return Err(ostree_error("upgrade"));
            }
            miseprintln!(
                "dnf: layered packages update together with the base image; run `rpm-ostree upgrade` to update this host"
            );
            return Ok(());
        }
        let args = upgrade_args(pkgs);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            return Ok(());
        }
        sudo::run("dnf", &args, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(name: &str, version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
            desired: crate::system::packages::PackageDesiredState::Present,
        }
    }

    #[test]
    fn test_ostree_error_names_the_setting() {
        let msg = ostree_error("install").to_string();
        assert!(msg.contains("ostree"));
        assert!(msg.contains("system_packages.ostree layer"));
    }

    #[test]
    fn test_ostree_marker_decides_booted() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ostree-booted");
        assert!(!ostree_booted_at(&marker));
        std::fs::write(&marker, "").unwrap();
        assert!(ostree_booted_at(&marker));
    }

    #[test]
    fn test_ostree_args() {
        let pkgs = vec![req("gimp", None), req("htop", None)];
        assert_eq!(
            ostree_install_args(&pkgs),
            vec![
                "install",
                "--idempotent",
                "--allow-inactive",
                "gimp",
                "htop"
            ]
        );
        assert_eq!(
            ostree_uninstall_args(&pkgs),
            vec!["uninstall", "gimp", "htop"]
        );
    }

    #[test]
    fn test_requests_follow_the_staged_deployment() {
        let json = r#"{"deployments":[
            {"staged":true,"requested-packages":["gimp"]},
            {"booted":true,"requested-packages":["old"]}]}"#;
        assert_eq!(
            parse_requests(json),
            OstreeRequests {
                booted: vec!["old".to_string()],
                next: vec!["gimp".to_string()],
            }
        );
        let json = r#"{"deployments":[{"booted":true,"requested-packages":["old"]}]}"#;
        assert_eq!(parse_requests(json).next, vec!["old"]);
        assert_eq!(parse_requests("not json"), OstreeRequests::default());
    }

    #[test]
    fn test_pending_installs_and_removals() {
        let requests = OstreeRequests {
            booted: vec!["old".to_string()],
            next: vec!["gimp".to_string(), "pinned".to_string()],
        };
        let mut statuses = parse_rpm_query(
            "old\t1-1\nbase\t1-1\n",
            &[
                req("gimp", None),
                req("htop", None),
                req("old", None),
                req("base", None),
                req("pinned", Some("1.0")),
            ],
        );
        apply_requests(&mut statuses, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "pending reboot".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        // layered in the booted deployment, request dropped: pending removal
        assert_eq!(statuses[2].state, PackageState::Missing);
        // part of the base image: never touched
        assert!(statuses[3].state.is_installed());
        // a pinned request is not satisfied by a pending unpinned one
        assert_eq!(statuses[4].state, PackageState::Missing);
    }

    #[test]
    fn test_ostree_host_rejects_install_unless_layering() {
        let mgr = DnfManager::new();
        let pkgs = vec![req("gimp", None)];
        let opts = InstallOpts {
            dry_run: true,
            update: false,
            ask: false,
        };
        let err = mgr.install_on(true, &pkgs, &opts).unwrap_err().to_string();
        assert!(err.contains("system_packages.ostree layer"));
        // upgrade fails the same way, rather than reporting success
        let err = mgr.upgrade_on(true, &pkgs, &opts).unwrap_err().to_string();
        assert!(err.contains("cannot upgrade"));
        // without the marker, a dry run goes on to print the dnf command
        assert!(mgr.install_on(false, &pkgs, &opts).is_ok());
        assert!(mgr.upgrade_on(false, &pkgs, &opts).is_ok());
    }

    #[test]
    fn test_install_args_no_separator() {
        let pkgs = vec![req("ripgrep", None), req("bat", Some("0.24.0"))];
        let opts = InstallOpts {
            dry_run: false,
            update: false,
            ask: false,
        };
        let args = install_args(&pkgs, &opts);
        // DNF5 rejects a bare `--` on subcommands; it must never appear
        assert!(args.iter().all(|a| a != "--"));
        assert_eq!(args, vec!["install", "-y", "ripgrep", "bat-0.24.0"]);
    }

    #[test]
    fn test_install_args_update_adds_refresh() {
        let pkgs = vec![req("ripgrep", None)];
        let opts = InstallOpts {
            dry_run: false,
            update: true,
            ask: false,
        };
        let args = install_args(&pkgs, &opts);
        assert!(args.iter().all(|a| a != "--"));
        // --refresh precedes the operands, after the subcommand flags
        assert_eq!(args, vec!["install", "-y", "--refresh", "ripgrep"]);
    }

    #[test]
    fn test_upgrade_args_no_separator() {
        let pkgs = vec![req("ripgrep", None), req("bat", Some("0.24.0"))];
        let args = upgrade_args(&pkgs);
        assert!(args.iter().all(|a| a != "--"));
        assert_eq!(
            args,
            vec!["upgrade", "-y", "--refresh", "ripgrep", "bat-0.24.0"]
        );
    }

    #[test]
    fn test_parse_rpm_query() {
        let requests = vec![
            req("bc", None),
            req("nonexistent", None),
            req("bash", Some("5.2.26-3.fc40")),
            req("glib2-devel", None),
            req("zsh", Some("5.8-1.fc40")),
            req("tmux", Some("3.4")),
        ];
        let output = "bc\t1.07.1-14.fc39\npackage nonexistent is not installed\nbash\t5.2.26-3.fc40\nglib2\t2.80.0-1.fc40\nzsh\t5.9-2.fc40\ntmux\t3.4-3.fc40\n";
        let statuses = parse_rpm_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.07.1-14.fc39".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        // a version-release pin matches the full installed version-release
        assert_eq!(
            statuses[2].state,
            PackageState::Installed {
                version: "5.2.26-3.fc40".to_string()
            }
        );
        // an installed "glib2" must not satisfy a "glib2-devel" request
        assert_eq!(statuses[3].state, PackageState::Missing);
        // a different installed version must not satisfy a pin
        assert_eq!(
            statuses[4].state,
            PackageState::VersionMismatch {
                installed: "5.9-2.fc40".to_string()
            }
        );
        // a version-only pin matches any release
        assert_eq!(
            statuses[5].state,
            PackageState::Installed {
                version: "3.4-3.fc40".to_string()
            }
        );
    }
}
