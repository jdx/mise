use std::path::{Path, PathBuf};

use contracts::requires;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;
use crate::{file, github, plugins};

#[derive(Debug)]
pub struct BunPlugin {
    ba: BackendArg,
}

impl BunPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("bun"),
        }
    }

    fn bun_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(bun_bin_name())
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
                .join(bun_bin_name()),
            self.bun_bin(&ctx.tv),
        )?;
        if cfg!(unix) {
            file::make_executable(self.bun_bin(&ctx.tv))?;
            file::make_symlink(Path::new("./bun"), &ctx.tv.install_path().join("bin/bunx"))?;
        }
        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_bun(ctx)
    }
}

impl Backend for BunPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        let versions = github::list_releases("oven-sh/bun")?
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| v.strip_prefix("bun-v").map(|v| v.to_string()))
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into()])
    }

    #[requires(matches!(ctx.tv.request, ToolRequest::Version { .. } | ToolRequest::Prefix { .. }), "unsupported tool version request type")]
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
    if cfg!(target_arch = "x86_64") {
        if cfg!(target_feature = "avx2") {
            "x64"
        } else {
            "x64-baseline"
        }
    } else if cfg!(target_arch = "aarch64") {
        if cfg!(windows) {
            "x64"
        } else {
            "aarch64"
        }
    } else {
        &ARCH
    }
}

fn bun_bin_name() -> &'static str {
    if cfg!(windows) {
        "bun.exe"
    } else {
        "bun"
    }
}
