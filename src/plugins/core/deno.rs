use std::collections::HashMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::github::GithubRelease;
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::ProgressReport;
use crate::{file, http};

#[derive(Debug)]
pub struct DenoPlugin {
    core: CorePlugin,
}

impl DenoPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("deno");
        Self { core }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let http = http::Client::new()?;
        let releases: Vec<GithubRelease> =
            http.json("https://api.github.com/repos/denoland/deno/releases?per_page=100")?;
        let versions = releases
            .into_iter()
            .map(|r| r.name)
            .filter(|v| !v.is_empty())
            .filter(|v| v.starts_with('v'))
            .map(|v| v.trim_start_matches('v').to_string())
            .unique()
            .sorted_by_cached_key(|s| Versioning::new(s))
            .collect();
        Ok(versions)
    }

    fn deno_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/deno")
    }

    fn test_deno(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("deno -V");
        CmdLineRunner::new(&config.settings, self.deno_bin(tv))
            .with_pr(pr)
            .arg("-V")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &ProgressReport) -> Result<PathBuf> {
        let http = http::Client::new()?;
        let url = format!(
            "https://github.com/denoland/deno/releases/download/v{}/deno-{}-{}.zip",
            tv.version,
            arch(),
            os()
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {}", &url));
        http.download_file(&url, &tarball_path)?;

        // TODO: hash::ensure_checksum_sha256(&tarball_path, &m.sha256)?;

        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &ProgressReport, tarball_path: &Path) -> Result<()> {
        pr.set_message(format!("installing {}", tarball_path.display()));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path())?;
        file::rename(tv.download_path().join("deno"), self.deno_bin(tv))?;
        file::make_executable(&self.deno_bin(tv))?;
        Ok(())
    }

    fn verify(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        self.test_deno(config, tv, pr)
    }
}

impl Plugin for DenoPlugin {
    fn name(&self) -> &str {
        "deno"
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![".deno-version".into()])
    }

    fn install_version(&self, ctx: &InstallContext) -> Result<()> {
        assert!(matches!(
            &ctx.tv.request,
            ToolVersionRequest::Version { .. }
        ));

        let tarball_path = self.download(&ctx.tv, &ctx.pr)?;
        self.install(&ctx.tv, &ctx.pr, &tarball_path)?;
        self.verify(ctx.config, &ctx.tv, &ctx.pr)?;

        Ok(())
    }

    fn list_bin_paths(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let bin_paths = vec![
            tv.install_short_path().join("bin"),
            tv.install_short_path().join(".deno/bin"),
        ];
        Ok(bin_paths)
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<HashMap<String, String>> {
        let map = HashMap::from([(
            "DENO_INSTALL_ROOT".into(),
            tv.install_path().join(".deno").to_string_lossy().into(),
        )]);
        Ok(map)
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else {
        &ARCH
    }
}
