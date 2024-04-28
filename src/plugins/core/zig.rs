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
pub struct ZigPlugin {
    core: CorePlugin,
}

impl ZigPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("zig");
        Self { core }
    }

    fn zig_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("zig")
    }

    fn test_zig(&self, ctx: &InstallContext) -> Result<()> {
        ctx.pr.set_message("zig version".into());
        CmdLineRunner::new(self.zig_bin(&ctx.tv))
            .with_pr(ctx.pr.as_ref())
            .arg("version")
            .execute()
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }

        let releases: Vec<GithubRelease> =
            HTTP_FETCH.json("https://api.github.com/repos/ziglang/zig/releases?per_page=100")?;
        let versions = releases
            .into_iter()
            .map(|r| r.tag_name)
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = if tv.version == "ref:master" {
            format!(
                "https://ziglang.org/builds/zig-{}-{}-{}.tar.xz",
                os(),
                arch(),
                self.get_master_version()?
            )
        } else {
            format!(
                "https://ziglang.org/download/{}/zig-{}-{}-{}.tar.xz",
                tv.version,
                os(),
                arch(),
                tv.version
            )
        };

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
        untar_xy(tarball_path, &ctx.tv.download_path())?;
        file::rename(
            ctx.tv.download_path().join(format!(
                "zig-{}-{}-{}",
                os(),
                arch(),
                if ctx.tv.version == "ref:master" {
                    self.get_master_version()?
                } else {
                    ctx.tv.version.clone()
                }
            )),
            ctx.tv.install_path(),
        )?;
        file::create_dir_all(ctx.tv.install_path().join("bin"))?;
        file::make_symlink(
            self.zig_bin(&ctx.tv).as_path(),
            &ctx.tv.install_path().join("bin/zig"),
        )?;

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext) -> Result<()> {
        self.test_zig(ctx)
    }

    fn get_master_version(&self) -> Result<String> {
        let version_json: serde_json::Value =
            HTTP_FETCH.json("https://ziglang.org/download/index.json")?;
        let master_version = version_json
            .pointer("/master/version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Failed to get master version"))?;
        Ok(master_version.to_string())
    }
}

impl Forge for ZigPlugin {
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
        Ok(vec![".zig-version".into()])
    }
    #[requires(matches ! (ctx.tv.request, ToolVersionRequest::Version { .. } | ToolVersionRequest::Prefix { .. } | ToolVersionRequest::Ref { .. }), "unsupported tool version request type")]
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
        self.install(ctx, &tarball_path)?;
        self.verify(ctx)?;
        Ok(())
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "x86_64"
    } else if cfg!(target_arch = "i686") || cfg!(target_arch = "i386") || cfg!(target_arch = "386")
    {
        "i386"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else if cfg!(target_arch = "armv7a") {
        "armv7a"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else {
        &ARCH
    }
}

pub fn untar_xy(archive: &Path, dest: &Path) -> Result<()> {
    let archive = archive
        .to_str()
        .ok_or(eyre::eyre!("Failed to read archive path"))?;
    let dest = dest
        .to_str()
        .ok_or(eyre::eyre!("Failed to read destination path"))?;

    let output = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(eyre::eyre!("Failed to extract tar: {}", err));
    }

    Ok(())
}
