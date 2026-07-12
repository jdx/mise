use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;

/// Flatpak applications and runtimes installed system-wide.
pub struct FlatpakManager {}

impl FlatpakManager {
    pub fn new() -> Self {
        Self {}
    }
}

fn parse_flatpak_list(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let installed: HashMap<&str, &str> = output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    requests
        .iter()
        .map(|request| {
            let state = match installed.get(request.name.as_str()) {
                Some(version) => PackageState::Installed {
                    version: if version.is_empty() {
                        "unknown"
                    } else {
                        version
                    }
                    .to_string(),
                },
                None => PackageState::Missing,
            };
            PackageStatus {
                request: request.clone(),
                state,
            }
        })
        .collect()
}

async fn run_flatpak(args: &[String], action: &str) -> Result<()> {
    debug!("$ flatpak {}", args.join(" "));
    let output = tokio::process::Command::new("flatpak")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("flatpak {action} failed: {}", stderr.trim());
    }
    Ok(())
}

#[async_trait(?Send)]
impl SystemPackageManager for FlatpakManager {
    fn name(&self) -> &'static str {
        "flatpak"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("flatpak").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "flatpak not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let args = ["list", "--system", "--columns=application,version"];
        debug!("$ flatpak {}", args.join(" "));
        let output = tokio::process::Command::new("flatpak")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("flatpak list failed: {}", stderr.trim());
        }
        Ok(parse_flatpak_list(
            &String::from_utf8_lossy(&output.stdout),
            pkgs,
        ))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if let Some(pkg) = pkgs.iter().find(|pkg| pkg.version.is_some()) {
            bail!("flatpak cannot install a pinned version ('{pkg}')");
        }
        let mut args = vec![
            "install".to_string(),
            "--system".to_string(),
            "--noninteractive".to_string(),
        ];
        args.extend(pkgs.iter().map(|pkg| pkg.name.clone()));
        if opts.dry_run {
            miseprintln!("flatpak {}", args.join(" "));
            return Ok(());
        }
        run_flatpak(&args, "install").await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut args = vec![
            "update".to_string(),
            "--system".to_string(),
            "--noninteractive".to_string(),
        ];
        args.extend(pkgs.iter().map(|pkg| pkg.name.clone()));
        if opts.dry_run {
            miseprintln!("flatpak {}", args.join(" "));
            return Ok(());
        }
        run_flatpak(&args, "update").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(name: &str) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: None,
            tap_url: None,
        }
    }

    #[test]
    fn test_parse_flatpak_list() {
        let statuses = parse_flatpak_list(
            "org.mozilla.firefox\t128.0\norg.freedesktop.Platform\t24.08.1\n",
            &[req("org.mozilla.firefox"), req("org.gnome.Builder")],
        );
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "128.0".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
    }
}
