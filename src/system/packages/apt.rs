use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

pub struct AptManager {}

impl AptManager {
    pub fn new() -> Self {
        Self {}
    }

    /// fresh container/VM case: no package lists at all, so any install would
    /// fail with "Unable to locate package"
    fn lists_missing(&self) -> bool {
        let lists = Path::new("/var/lib/apt/lists");
        !crate::file::ls(lists).unwrap_or_default().iter().any(|p| {
            p.file_name()
                .map(|f| f.to_string_lossy().contains("_Packages"))
                .unwrap_or(false)
        })
    }

    fn update(&self, opts: &InstallOpts) -> Result<()> {
        let args = vec!["update".to_string()];
        if opts.dry_run {
            miseprintln!(
                "{}",
                sudo::argv_with_env("apt-get", &args, &debian_frontend()).join(" ")
            );
            return Ok(());
        }
        sudo::run("apt-get", &args, &debian_frontend())
    }
}

fn debian_frontend() -> Vec<(String, String)> {
    vec![("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())]
}

/// `name` may carry an architecture qualifier (`gcc:arm64`); dpkg-query
/// reports the bare package name
fn dpkg_name(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

fn parse_dpkg_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    // keyed by both "name" and "name:arch" so arch-qualified requests
    // (gcc:arm64) only match that architecture
    let mut installed: HashMap<String, (&str, &str)> = HashMap::new();
    for line in output.lines() {
        let mut parts = line.split('\t');
        if let (Some(name), Some(status), Some(version)) =
            (parts.next(), parts.next(), parts.next())
        {
            if let Some(arch) = parts.next() {
                installed.insert(format!("{name}:{arch}"), (status, version));
            }
            // a package can be present for several architectures
            // (multi-arch); a bare-name request is satisfied by any
            // installed arch, so never let a deinstalled foreign arch
            // overwrite an installed entry
            installed
                .entry(name.to_string())
                .and_modify(|entry| {
                    if entry.0 != "installed" {
                        *entry = (status, version);
                    }
                })
                .or_insert((status, version));
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(&req.name) {
                Some(("installed", version)) => match &req.version {
                    Some(requested) if requested != version => PackageState::VersionMismatch {
                        installed: version.to_string(),
                    },
                    _ => PackageState::Installed {
                        version: version.to_string(),
                    },
                },
                _ => PackageState::Missing,
            };
            PackageStatus {
                request: req.clone(),
                state,
            }
        })
        .collect()
}

#[async_trait]
impl SystemPackageManager for AptManager {
    fn name(&self) -> &'static str {
        "apt"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("apt-get").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "apt-get not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let mut args = vec![
            "-W".to_string(),
            "-f=${Package}\\t${db:Status-Status}\\t${Version}\\t${Architecture}\\n".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| dpkg_name(&p.name).to_string()));
        debug!("$ dpkg-query {}", args.join(" "));
        let output = tokio::process::Command::new("dpkg-query")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // exit 1 just means some packages are unknown to dpkg — they're Missing
        if !output.status.success() && output.status.code() != Some(1) {
            bail!(
                "dpkg-query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_dpkg_query(&stdout, pkgs))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.update || self.lists_missing() {
            self.update(opts)?;
        }
        let mut args = vec!["install".to_string(), "-y".to_string()];
        // raw entries pass through verbatim — apt-get natively handles
        // name=version pins and name:arch qualifiers
        args.extend(pkgs.iter().map(|p| p.raw.clone()));
        if opts.dry_run {
            miseprintln!(
                "{}",
                sudo::argv_with_env("apt-get", &args, &debian_frontend()).join(" ")
            );
            return Ok(());
        }
        sudo::run("apt-get", &args, &debian_frontend())
    }

    fn parse_request(&self, raw: &str) -> PackageRequest {
        let (name, version) = match raw.split_once('=') {
            Some((name, version)) => (name.to_string(), Some(version.to_string())),
            None => (raw.to_string(), None),
        };
        PackageRequest {
            raw: raw.to_string(),
            name,
            version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mgr: &AptManager, raw: &str) -> PackageRequest {
        mgr.parse_request(raw)
    }

    #[test]
    fn test_parse_request() {
        let mgr = AptManager::new();
        let r = req(&mgr, "libssl-dev");
        assert_eq!(r.name, "libssl-dev");
        assert_eq!(r.version, None);
        let r = req(&mgr, "curl=8.5.0-2ubuntu10");
        assert_eq!(r.name, "curl");
        assert_eq!(r.version, Some("8.5.0-2ubuntu10".to_string()));
        let r = req(&mgr, "gcc:arm64");
        assert_eq!(r.name, "gcc:arm64");
        assert_eq!(dpkg_name(&r.name), "gcc");
    }

    #[test]
    fn test_parse_dpkg_query() {
        let mgr = AptManager::new();
        let requests = vec![
            req(&mgr, "bc"),
            req(&mgr, "nonexistent"),
            req(&mgr, "removed-pkg"),
            req(&mgr, "curl=9.9.9"),
        ];
        let output = "bc\tinstalled\t1.07.1-3\tamd64\nremoved-pkg\tdeinstall\t2.0\tamd64\ncurl\tinstalled\t8.5.0-2\tamd64\n";
        let statuses = parse_dpkg_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.07.1-3".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        assert_eq!(statuses[2].state, PackageState::Missing);
        assert_eq!(
            statuses[3].state,
            PackageState::VersionMismatch {
                installed: "8.5.0-2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_dpkg_query_multiarch_bare_name() {
        let mgr = AptManager::new();
        let requests = vec![req(&mgr, "libssl3")];
        // installed for amd64, deinstalled for i386 — the bare request is
        // satisfied regardless of which line dpkg-query prints last
        for output in [
            "libssl3\tinstalled\t3.0.2\tamd64\nlibssl3\tdeinstall\t3.0.1\ti386\n",
            "libssl3\tdeinstall\t3.0.1\ti386\nlibssl3\tinstalled\t3.0.2\tamd64\n",
        ] {
            let statuses = parse_dpkg_query(output, &requests);
            assert_eq!(
                statuses[0].state,
                PackageState::Installed {
                    version: "3.0.2".to_string()
                }
            );
        }
    }

    #[test]
    fn test_parse_dpkg_query_arch_qualified() {
        let mgr = AptManager::new();
        let requests = vec![req(&mgr, "gcc:arm64"), req(&mgr, "gcc:amd64")];
        let output = "gcc\tinstalled\t12.3\tarm64\n";
        let statuses = parse_dpkg_query(output, &requests);
        // gcc:arm64 matches the installed arm64 package
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "12.3".to_string()
            }
        );
        // gcc:amd64 does not match the arm64 install
        assert_eq!(statuses[1].state, PackageState::Missing);
    }
}
