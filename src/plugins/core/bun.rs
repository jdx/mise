use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{backend::Backend, config::Config};
use crate::{file, github, plugins};

#[derive(Debug)]
pub struct BunPlugin {
    ba: Arc<BackendArg>,
}

impl BunPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("bun")),
        }
    }

    fn bun_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(bun_bin_name())
    }

    fn test_bun(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("bun -v".into());
        CmdLineRunner::new(self.bun_bin(tv))
            .with_pr(&ctx.pr)
            .arg("-v")
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<PathBuf> {
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/bun-{}-{}.zip",
            tv.version,
            os(),
            arch()
        );
        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr)).await?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path())?;
        file::rename(
            tv.download_path()
                .join(format!("bun-{}-{}", os(), arch()))
                .join(bun_bin_name()),
            self.bun_bin(tv),
        )?;
        if cfg!(unix) {
            file::make_executable(self.bun_bin(tv))?;
            file::make_symlink(Path::new("./bun"), &tv.install_path().join("bin/bunx"))?;
        }
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_bun(ctx, tv)
    }
}

#[async_trait]
impl Backend for BunPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let versions = github::list_releases("oven-sh/bun")
            .await?
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| v.strip_prefix("bun-v").map(|v| v.to_string()))
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into()])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, &ctx.pr).await?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(ctx, &tv, &tarball_path)?;
        self.verify(ctx, &tv)?;

        Ok(tv)
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
    if cfg!(target_arch = "x86_64") {
        if cfg!(target_feature = "avx2") {
            "x64"
        } else {
            "x64-baseline"
        }
    } else if cfg!(target_arch = "aarch64") {
        if cfg!(windows) { "x64" } else { "aarch64" }
    } else {
        &ARCH
    }
}

fn bun_bin_name() -> &'static str {
    if cfg!(windows) { "bun.exe" } else { "bun" }
}
