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
    let mut installed: HashMap<&str, Vec<&str>> = HashMap::new();
    for (application, version) in output.lines().filter_map(|line| line.split_once('\t')) {
        installed.entry(application).or_default().push(version);
    }
    requests
        .iter()
        .map(|request| {
            let state = match installed.get_mut(request.name.as_str()) {
                Some(versions) => {
                    versions.sort_unstable();
                    versions.dedup();
                    let installed = versions
                        .iter()
                        .map(|version| {
                            if version.is_empty() {
                                "unknown"
                            } else {
                                version
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    match &request.version {
                        Some(requested) if !versions.contains(&requested.as_str()) => {
                            PackageState::VersionMismatch { installed }
                        }
                        _ => PackageState::Installed { version: installed },
                    }
                }
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
    fn name(&self) -> &str {
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
        if pkgs.is_empty() {
            return Ok(());
        }
        if let Some(pkg) = pkgs.iter().find(|pkg| pkg.version.is_some()) {
            bail!("flatpak cannot upgrade a pinned version ('{pkg}')");
        }
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

    fn req(name: &str, version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
        }
    }

    #[test]
    fn test_parse_flatpak_list() {
        let statuses = parse_flatpak_list(
            "org.mozilla.firefox\t128.0\norg.freedesktop.Platform\t24.08.1\n",
            &[
                req("org.mozilla.firefox", None),
                req("org.gnome.Builder", None),
            ],
        );
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "128.0".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
    }

    #[test]
    fn test_parse_flatpak_list_versions() {
        let statuses = parse_flatpak_list(
            "org.gnome.Sdk\t44.2\norg.gnome.Sdk\t43.1\norg.gnome.Sdk\t44.2\n",
            &[
                req("org.gnome.Sdk", None),
                req("org.gnome.Sdk", Some("43.1")),
                req("org.gnome.Sdk", Some("45.0")),
            ],
        );
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "43.1, 44.2".to_string()
            }
        );
        assert_eq!(
            statuses[1].state,
            PackageState::Installed {
                version: "43.1, 44.2".to_string()
            }
        );
        assert_eq!(
            statuses[2].state,
            PackageState::VersionMismatch {
                installed: "43.1, 44.2".to_string()
            }
        );
    }

    #[tokio::test]
    async fn test_upgrade_rejects_empty_and_pinned_requests_before_running_flatpak() {
        let manager = FlatpakManager::new();
        manager.upgrade(&[], &InstallOpts::default()).await.unwrap();

        let err = manager
            .upgrade(
                &[req("org.mozilla.firefox", Some("128.0"))],
                &InstallOpts::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "flatpak cannot upgrade a pinned version ('org.mozilla.firefox@128.0')"
        );
    }
}
