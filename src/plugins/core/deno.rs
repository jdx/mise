use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::Result;
use itertools::Itertools;
use serde::Deserialize;
use versions::Versioning;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{file, plugins};

#[derive(Debug)]
pub struct DenoPlugin {
    ba: BackendArg,
}

impl DenoPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("deno"),
        }
    }

    fn deno_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join(if cfg!(target_os = "windows") {
            "bin/deno.exe"
        } else {
            "bin/deno"
        })
    }

    fn test_deno(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("deno -V".into());
        CmdLineRunner::new(self.deno_bin(tv))
            .with_pr(pr)
            .arg("-V")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = format!(
            "https://dl.deno.land/release/v{}/deno-{}-{}.zip",
            tv.version,
            arch(),
            os()
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        // TODO: hash::ensure_checksum_sha256(&tarball_path, &m.sha256)?;

        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &dyn SingleReport, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path())?;
        file::rename(
            tv.download_path().join(if cfg!(target_os = "windows") {
                "deno.exe"
            } else {
                "deno"
            }),
            self.deno_bin(tv),
        )?;
        file::make_executable(self.deno_bin(tv))?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        self.test_deno(tv, pr)
    }
}

impl Backend for DenoPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        let versions: DenoVersions = HTTP_FETCH.json("https://deno.com/versions.json")?;
        let versions = versions
            .cli
            .into_iter()
            .filter(|v| v.starts_with('v'))
            .map(|v| v.trim_start_matches('v').to_string())
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".deno-version".into()])
    }

    fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref())?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(&tv, ctx.pr.as_ref(), &tarball_path)?;
        self.verify(&tv, ctx.pr.as_ref())?;

        Ok(tv)
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        if let ToolRequest::System { .. } = tv.request {
            return Ok(vec![]);
        }
        let bin_paths = vec![
            tv.install_path().join("bin"),
            tv.install_path().join(".deno/bin"),
        ];
        Ok(bin_paths)
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let map = BTreeMap::from([(
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
    } else if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        &ARCH
    }
}

#[derive(Debug, Deserialize)]
struct DenoVersions {
    cli: Vec<String>,
}
