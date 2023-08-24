use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::github::GithubRelease;
use crate::plugins::core::CorePlugin;
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;
use crate::{env, file, http};

#[derive(Debug)]
pub struct BunPlugin {
    core: CorePlugin,
}

impl BunPlugin {
    pub fn new(name: PluginName) -> Self {
        let core = CorePlugin::new(name);
        Self { core }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let http = http::Client::new()?;
        let mut req = http.get("https://api.github.com/repos/oven-sh/bun/releases?per_page=100");
        if let Some(token) = &*env::GITHUB_API_TOKEN {
            req = req.header("authorization", format!("token {}", token));
        }
        let resp = req.send()?;
        http.ensure_success(&resp)?;
        let releases: Vec<GithubRelease> = resp.json()?;
        let versions = releases
            .into_iter()
            .map(|r| r.tag_name)
            .filter_map(|v| v.strip_prefix("bun-v").map(|v| v.to_string()))
            .unique()
            .sorted_by_cached_key(|s| Versioning::new(s))
            .collect();
        Ok(versions)
    }

    fn bun_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/bun")
    }

    fn test_bun(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("bun -v");
        CmdLineRunner::new(&config.settings, self.bun_bin(tv))
            .with_pr(pr)
            .arg("-v")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &ProgressReport) -> Result<PathBuf> {
        let http = http::Client::new()?;
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/bun-{}-{}.zip",
            tv.version,
            os(),
            arch()
        );
        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("downloading {}", &url));
        http.download_file(&url, &tarball_path)?;

        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &ProgressReport, tarball_path: &Path) -> Result<()> {
        pr.set_message(format!("installing {}", tarball_path.display()));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path())?;
        file::rename(
            tv.download_path()
                .join(format!("bun-{}-{}", os(), arch()))
                .join("bun"),
            self.bun_bin(tv),
        )?;
        file::make_executable(&self.bun_bin(tv))?;
        Ok(())
    }

    fn verify(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        self.test_bun(config, tv, pr)
    }
}

impl Plugin for BunPlugin {
    fn name(&self) -> &PluginName {
        &self.core.name
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into()])
    }

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        assert!(matches!(&tv.request, ToolVersionRequest::Version { .. }));

        let tarball_path = self.download(tv, pr)?;
        self.install(tv, pr, &tarball_path)?;
        self.verify(config, tv, pr)?;

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
        "x64"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "aarch64"
    } else {
        &ARCH
    }
}
