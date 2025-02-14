use std::path::{Path, PathBuf};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::OS;
use crate::cmd::CmdLineRunner;
use crate::config::SETTINGS;
use crate::file::TarOptions;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::ui::progress_report::SingleReport;
use crate::{file, github, minisign, plugins};
use contracts::requires;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct ZigPlugin {
    ba: BackendArg,
}

const ZIG_MINISIGN_KEY: &str = "RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U";

impl ZigPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("zig"),
        }
    }

    fn zig_bin(&self, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join("zig.exe")
        } else {
            tv.install_path().join("bin").join("zig")
        }
    }

    fn test_zig(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("zig version".into());
        CmdLineRunner::new(self.zig_bin(tv))
            .with_pr(&ctx.pr)
            .arg("version")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<PathBuf> {
        let archive_ext = if cfg!(target_os = "windows") {
            "zip"
        } else {
            "tar.xz"
        };
        let url = if tv.version == "ref:master" {
            format!(
                "https://ziglang.org/builds/zig-{}-{}-{}.{archive_ext}",
                os(),
                arch(),
                self.get_master_version()?
            )
        } else if regex!(r"^[0-9]+\.[0-9]+\.[0-9]+-dev.[0-9]+\+[0-9a-f]+$").is_match(&tv.version) {
            format!(
                "https://pkg.machengine.org/zig/zig-{}-{}-{}.{archive_ext}",
                os(),
                arch(),
                tv.version
            )
        } else {
            format!(
                "https://ziglang.org/download/{}/zig-{}-{}-{}.{archive_ext}",
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

        pr.set_message(format!("minisign {filename}"));
        let tarball_data = file::read(&tarball_path)?;
        let sig = HTTP.get_text(format!("{url}.minisig"))?;
        minisign::verify(ZIG_MINISIGN_KEY, &tarball_data, &sig)?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::untar(
            tarball_path,
            &tv.install_path(),
            &TarOptions {
                strip_components: 1,
                pr: Some(&ctx.pr),
                ..Default::default()
            },
        )?;

        if cfg!(unix) {
            file::create_dir_all(tv.install_path().join("bin"))?;
            file::make_symlink(Path::new("../zig"), &tv.install_path().join("bin/zig"))?;
        }

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
        let versions: Vec<String> = github::list_releases("ziglang/zig")?
            .into_iter()
            .map(|r| r.tag_name)
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        if cfg!(windows) {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".zig-version".into()])
    }

    #[requires(matches!(tv.request, ToolRequest::Version { .. } | ToolRequest::Prefix { .. } | ToolRequest::Ref { .. }), "unsupported tool version request type")]
    fn install_version_(&self, ctx: &InstallContext, mut tv: ToolVersion) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, &ctx.pr)?;
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
    let arch = SETTINGS.arch();
    if arch == "x86_64" {
        "x86_64"
    } else if arch == "aarch64" {
        "aarch64"
    } else if arch == "arm" {
        "armv7a"
    } else if arch == "riscv64" {
        "riscv64"
    } else {
        arch
    }
}
