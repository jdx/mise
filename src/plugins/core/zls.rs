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
pub struct ZlsPlugin {
    ba: BackendArg,
}

const MINISIGN_KEY: &str = "RWR+9B91GBZ0zOjh6Lr17+zKf5BoSuFvrx2xSeDE57uIYvnKBGmMjOex"

impl ZlsPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("zls"),
        }
    }

    fn bin_path(&self, bin_name: &str, tv: &ToolVersion) -> PathBuf {
        if cfg!(windows) {
            tv.install_path().join(bin_name + ".exe")
        } else {
            tv.install_path().join("bin").join(bin_name)
        }
    }
   
    fn bin_version(&self, bin_name: &str, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message((bin_name + "version").into());
        CmdLineRunner::new(self.bin_path(bin_name, tv))
            .with_pr(&ctx.pr)
            .arg("version")
            .execute()
    }

    fn download(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<PathBuf> {
        let archive_ext = if cfg!(target_os = "windows") {
            "zip"
        } else {
            "tar.xz"
        };

        let url = if tv.version == "ref:zig" {
            format!(
                "",
                os(),
                arch(),
                self.get_version_from_zig?(tv, &ctx.pr)
            )
        } else {
            ""
        }
         else {
        //     format!(
        //         "https://ziglang.org/download/{}/zig-{}-{}-{}.{archive_ext}",
        //         tv.version,
        //         os(),
        //         arch(),
        //         tv.version
        //     )
        // };

        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        pr.set_message(format!("minisign {filename}"));
        let tarball_data = file::read(&tarball_path)?;
        let sig = HTTP.get_text(format!("{url}.minisig"))?;
        minisign::verify(MINISIGN_KEY, &tarball_data, &sig)?;

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
            file::make_symlink(Path::new("../zls"), &tv.install_path().join("bin/zls"))?;
        }

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.bin_version("zls", ctx, tv)
    }

    fn get_version_from_zig(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<String> {
        let json_url = format!("https://releases.zigtools.org/v1/zls/select-version?zig_version={}&compatibility=only-runtime", self.bin_version("zig", ctx, tv))

        let version_json: serde_json::Value = HTTP_FETCH.json(json_url)?;
        print!("version_json: {}", version_json)

        // let zls_version = version_json
        //     .pointer(&format!("/{key}/version"))
        //     .and_then(|v| v.as_str())
        //     .ok_or_else(|| eyre::eyre!("Failed to get zls version from {:?}", json_url))?;
        // Ok(zls_version.to_string())
    }
}

impl Backend for ZlsPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        print!("Check versions")
        // let versions: Vec<String> = github::list_releases("zigtools/zls")?
        //     .into_iter()
        //     .map(|r| r.tag_name)
        //     .unique()
        //     .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
        //     .collect();
        // Ok(versions)
        OK("")
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        if cfg!(windows) {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
    }

    fn idiomatic_filenames(&self) -> Result {
        Ok()
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
