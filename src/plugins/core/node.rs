use std::collections::BTreeMap;

use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use serde_derive::Deserialize;
use tempfile::tempdir_in;
use url::Url;

use crate::build_time::built_info;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::{
    RTX_FETCH_REMOTE_VERSIONS_TIMEOUT, RTX_NODE_CFLAGS, RTX_NODE_CONCURRENCY,
    RTX_NODE_CONFIGURE_OPTS, RTX_NODE_FORCE_COMPILE, RTX_NODE_MAKE, RTX_NODE_MAKE_INSTALL_OPTS,
    RTX_NODE_MAKE_OPTS, RTX_NODE_MIRROR_URL, RTX_NODE_NINJA, RTX_NODE_VERIFY, RTX_TMP_DIR,
};
use crate::plugins::core::CorePlugin;
use crate::plugins::Plugin;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::ProgressReport;
use crate::{env, file, hash, http};

#[derive(Debug)]
pub struct NodePlugin {
    core: CorePlugin,
    http: http::Client,
}

impl NodePlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("node"),
            http: http::Client::new_with_timeout(*RTX_FETCH_REMOTE_VERSIONS_TIMEOUT).unwrap(),
        }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let versions = self
            .http
            .json::<Vec<NodeVersion>, _>(RTX_NODE_MIRROR_URL.join("index.json")?)?
            .into_iter()
            .map(|v| {
                if regex!(r"^v\d+\.").is_match(&v.version) {
                    v.version.strip_prefix('v').unwrap().to_string()
                } else {
                    v.version
                }
            })
            .rev()
            .collect();
        Ok(versions)
    }

    fn install_precompiled(&self, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        let slug = slug(&tv.version, os(), arch());
        let tarball_name = binary_tarball_name(&tv.version);
        let tmp_tarball = tv.download_path().join(&tarball_name);
        let url = self.binary_tarball_url(&tv.version)?;
        self.fetch_tarball(pr, url, &tmp_tarball, &tv.version)?;
        pr.set_message(format!("extracting {tarball_name}"));
        let tmp_extract_path = tempdir_in(tv.install_path().parent().unwrap())?;
        file::untar(&tmp_tarball, tmp_extract_path.path())?;
        file::remove_all(tv.install_path())?;
        file::rename(tmp_extract_path.path().join(slug), tv.install_path())?;
        Ok(())
    }

    fn install_compiled(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        let tarball_name = source_tarball_name(&tv.version);
        let tmp_tarball = tv.download_path().join(&tarball_name);
        let url = self.source_tarball_url(&tv.version)?;
        self.fetch_tarball(pr, url, &tmp_tarball, &tv.version)?;
        pr.set_message(format!("extracting {tarball_name}"));
        let build_dir = RTX_TMP_DIR.join(format!("node-v{}", tv.version));
        file::remove_all(&build_dir)?;
        file::untar(&tmp_tarball, &RTX_TMP_DIR)?;
        self.exec_configure(config, pr, &build_dir, &tv.install_path())?;
        self.exec_make(config, pr, &build_dir)?;
        self.exec_make_install(config, pr, &build_dir)?;
        Ok(())
    }

    fn fetch_tarball(
        &self,
        pr: &ProgressReport,
        url: Url,
        local: &Path,
        version: &str,
    ) -> Result<()> {
        let tarball_name = local.file_name().unwrap().to_string_lossy().to_string();
        if local.exists() {
            pr.set_message(format!("using previously downloaded {tarball_name}"));
        } else {
            pr.set_message(format!("downloading {tarball_name}"));
            self.http.download_file(url, local)?;
        }
        if *RTX_NODE_VERIFY {
            pr.set_message(format!("verifying {tarball_name}"));
            self.verify(local, version)?;
        }
        Ok(())
    }

    fn sh<'a>(
        &'a self,
        settings: &'a Settings,
        pr: &'a ProgressReport,
        dir: &Path,
    ) -> CmdLineRunner {
        let mut cmd = CmdLineRunner::new(settings, "sh")
            .with_pr(pr)
            .current_dir(dir)
            .arg("-c");
        if let Some(cflags) = &*RTX_NODE_CFLAGS {
            cmd = cmd.env("CFLAGS", cflags);
        }
        cmd
    }

    fn exec_configure(
        &self,
        config: &Config,
        pr: &ProgressReport,
        build_dir: &Path,
        out: &Path,
    ) -> Result<()> {
        self.sh(&config.settings, pr, build_dir)
            .arg(format!(
                "./configure --prefix={} {} {}",
                out.display(),
                if *RTX_NODE_NINJA { "--ninja" } else { "" },
                RTX_NODE_CONFIGURE_OPTS.clone().unwrap_or_default()
            ))
            .execute()
    }
    fn exec_make(&self, config: &Config, pr: &ProgressReport, build_dir: &Path) -> Result<()> {
        self.sh(&config.settings, pr, build_dir)
            .arg(format!(
                "{} {} {}",
                &*RTX_NODE_MAKE,
                RTX_NODE_CONCURRENCY
                    .map(|c| format!("-j{}", c))
                    .unwrap_or_default(),
                RTX_NODE_MAKE_OPTS.clone().unwrap_or_default()
            ))
            .execute()
    }
    fn exec_make_install(
        &self,
        config: &Config,
        pr: &ProgressReport,
        build_dir: &Path,
    ) -> Result<()> {
        self.sh(&config.settings, pr, build_dir)
            .arg(format!(
                "{} install {}",
                &*RTX_NODE_MAKE,
                RTX_NODE_MAKE_INSTALL_OPTS.clone().unwrap_or_default()
            ))
            .execute()
    }

    fn verify(&self, tarball: &Path, version: &str) -> Result<()> {
        let tarball_name = tarball.file_name().unwrap().to_string_lossy().to_string();
        // TODO: verify gpg signature
        let shasums = self.http.get_text(self.shasums_url(version)?)?;
        let shasums = hash::parse_shasums(&shasums);
        let shasum = shasums.get(&tarball_name).unwrap();
        hash::ensure_checksum_sha256(tarball, shasum)
    }

    fn node_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/node")
    }

    fn npm_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/npm")
    }

    fn install_default_packages(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        let body = file::read_to_string(&*env::RTX_NODE_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default package: {}", package));
            let npm = self.npm_path(tv);
            CmdLineRunner::new(&config.settings, npm)
                .with_pr(pr)
                .arg("install")
                .arg("--global")
                .arg(package)
                .envs(&config.env)
                .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
                .execute()?;
        }
        Ok(())
    }

    fn install_npm_shim(&self, tv: &ToolVersion) -> Result<()> {
        file::remove_file(self.npm_path(tv)).ok();
        file::write(self.npm_path(tv), include_str!("assets/node_npm_shim"))?;
        file::make_executable(&self.npm_path(tv))?;
        Ok(())
    }

    fn test_node(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("node -v");
        CmdLineRunner::new(&config.settings, self.node_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(&config.env)
            .execute()
    }

    fn test_npm(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        pr.set_message("npm -v");
        CmdLineRunner::new(&config.settings, self.npm_path(tv))
            .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
            .with_pr(pr)
            .arg("-v")
            .envs(&config.env)
            .execute()
    }

    fn source_tarball_url(&self, v: &str) -> Result<Url> {
        let url = RTX_NODE_MIRROR_URL.join(&format!("v{v}/{}", source_tarball_name(v)))?;
        Ok(url)
    }
    fn binary_tarball_url(&self, v: &str) -> Result<Url> {
        let url = RTX_NODE_MIRROR_URL.join(&format!("v{v}/{}", binary_tarball_name(v)))?;
        Ok(url)
    }
    fn shasums_url(&self, v: &str) -> Result<Url> {
        // let url = RTX_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt.asc"))?;
        let url = RTX_NODE_MIRROR_URL.join(&format!("v{v}/SHASUMS256.txt"))?;
        Ok(url)
    }
}

impl Plugin for NodePlugin {
    fn name(&self) -> &str {
        "node"
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn get_aliases(&self, _settings: &Settings) -> Result<BTreeMap<String, String>> {
        let aliases = [
            ("lts/argon", "4"),
            ("lts/boron", "6"),
            ("lts/carbon", "8"),
            ("lts/dubnium", "10"),
            ("lts/erbium", "12"),
            ("lts/fermium", "14"),
            ("lts/gallium", "16"),
            ("lts/hydrogen", "18"),
            ("lts/iron", "20"),
            ("lts-argon", "4"),
            ("lts-boron", "6"),
            ("lts-carbon", "8"),
            ("lts-dubnium", "10"),
            ("lts-erbium", "12"),
            ("lts-fermium", "14"),
            ("lts-gallium", "16"),
            ("lts-hydrogen", "18"),
            ("lts-iron", "20"),
            ("lts", "20"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Ok(aliases)
    }

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![".node-version".into(), ".nvmrc".into()])
    }

    fn parse_legacy_file(&self, path: &Path, _settings: &Settings) -> Result<String> {
        let body = file::read_to_string(path)?;
        // trim "v" prefix
        let body = body.trim().strip_prefix('v').unwrap_or(&body);
        // replace lts/* with lts
        let body = body.replace("lts/*", "lts");
        Ok(body.to_string())
    }

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        pr.set_message("running node-build");
        if *RTX_NODE_FORCE_COMPILE {
            self.install_compiled(config, tv, pr)?;
        } else {
            match self.install_precompiled(tv, pr) {
                Err(e) if matches!(http::error_code(&e), Some(404)) => {
                    debug!("precompiled node not found");
                    self.install_compiled(config, tv, pr)?;
                }
                Err(e) => return Err(e),
                Ok(()) => {}
            }
        }
        self.test_node(config, tv, pr)?;
        self.install_npm_shim(tv)?;
        self.test_npm(config, tv, pr)?;
        self.install_default_packages(config, tv, pr)?;
        Ok(())
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        built_info::CFG_OS
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "arm") {
        "armv7l"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        built_info::CFG_TARGET_ARCH
    }
}

fn slug(v: &str, os: &str, arch: &str) -> String {
    format!("node-v{v}-{os}-{arch}")
}
fn source_tarball_name(v: &str) -> String {
    format!("node-v{v}.tar.gz")
}
fn binary_tarball_name(v: &str) -> String {
    format!("{}.tar.gz", slug(v, os(), arch()))
}

#[derive(Debug, Deserialize)]
struct NodeVersion {
    version: String,
}
