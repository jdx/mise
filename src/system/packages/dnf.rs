use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

/// RedHat-family (Fedora, RHEL, CentOS, Rocky, Alma) via dnf
pub(crate) struct DnfManager {}

impl DnfManager {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

// Never add `--` here: DNF5 rejects it on subcommands like install/upgrade.
// Pins use rpm NEVRA syntax (name-version) which dnf accepts positionally.
fn pkg_operand(p: &PackageRequest) -> String {
    if group_id(p).is_some() {
        // groups have no version; `dnf install @group` works on dnf4 and dnf5
        return p.name.clone();
    }
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

/// `@development-tools` names a comps group rather than an rpm package.
fn group_id(p: &PackageRequest) -> Option<&str> {
    p.name.strip_prefix('@')
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
    args.extend(
        pkgs.iter()
            .filter(|p| group_id(p).is_none())
            .map(pkg_operand),
    );
    args
}

fn group_upgrade_args(pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec![
        "group".to_string(),
        "upgrade".to_string(),
        "-y".to_string(),
        "--refresh".to_string(),
    ];
    args.extend(pkgs.iter().filter_map(group_id).map(str::to_string));
    args
}

/// Ids of installed groups from `dnf group list --installed`. dnf5 prints a
/// table whose first column is the id; dnf4 with `-v` prints `Name (id)`.
fn parse_group_list(output: &str) -> Vec<String> {
    let mut ids = vec![];
    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_suffix(')')
            && let Some((_, id)) = rest.rsplit_once('(')
        {
            ids.push(id.to_string());
        } else if let Some(first) = line.split_whitespace().next() {
            ids.push(first.to_string());
        }
    }
    ids
}

fn parse_group_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let installed = parse_group_list(output);
    requests
        .iter()
        .map(|req| PackageStatus {
            request: req.clone(),
            state: match group_id(req) {
                Some(id) if installed.iter().any(|i| i == id) => PackageState::Installed {
                    version: "group".to_string(),
                },
                _ => PackageState::Missing,
            },
            display_name: None,
        })
        .collect()
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
    async fn installed_rpms(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
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
        Ok(parse_rpm_query(&stdout, pkgs))
    }

    async fn installed_groups(&self, groups: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        // dnf4 only prints group ids with -v; dnf5 has no such flag and
        // always prints the id column
        let dnf5 = tokio::process::Command::new("dnf")
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("dnf5"))
            .unwrap_or(false);
        let mut args = vec!["group", "list", "--installed", "--hidden"];
        if !dnf5 {
            args.push("-v");
        }
        debug!("$ dnf {}", args.join(" "));
        let output = tokio::process::Command::new("dnf")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !output.status.success() {
            bail!(
                "dnf group list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(parse_group_query(
            &String::from_utf8_lossy(&output.stdout),
            groups,
        ))
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
        if pkgs.iter().any(|p| p.name.starts_with("@^")) {
            bail!("dnf environment groups (@^name) are not supported; use the group ids instead");
        }
        let (groups, plain): (Vec<_>, Vec<_>) =
            pkgs.iter().cloned().partition(|p| group_id(p).is_some());
        let mut statuses = if groups.is_empty() {
            vec![]
        } else {
            self.installed_groups(&groups).await?
        };
        if !plain.is_empty() {
            statuses.extend(self.installed_rpms(&plain).await?);
        }
        // restore the caller's order
        statuses.sort_by_key(|s| pkgs.iter().position(|p| p.name == s.request.name));
        Ok(statuses)
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let args = install_args(pkgs, opts);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            return Ok(());
        }
        sudo::run("dnf", &args, &[])
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut runs = vec![];
        if pkgs.iter().any(|p| group_id(p).is_none()) {
            runs.push(upgrade_args(pkgs));
        }
        if pkgs.iter().any(|p| group_id(p).is_some()) {
            runs.push(group_upgrade_args(pkgs));
        }
        for args in runs {
            if opts.dry_run {
                miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            } else {
                sudo::run("dnf", &args, &[])?;
            }
        }
        Ok(())
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
    fn test_group_requests() {
        let pkgs = vec![req("@development-tools", None), req("bat", None)];
        let opts = InstallOpts {
            dry_run: false,
            update: false,
            ask: false,
        };
        assert_eq!(
            install_args(&pkgs, &opts),
            vec!["install", "-y", "@development-tools", "bat"]
        );
        assert_eq!(
            upgrade_args(&pkgs),
            vec!["upgrade", "-y", "--refresh", "bat"]
        );
        assert_eq!(
            group_upgrade_args(&pkgs),
            vec!["group", "upgrade", "-y", "--refresh", "development-tools"]
        );
    }

    #[test]
    fn test_parse_group_query() {
        let requests = vec![
            req("@development-tools", None),
            req("@virtualization", None),
        ];
        // dnf5 table
        let dnf5 = "ID                Name              Installed\ndevelopment-tools Development Tools yes\n";
        let st = parse_group_query(dnf5, &requests);
        assert!(matches!(st[0].state, PackageState::Installed { .. }));
        assert_eq!(st[1].state, PackageState::Missing);
        // dnf4 -v
        let dnf4 = "Installed Groups:\n   Development Tools (development-tools)\n";
        let st = parse_group_query(dnf4, &requests);
        assert!(matches!(st[0].state, PackageState::Installed { .. }));
        assert_eq!(st[1].state, PackageState::Missing);
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
