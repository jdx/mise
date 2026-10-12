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
    Path::new(OSTREE_BOOTED).exists()
}

fn layering() -> bool {
    ostree_booted() && Settings::get().system_packages.ostree == SystemPackagesOstree::Layer
}

fn ostree_error() -> eyre::Report {
    eyre::eyre!(
        "cannot install dnf packages: this host is booted from an ostree image, so /usr is read-only\n\
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

/// The packages requested for the next deployment, from `rpm-ostree status
/// --json`. The first deployment is the staged one when a reboot is pending,
/// and the booted one otherwise.
fn parse_requested_packages(json: &str) -> Vec<String> {
    let Ok(status) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec![];
    };
    status["deployments"][0]["requested-packages"]
        .as_array()
        .map(|pkgs| {
            pkgs.iter()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

async fn requested_packages() -> Vec<String> {
    let output = tokio::process::Command::new("rpm-ostree")
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            parse_requested_packages(&String::from_utf8_lossy(&o.stdout))
        }
        _ => vec![],
    }
}

/// Packages that are not in the booted image but are already requested for the
/// next deployment count as installed, so every run does not request them again.
fn mark_pending(statuses: &mut [PackageStatus], requested: &[String]) {
    for status in statuses {
        if status.state == PackageState::Missing && requested.contains(&status.request.name) {
            status.state = PackageState::Installed {
                version: PENDING_REBOOT.to_string(),
            };
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

impl DnfManager {
    fn layer(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if !layering() {
            return Err(ostree_error());
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
}

#[async_trait(?Send)]
impl SystemPackageManager for DnfManager {
    fn name(&self) -> &str {
        "dnf"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("dnf").is_some()
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
        if ostree_booted() && statuses.iter().any(|s| s.state == PackageState::Missing) {
            mark_pending(&mut statuses, &requested_packages().await);
        }
        Ok(statuses)
    }

    fn supports_version_pins(&self) -> bool {
        // rpm-ostree layering cannot pin: it would fight base-image updates
        !ostree_booted()
    }

    fn supports_remove(&self) -> bool {
        layering()
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let args = ostree_uninstall_args(pkgs);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("rpm-ostree", &args).join(" "));
            return Ok(());
        }
        sudo::run("rpm-ostree", &args, &[])?;
        miseprintln!("dnf: reboot to finish removing the layered packages");
        Ok(())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if ostree_booted() {
            return self.layer(pkgs, opts);
        }
        let args = install_args(pkgs, opts);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            return Ok(());
        }
        sudo::run("dnf", &args, &[])
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if ostree_booted() {
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
        let msg = ostree_error().to_string();
        assert!(msg.contains("ostree"));
        assert!(msg.contains("system_packages.ostree layer"));
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
    fn test_pending_requested_packages_count_as_installed() {
        let json =
            r#"{"deployments":[{"requested-packages":["gimp"]},{"requested-packages":["old"]}]}"#;
        let requested = parse_requested_packages(json);
        assert_eq!(requested, vec!["gimp"]);
        let mut statuses = parse_rpm_query("", &[req("gimp", None), req("htop", None)]);
        mark_pending(&mut statuses, &requested);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "pending reboot".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        assert!(parse_requested_packages("not json").is_empty());
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
