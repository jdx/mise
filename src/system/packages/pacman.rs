use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

/// Arch-family (Arch, Manjaro, EndeavourOS) via pacman
pub struct PacmanManager {}

impl PacmanManager {
    pub fn new() -> Self {
        Self {}
    }

    /// fresh container case: no sync databases, any install would fail
    fn dbs_missing(&self) -> bool {
        let sync = Path::new("/var/lib/pacman/sync");
        !crate::file::ls(sync).unwrap_or_default().iter().any(|p| {
            p.extension()
                .map(|e| e.to_string_lossy() == "db")
                .unwrap_or(false)
        })
    }

    fn refresh(&self, opts: &InstallOpts) -> Result<()> {
        let args = vec!["-Sy".to_string()];
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("pacman", &args).join(" "));
            return Ok(());
        }
        sudo::run("pacman", &args, &[])
    }
}

fn parse_pacman_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let mut installed: HashMap<&str, &str> = HashMap::new();
    for line in output.lines() {
        if let Some((name, version)) = line.split_once(' ') {
            installed.insert(name, version);
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(req.name.as_str()) {
                Some(version) => PackageState::Installed {
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
impl SystemPackageManager for PacmanManager {
    fn name(&self) -> &'static str {
        "pacman"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("pacman").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "pacman not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let mut args = vec!["-Q".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        debug!("$ pacman {}", args.join(" "));
        let output = tokio::process::Command::new("pacman")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // pacman -Q exits 1 when any package is missing ("error: package 'x'
        // was not found" on stderr); installed ones still print to stdout
        if !output.status.success() && output.stdout.is_empty() && pkgs.len() > 1 {
            debug!(
                "pacman -Q: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_pacman_query(&stdout, pkgs))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.update || self.dbs_missing() {
            self.refresh(opts)?;
        }
        let mut args = vec![
            "-S".to_string(),
            "--noconfirm".to_string(),
            "--needed".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| p.raw.clone()));
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("pacman", &args).join(" "));
            return Ok(());
        }
        sudo::run("pacman", &args, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pacman_query() {
        let mgr = PacmanManager::new();
        let requests = vec![mgr.parse_request("bc"), mgr.parse_request("nonexistent")];
        let output = "bc 1.08.2-1\n";
        let statuses = parse_pacman_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.08.2-1".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
    }
}
