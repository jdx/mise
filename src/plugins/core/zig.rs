use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::OS;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::TarOptions;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{file, minisign, plugins};
use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct ZigPlugin {
    ba: Arc<BackendArg>,
}

const ZIG_MINISIGN_KEY: &str = "RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U";

impl ZigPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("zig")),
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

    async fn download(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> Result<PathBuf> {
        let settings = Settings::get();
        let indexes = HashMap::from([
            ("zig", "https://ziglang.org/download/index.json"),
            ("mach", "https://machengine.org/zig/index.json"),
        ]);

        let url = if regex!(r"^mach-|-mach$").is_match(&tv.version) {
            self.get_tarball_url_from_json(
                indexes["mach"],
                tv.version.as_str(),
                arch(&settings),
                os(),
            )
            .await?
        } else {
            self.get_tarball_url_from_json(
                indexes["zig"],
                tv.version.as_str(),
                arch(&settings),
                os(),
            )
            .await?
        };

        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr)).await?;

        pr.set_message(format!("minisign {filename}"));
        let tarball_data = file::read(&tarball_path)?;
        let sig = HTTP.get_text(format!("{url}.minisig")).await?;
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

    async fn get_tarball_url_from_json(
        &self,
        json_url: &str,
        version: &str,
        arch: &str,
        os: &str,
    ) -> Result<String> {
        let version_json: serde_json::Value = HTTP_FETCH.json(json_url).await?;
        let zig_tarball_url = version_json
            .pointer(&format!("/{version}/{arch}-{os}/tarball"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| eyre::eyre!("Failed to get zig tarball url from {:?}", json_url))?;
        Ok(zig_tarball_url.to_string())
    }
}

#[async_trait]
impl Backend for ZigPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let indexes = [
            "https://ziglang.org/download/index.json",
            // "https://machengine.org/zig/index.json", // need to handle mach's CalVer
        ];
        let mut versions: Vec<String> = Vec::new();

        for index in indexes {
            let index_json: serde_json::Value = HTTP_FETCH.json(index).await?;
            let index_versions: Vec<String> = index_json
                .as_object()
                .ok_or_else(|| eyre::eyre!("Failed to get zig version from {:?}", index))?
                .keys()
                .cloned()
                .collect();

            versions.extend(index_versions);
        }

        let versions = versions
            .into_iter()
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();

        Ok(versions)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        if cfg!(windows) {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".zig-version".into()])
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
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        &OS
    }
}

fn arch(settings: &Settings) -> &str {
    let arch = settings.arch();
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
