use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};

use crate::Result;
use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::version::OS;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{TarFormat, TarOptions};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, env, file, plugins};
use async_trait::async_trait;
use itertools::Itertools;
use tempfile::tempdir_in;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct GoPlugin {
    ba: Arc<BackendArg>,
}

impl GoPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("go")),
        }
    }

    // Represents go binary path
    fn go_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join("go")
    }

    // Represents GOPATH environment variable
    fn gopath(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("packages")
    }

    // Represents GOROOT environment variable
    fn goroot(&self, tv: &ToolVersion) -> PathBuf {
        let old_path = tv.install_path().join("go");
        if old_path.exists() {
            return old_path;
        }
        tv.install_path()
    }

    // Represents GOBIN environment variable
    fn gobin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin")
    }

    fn install_default_packages(
        &self,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> eyre::Result<()> {
        let settings = Settings::get();
        let default_packages_file = file::replace_path(&settings.go_default_packages_file);
        let body = file::read_to_string(default_packages_file).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("install default package: {package}"));
            let package = if package.contains('@') {
                package.to_string()
            } else {
                format!("{package}@latest")
            };
            CmdLineRunner::new(self.go_bin(tv))
                .with_pr(pr)
                .arg("install")
                .arg(package)
                .envs(self._exec_env(tv)?)
                .execute()?;
        }
        Ok(())
    }

    fn test_go(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> eyre::Result<()> {
        pr.set_message("go version".into());
        CmdLineRunner::new(self.go_bin(tv))
            // run the command in the install path to prevent issues with go.mod version mismatch
            .current_dir(tv.install_path())
            .with_pr(pr)
            .arg("version")
            .execute()
    }

    async fn download(
        &self,
        tv: &mut ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> eyre::Result<PathBuf> {
        let settings = Settings::get();
        let filename = format!(
            "go{}.{}-{}.{}",
            tv.version,
            platform(),
            arch(&settings),
            ext()
        );
        let tarball_url = Arc::new(format!("{}/{}", &settings.go_download_mirror, &filename));
        let tarball_path = tv.download_path().join(&filename);

        let tarball_url_ = tarball_url.clone();
        let checksum_handle = tokio::spawn(async move {
            let checksum_url = format!("{}.sha256", &tarball_url_);
            HTTP.get_text(checksum_url).await
        });
        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&*tarball_url, &tarball_path, Some(pr))
            .await?;

        if !settings.go_skip_checksum && !tv.checksums.contains_key(&filename) {
            let checksum = checksum_handle.await.unwrap()?;
            tv.checksums.insert(filename, format!("sha256:{checksum}"));
        }
        Ok(tarball_path)
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
        tarball_path: &Path,
    ) -> eyre::Result<()> {
        let tarball = tarball_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        pr.set_message(format!("extract {tarball}"));
        let tmp_extract_path = tempdir_in(tv.install_path().parent().unwrap())?;
        if cfg!(windows) {
            file::unzip(tarball_path, tmp_extract_path.path())?;
        } else {
            file::untar(
                tarball_path,
                tmp_extract_path.path(),
                &TarOptions {
                    format: TarFormat::TarGz,
                    pr: Some(pr),
                    ..Default::default()
                },
            )?;
        }
        file::remove_all(tv.install_path())?;
        file::rename(tmp_extract_path.path().join("go"), tv.install_path())?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &Box<dyn SingleReport>) -> eyre::Result<()> {
        self.test_go(tv, pr)?;
        if let Err(err) = self.install_default_packages(tv, pr) {
            warn!("failed to install default go packages: {err:#}");
        }
        let settings = Settings::get();
        if settings.go_set_gopath {
            warn!("setting go_set_gopath is deprecated");
        }
        Ok(())
    }

    fn _exec_env(&self, tv: &ToolVersion) -> eyre::Result<BTreeMap<String, String>> {
        let mut map = BTreeMap::new();
        let mut set = |k: &str, v: PathBuf| {
            map.insert(k.to_string(), v.to_string_lossy().to_string());
        };
        let settings = Settings::get();
        let gobin = settings.go_set_gobin;
        let gobin_env_is_set = env::PRISTINE_ENV.contains_key("GOBIN");
        if gobin == Some(true) || (gobin.is_none() && !gobin_env_is_set) {
            set("GOBIN", self.gobin(tv));
        }
        if settings.go_set_goroot {
            set("GOROOT", self.goroot(tv));
        }
        if settings.go_set_gopath {
            set("GOPATH", self.gopath(tv));
        }
        Ok(map)
    }
}

#[async_trait]
impl Backend for GoPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }
    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        plugins::core::run_fetch_task_with_timeout(move || {
            let output = cmd!(
                "git",
                "ls-remote",
                "--tags",
                &Settings::get().go_repo,
                "go*"
            )
            .read()?;
            let lines = output.split('\n');
            let versions = lines.map(|s| s.split("/go").last().unwrap_or_default().to_string())
                .filter(|s| !s.is_empty())
                .filter(|s| !regex!(r"^1($|\.0|\.0\.[0-9]|\.1|\.1rc[0-9]|\.1\.[0-9]|.2|\.2rc[0-9]|\.2\.1|.8.5rc5)$").is_match(s))
                .unique()
                .sorted_by_cached_key(|s| (Versioning::new(s), s.to_string()))
                .collect();
            Ok(versions)
        })
    }
    fn idiomatic_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![".go-version".into()])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&mut tv, &ctx.pr).await?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        self.install(&tv, &ctx.pr, &tarball_path)?;
        self.verify(&tv, &ctx.pr)?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &Box<dyn SingleReport>,
        tv: &ToolVersion,
    ) -> eyre::Result<()> {
        let gopath = self.gopath(tv);
        if gopath.exists() {
            cmd!("chmod", "-R", "u+wx", gopath).run()?;
        }
        Ok(())
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<PathBuf>> {
        if let ToolRequest::System { .. } = tv.request {
            return Ok(vec![]);
        }
        // goroot/bin must always be included, irrespective of MISE_GO_SET_GOROOT
        Ok(vec![self.gobin(tv)])
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        self._exec_env(tv)
    }
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        &OS
    }
}

fn arch(settings: &Settings) -> &str {
    let arch = settings.arch();
    if arch == "x86_64" {
        "amd64"
    } else if arch == "arm" {
        "armv6l"
    } else if arch == "aarch64" {
        "arm64"
    } else {
        arch
    }
}

fn ext() -> &'static str {
    if cfg!(windows) { "zip" } else { "tar.gz" }
}
