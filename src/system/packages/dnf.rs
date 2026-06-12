use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

/// RedHat-family (Fedora, RHEL, CentOS, Rocky, Alma) via dnf
pub struct DnfManager {}

impl DnfManager {
    pub fn new() -> Self {
        Self {}
    }
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
            // entries can be bare names or rpm name-version-release specs;
            // match the exact name, or a spec like "bash-5.2.26-3.fc40" where
            // what follows the name is a version (starts with a digit) — a
            // bare "glib2" install must not satisfy a "glib2-devel" request
            let found = installed.iter().find(|(name, _)| {
                req.name == **name
                    || req
                        .name
                        .strip_prefix(*name)
                        .and_then(|rest| rest.strip_prefix('-'))
                        .and_then(|version| version.chars().next())
                        .is_some_and(|c| c.is_ascii_digit())
            });
            let state = match found {
                Some((_, version)) => PackageState::Installed {
                    version: version.to_string(),
                },
                None => PackageState::Missing,
            };
            PackageStatus {
                request: req.clone(),
                state,
            }
        })
        .collect()
}

#[async_trait]
impl SystemPackageManager for DnfManager {
    fn name(&self) -> &'static str {
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
        Ok(parse_rpm_query(&stdout, pkgs))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut args = vec!["install".to_string(), "-y".to_string()];
        if opts.update {
            args.push("--refresh".to_string());
        }
        // raw entries pass through verbatim — dnf natively handles
        // name-version-release pins and group/module syntax
        args.extend(pkgs.iter().map(|p| p.raw.clone()));
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

    #[test]
    fn test_parse_rpm_query() {
        let mgr = DnfManager::new();
        let requests = vec![
            mgr.parse_request("bc"),
            mgr.parse_request("nonexistent"),
            mgr.parse_request("bash-5.2.26-3.fc40"),
            mgr.parse_request("glib2-devel"),
        ];
        let output = "bc\t1.07.1-14.fc39\npackage nonexistent is not installed\nbash\t5.2.26-3.fc40\nglib2\t2.80.0-1.fc40\n";
        let statuses = parse_rpm_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.07.1-14.fc39".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        // name-version-release specs match by name prefix
        assert_eq!(
            statuses[2].state,
            PackageState::Installed {
                version: "5.2.26-3.fc40".to_string()
            }
        );
        // an installed "glib2" must not satisfy a "glib2-devel" request
        assert_eq!(statuses[3].state, PackageState::Missing);
    }
}
