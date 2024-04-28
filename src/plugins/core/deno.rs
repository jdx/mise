use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::args::ForgeArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::file;
use crate::forge::Forge;
use crate::github::GithubRelease;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::SingleReport;

#[derive(Debug)]
pub struct DenoPlugin {
    core: CorePlugin,
}

impl DenoPlugin {
    pub fn new() -> Self {
        let core = CorePlugin::new("deno");
        Self { core }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        let releases: Vec<GithubRelease> =
            HTTP_FETCH.json("https://api.github.com/repos/denoland/deno/releases?per_page=100")?;
        let versions = releases
            .into_iter()
            .map(|r| r.name)
            .filter(|v| !v.is_empty())
            .filter(|v| v.starts_with('v'))
            .map(|v| v.trim_start_matches('v').to_string())
            .unique()
            .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
            .collect();
        Ok(versions)
    }

    fn deno_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/deno")
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
            "https://github.com/denoland/deno/releases/download/v{}/deno-{}-{}.zip",
            tv.version,
            arch(),
            os()
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr))?;

        // TODO: hash::ensure_checksum_sha256(&tarball_path, &m.sha256)?;

        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &dyn SingleReport, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("installing {filename}"));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path())?;
        file::rename(tv.download_path().join("deno"), self.deno_bin(tv))?;
        file::make_executable(&self.deno_bin(tv))?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        self.test_deno(tv, pr)
    }
}

impl Forge for DenoPlugin {
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
        Ok(vec![".deno-version".into()])
    }

    #[requires(matches!(ctx.tv.request, ToolVersionRequest::Version { .. } | ToolVersionRequest::Prefix { .. }), "unsupported tool version request type")]
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
        self.install(&ctx.tv, ctx.pr.as_ref(), &tarball_path)?;
        self.verify(&ctx.tv, ctx.pr.as_ref())?;

        Ok(())
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        if let ToolVersionRequest::System(_) = tv.request {
            return Ok(vec![]);
        }
        let bin_paths = vec![
            tv.install_short_path().join("bin"),
            tv.install_short_path().join(".deno/bin"),
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
    } else {
        &OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else {
        &ARCH
    }
}
