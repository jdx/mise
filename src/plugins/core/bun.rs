use std::path::{Path, PathBuf};

use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::args::ForgeArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::file;
use crate::forge::Forge;
use crate::github::GithubRelease;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::SingleReport;

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
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        let releases: Vec<GithubRelease> =
            HTTP_FETCH.json("https://api.github.com/repos/oven-sh/bun/releases?per_page=100")?;
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
        ctx.pr.set_message("bun -v".into());
        CmdLineRunner::new(self.bun_bin(&ctx.tv))
            .with_pr(ctx.pr.as_ref())
            .arg("-v")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/bun-{}-{}.zip",
            tv.version,
            os(),
            arch()
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("installing {filename}"));
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
        file::make_symlink(Path::new("./bun"), &ctx.tv.install_path().join("bin/bunx"))?;
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_bun(ctx)
    }
}

impl Forge for BunPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into()])
    }

    #[requires(matches!(ctx.tv.request, ToolVersionRequest::Version { .. } | ToolVersionRequest::Prefix { .. }), "unsupported tool version request type")]
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
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
        if cfg!(target_feature = "avx2") {
            "x64"
        } else {
            "x64-baseline"
        }
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else {
        &ARCH
    }
}
