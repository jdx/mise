use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::github::GithubRelease;
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::plugins::{Plugin, HTTP};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;
use crate::{file, http};

#[derive(Debug)]
pub struct BunPlugin {
    core: CorePlugin,
}

impl BunPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("bun");
        Self { core }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_rtx() {
            Ok(versions) => return Ok(versions),
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        let releases: Vec<GithubRelease> =
            HTTP.json("https://api.github.com/repos/oven-sh/bun/releases?per_page=100")?;
        let versions = releases
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| v.strip_prefix("bun-v").map(|v| v.to_string()))
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn bun_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/bun")
    }

    fn test_bun(&self, ctx: &InstallContext) -> Result<()> {
        ctx.pr.set_message("bun -v");
        CmdLineRunner::new(&ctx.config.settings, self.bun_bin(&ctx.tv))
            .with_pr(&ctx.pr)
            .arg("-v")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &ProgressReport) -> Result<PathBuf> {
        let http = http::Client::new()?;
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/bun-{}-{}.zip",
            tv.version,
            os(),
            arch()
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {}", &url));
        http.download_file(&url, &tarball_path)?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tarball_path: &Path) -> Result<()> {
        ctx.pr
            .set_message(format!("installing {}", tarball_path.display()));
        file::remove_all(ctx.tv.install_path())?;
        file::create_dir_all(ctx.tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &ctx.tv.download_path())?;
        file::rename(
            ctx.tv
                .download_path()
                .join(format!("bun-{}-{}", os(), arch()))
                .join("bun"),
            self.bun_bin(&ctx.tv),
        )?;
        file::make_executable(&self.bun_bin(&ctx.tv))?;
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_bun(ctx)
    }
}

impl Plugin for BunPlugin {
    fn name(&self) -> &str {
        "bun"
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into()])
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        assert!(matches!(
            &ctx.tv.request,
            ToolVersionRequest::Version { .. }
        ));

        let tarball_path = self.download(&ctx.tv, &ctx.pr)?;
        self.install(ctx, &tarball_path)?;
        self.verify(ctx)?;

        Ok(())
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else {
        &ARCH
    }
}
