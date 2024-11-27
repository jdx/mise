use std::path::{Path, PathBuf};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::github::GithubRelease;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;
use crate::{file, plugins};
use contracts::requires;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct ZigPlugin {
    ba: BackendArg,
}

impl ZigPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("zig"),
        }
    }

    fn zig_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("zig")
    }

    fn test_zig(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("zig version".into());
        CmdLineRunner::new(self.zig_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("version")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = if tv.version == "ref:master" {
            format!(
                "https://ziglang.org/builds/zig-{}-{}-{}.tar.xz",
                os(),
                arch(),
                self.get_master_version()?
            )
        } else if regex!(r"^[0-9]+\.[0-9]+\.[0-9]+-dev.[0-9]+\+[0-9a-f]+$").is_match(&tv.version) {
            format!(
                "https://ziglang.org/builds/zig-{}-{}-{}.tar.xz",
                os(),
                arch(),
                tv.version
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

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        untar_xy(tarball_path, &tv.download_path())?;
        file::rename(
            tv.download_path().join(format!(
                "zig-{}-{}-{}",
                os(),
                arch(),
                if tv.version == "ref:master" {
                    self.get_master_version()?
                } else {
                    tv.version.clone()
                }
            )),
            tv.install_path(),
        )?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::make_symlink(
            self.zig_bin(tv).as_path(),
            &tv.install_path().join("bin/zig"),
        )?;

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_zig(ctx, tv)
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

impl Backend for ZigPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
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

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".zig-version".into()])
    }

    #[requires(matches!(tv.request, ToolRequest::Version { .. } | ToolRequest::Prefix { .. } | ToolRequest::Ref { .. }), "unsupported tool version request type")]
    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref())?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(ctx, &tv, &tarball_path)?;
        self.verify(ctx, &tv)?;
        Ok(tv)
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
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
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
