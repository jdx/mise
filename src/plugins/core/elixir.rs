use std::path::{Path, PathBuf};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::plugins::VERSION_REGEX;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{file, plugins};
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct ElixirPlugin {
    ba: BackendArg,
}

impl ElixirPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("elixir"),
        }
    }

    fn elixir_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join("elixir")
    }

    fn test_elixir(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("elixir --version".into());
        CmdLineRunner::new(self.elixir_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .envs(self.dependency_env()?)
            .arg("--version")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let version = &tv.version;
        let url = format!("https://builds.hex.pm/builds/elixir/v{version}.zip");

        let filename = url.split('/').last().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        if !tarball_path.exists() {
            HTTP.download_file(&url, &tarball_path, Some(pr))?;
        }

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::unzip(tarball_path, &tv.install_path())?;

        Ok(())
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_elixir(ctx, tv)
    }
}

impl Backend for ElixirPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        let versions: Vec<String> = HTTP_FETCH
            .get_text("https://builds.hex.pm/builds/elixir/builds.txt")?
            .lines()
            .unique()
            .filter_map(|s| s.split_once(' ').map(|(v, _)| v.trim_start_matches('v')))
            .filter(|s| regex!(r"^[0-9]+\.[0-9]+\.[0-9]").is_match(s))
            .sorted_by_cached_key(|s| {
                (
                    Versioning::new(s.split_once('-').map(|(v, _)| v).unwrap_or(s)),
                    !VERSION_REGEX.is_match(s),
                    s.contains("-otp-"),
                    Versioning::new(s),
                    s.to_string(),
                )
            })
            .map(|s| s.to_string())
            .collect();
        Ok(versions)
    }

    fn get_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec!["erlang"])
    }

    fn install_version_(&self, ctx: &InstallContext, mut tv: ToolVersion) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref())?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(ctx, &tv, &tarball_path)?;
        self.verify(ctx, &tv)?;
        Ok(tv)
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        Ok(["bin", ".mix/escripts"]
            .iter()
            .map(|p| tv.install_path().join(p))
            .collect())
    }
}
