use std::sync::Arc;

use async_trait::async_trait;

use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;

/// Winget backend requires experimental mode to be enabled
pub const EXPERIMENTAL: bool = true;

#[derive(Debug)]
pub struct WingetBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for WingetBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Winget
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        self.warn_if_dependency_missing(
            config,
            "winget",
            "winget is required for the winget backend.\n\
            Install it from https://github.com/microsoft/winget-cli or via the Microsoft Store.",
        )
        .await;

        let raw = cmd!(
            "winget",
            "show",
            "--id",
            self.tool_name(),
            "--versions",
            "--disable-interactivity",
            "--accept-source-agreements"
        )
        .read()?;

        let versions = parse_winget_versions(&raw);
        Ok(versions)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        Settings::get().ensure_experimental("winget backend")?;

        self.warn_if_dependency_missing(
            &ctx.config,
            "winget",
            "winget is required for the winget backend.\n\
            Install it from https://github.com/microsoft/winget-cli or via the Microsoft Store.",
        )
        .await;

        let mut cmd = CmdLineRunner::new("winget")
            .arg("install")
            .arg("--id")
            .arg(self.tool_name())
            .arg("--location")
            .arg(tv.install_path())
            .arg("--disable-interactivity")
            .arg("--accept-source-agreements")
            .arg("--accept-package-agreements");

        if tv.version != "latest" {
            cmd = cmd.arg("--version").arg(&tv.version);
        }

        cmd.with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env(&ctx.config).await?)
            .execute()?;

        Ok(tv)
    }

    async fn install_operation_count(&self, _tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        2
    }
}

impl WingetBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }
}

/// Parse the output of `winget show --id <id> --versions` into version info.
fn parse_winget_versions(output: &str) -> Vec<VersionInfo> {
    let mut versions = Vec::new();
    let mut past_separator = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("---") {
            past_separator = true;
            continue;
        }
        if past_separator && !trimmed.is_empty() {
            versions.push(VersionInfo {
                version: trimmed.to_string(),
                ..Default::default()
            });
        }
    }

    // winget outputs newest first, mise expects oldest first
    versions.reverse();
    versions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_winget_versions() {
        let output = "\
Found Python 3 [Python.Python.3.12]
Version
---------
3.12.5
3.12.4
3.12.3
3.12.2
3.12.1
3.12.0
";
        let versions = parse_winget_versions(output);
        assert_eq!(versions.len(), 6);
        assert_eq!(versions[0].version, "3.12.0");
        assert_eq!(versions[5].version, "3.12.5");
    }

    #[test]
    fn test_parse_winget_versions_empty() {
        let output = "";
        let versions = parse_winget_versions(output);
        assert!(versions.is_empty());
    }

    #[test]
    fn test_parse_winget_versions_no_separator() {
        let output = "No package found matching input criteria.";
        let versions = parse_winget_versions(output);
        assert!(versions.is_empty());
    }
}
