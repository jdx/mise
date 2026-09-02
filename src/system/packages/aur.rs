use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::cmd::CmdLineRunner;
use crate::result::Result;

/// Arch User Repository packages via yay or paru.
pub(crate) struct AurManager {}

impl AurManager {
    pub(crate) fn new() -> Self {
        Self {}
    }

    fn helper(&self) -> Option<&'static str> {
        ["yay", "paru"]
            .into_iter()
            .find(|helper| crate::file::which(helper).is_some())
    }
}

fn install_args(pkgs: &[PackageRequest], opts: &InstallOpts) -> Vec<String> {
    let mut args = vec![
        "-S".to_string(),
        "--aur".to_string(),
        "--noconfirm".to_string(),
        "--needed".to_string(),
    ];
    if opts.update {
        args.push("--refresh".to_string());
    }
    args.push("--".to_string());
    args.extend(pkgs.iter().map(|pkg| pkg.name.clone()));
    args
}

fn parse_foreign_packages(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let installed = output
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect::<HashMap<_, _>>();
    requests
        .iter()
        .map(|request| {
            let state = match installed.get(request.name.as_str()) {
                Some(version) => match &request.version {
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
                request: request.clone(),
                state,
            }
        })
        .collect()
}

async fn foreign_packages() -> Result<String> {
    let args = ["-Qm"];
    debug!("$ pacman {}", args.join(" "));
    let output = tokio::process::Command::new("pacman")
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        bail!(
            "pacman -Qm failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[async_trait(?Send)]
impl SystemPackageManager for AurManager {
    fn name(&self) -> &str {
        "aur"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux")
            && crate::file::which("pacman").is_some()
            && self.helper().is_some()
    }

    fn unavailable_reason(&self) -> String {
        if !cfg!(target_os = "linux") {
            "only available on linux".to_string()
        } else if crate::file::which("pacman").is_none() {
            "pacman not found".to_string()
        } else {
            "neither yay nor paru found".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let output = foreign_packages().await?;
        Ok(parse_foreign_packages(&output, pkgs))
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if let Some(pkg) = pkgs.iter().find(|pkg| pkg.version.is_some()) {
            bail!("AUR helpers cannot install a pinned version ('{pkg}')");
        }
        let helper = self
            .helper()
            .ok_or_else(|| eyre::eyre!(self.unavailable_reason()))?;
        let args = install_args(pkgs, opts);
        let command = std::iter::once(helper.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>();
        if opts.dry_run {
            miseprintln!("{}", shell_words::join(command));
            return Ok(());
        }
        if crate::system::sudo::is_root() {
            bail!("AUR packages cannot be built as root; run mise as a non-root user");
        }
        let mut runner = CmdLineRunner::new(helper);
        for arg in &args {
            runner = runner.arg(arg);
        }
        runner.raw(true).execute()
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install(
            pkgs,
            &InstallOpts {
                dry_run: opts.dry_run,
                update: true,
            },
        )
        .await
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
    fn test_install_args_force_aur_targets() {
        let pkgs = vec![
            req("google-chrome", None),
            req("visual-studio-code-bin", None),
        ];
        let args = install_args(&pkgs, &InstallOpts::default());
        assert_eq!(
            args,
            vec![
                "-S",
                "--aur",
                "--noconfirm",
                "--needed",
                "--",
                "google-chrome",
                "visual-studio-code-bin"
            ]
        );
    }

    #[test]
    fn test_install_args_update_refreshes_metadata() {
        let pkgs = vec![req("google-chrome", None)];
        let args = install_args(
            &pkgs,
            &InstallOpts {
                dry_run: false,
                update: true,
            },
        );
        assert_eq!(
            args,
            vec![
                "-S",
                "--aur",
                "--noconfirm",
                "--needed",
                "--refresh",
                "--",
                "google-chrome"
            ]
        );
    }

    #[test]
    fn test_installed_state_only_uses_foreign_query_results() {
        let requests = vec![req("aur-package", None), req("native-name-collision", None)];
        // pacman -Qm omits packages available from a configured sync database,
        // even if a same-named native package is installed.
        let statuses = parse_foreign_packages("aur-package 1.2.3-1\n", &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.2.3-1".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
    }
}
