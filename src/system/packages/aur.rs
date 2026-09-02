use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageStatus, SystemPackageManager};
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
        super::pacman::PacmanManager::new().installed(pkgs).await
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
}
