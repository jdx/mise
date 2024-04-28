use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread;

use itertools::Itertools;
use tempfile::tempdir_in;
use versions::Versioning;

use crate::cli::args::ForgeArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::forge::Forge;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::plugins::core::CorePlugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, env, file, hash};

#[derive(Debug)]
pub struct GoPlugin {
    core: CorePlugin,
}

impl GoPlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("go"),
        }
    }

    fn fetch_remote_versions(&self) -> eyre::Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        let settings = Settings::get();
        CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!("git", "ls-remote", "--tags", &settings.go_repo, "go*").read()?;
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

    // Represents go binary path
    fn go_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/go")
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
        pr: &dyn SingleReport,
    ) -> eyre::Result<()> {
        let settings = Settings::get();
        let default_packages_file = file::replace_path(&settings.go_default_packages_file);
        let body = file::read_to_string(default_packages_file).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default package: {}", package));
            let package = if package.contains('@') {
                package.to_string()
            } else {
                format!("{}@latest", package)
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

    fn test_go(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> eyre::Result<()> {
        pr.set_message("go version".into());
        CmdLineRunner::new(self.go_bin(tv))
            .with_pr(pr)
            .arg("version")
            .execute()
    }

    fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> eyre::Result<PathBuf> {
        let settings = Settings::get();
        let filename = format!("go{}.{}-{}.tar.gz", tv.version, platform(), arch());
        let tarball_url = format!("{}/{}", &settings.go_download_mirror, &filename);
        let tarball_path = tv.download_path().join(&filename);

        thread::scope(|s| {
            let checksum_handle = s.spawn(|| {
                let checksum_url = format!("{}.sha256", &tarball_url);
                HTTP.get_text(checksum_url)
            });
            pr.set_message(format!("downloading {filename}"));
            HTTP.download_file(&tarball_url, &tarball_path, Some(pr))?;

            if !settings.go_skip_checksum {
                pr.set_message(format!("verifying {filename}"));
                let checksum = checksum_handle.join().unwrap()?;
                hash::ensure_checksum_sha256(&tarball_path, &checksum, Some(pr))?;
            }
            Ok(tarball_path)
        })
    }

    fn install(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        tarball_path: &Path,
    ) -> eyre::Result<()> {
        let tarball = tarball_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        pr.set_message(format!("installing {}", tarball));
        let tmp_extract_path = tempdir_in(tv.install_path().parent().unwrap())?;
        file::untar(tarball_path, tmp_extract_path.path())?;
        file::remove_all(tv.install_path())?;
        file::rename(tmp_extract_path.path().join("go"), tv.install_path())?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> eyre::Result<()> {
        self.test_go(tv, pr)?;
        self.install_default_packages(tv, pr)?;
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

impl Forge for GoPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }
    fn legacy_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![".go-version".into()])
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let tarball_path = self.download(&ctx.tv, ctx.pr.as_ref())?;
        self.install(&ctx.tv, ctx.pr.as_ref(), &tarball_path)?;
        self.verify(&ctx.tv, ctx.pr.as_ref())?;

        Ok(())
    }

    fn uninstall_version_impl(&self, _pr: &dyn SingleReport, tv: &ToolVersion) -> eyre::Result<()> {
        let gopath = self.gopath(tv);
        if gopath.exists() {
            cmd!("chmod", "-R", "u+wx", gopath).run()?;
        }
        Ok(())
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> eyre::Result<Vec<PathBuf>> {
        if let ToolVersionRequest::System(_) = tv.request {
            return Ok(vec![]);
        }
        // goroot/bin must always be included, irrespective of MISE_GO_SET_GOROOT
        let mut paths = vec![
            self.gobin(tv),
            // TODO: this can be removed at some point since go is not installed here anymore
            tv.install_path().join("go/bin"),
        ];
        let settings = Settings::get();
        if settings.go_set_gopath {
            // TODO: this can be removed at some point since things are installed to GOBIN instead
            paths.push(self.gopath(tv).join("bin"));
        }
        Ok(paths)
    }

    fn exec_env(
        &self,
        _config: &Config,
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

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "amd64") {
        "amd64"
    } else if cfg!(target_arch = "i686") || cfg!(target_arch = "i386") || cfg!(target_arch = "386")
    {
        "386"
    } else if cfg!(target_arch = "armv6l") || cfg!(target_arch = "armv7l") {
        "armv6l"
    } else if cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm64") {
        "arm64"
    } else {
        &ARCH
    }
}
