use std::collections::BTreeMap;
use std::env::{join_paths, split_paths};
use std::fs;
use std::path::PathBuf;
use std::process::exit;
use std::time::Duration;

use color_eyre::eyre::Result;

use crate::cache::CacheManager;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::{RTX_EXE, RTX_NODE_CONCURRENCY, RTX_NODE_FORCE_COMPILE, RTX_NODE_VERBOSE_INSTALL};
use crate::file::create_dir_all;
use crate::git::Git;
use crate::lock_file::LockFile;
use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::ProgressReport;
use crate::{cmd, dirs, env};

#[derive(Debug)]
pub struct NodePlugin {
    pub name: PluginName,
    cache_path: PathBuf,
    remote_version_cache: CacheManager<Vec<String>>,
    legacy_file_support: bool,
}

impl NodePlugin {
    pub fn new(name: PluginName) -> Self {
        let cache_path = dirs::CACHE.join(&name);
        let fresh_duration = Some(Duration::from_secs(60 * 60 * 12)); // 12 hours
        Self {
            remote_version_cache: CacheManager::new(cache_path.join("remote_versions.msgpack.z"))
                .with_fresh_duration(fresh_duration)
                .with_fresh_file(RTX_EXE.clone()),
            name,
            cache_path,
            legacy_file_support: false,
        }
    }

    pub fn with_legacy_file_support(self) -> Self {
        Self {
            legacy_file_support: true,
            ..self
        }
    }

    fn node_build_path(&self) -> PathBuf {
        self.cache_path.join("node-build")
    }
    fn node_build_bin(&self) -> PathBuf {
        self.node_build_path().join("bin/node-build")
    }
    fn install_or_update_node_build(&self) -> Result<()> {
        let _lock = self.lock_node_build();
        if self.node_build_path().exists() {
            self.update_node_build()
        } else {
            self.install_node_build()
        }
    }

    fn lock_node_build(&self) -> Result<fslock::LockFile, std::io::Error> {
        LockFile::new(&self.node_build_path())
            .with_callback(|l| {
                trace!("install_or_update_node_build {}", l.display());
            })
            .lock()
    }
    fn install_node_build(&self) -> Result<()> {
        if self.node_build_path().exists() {
            return Ok(());
        }
        debug!(
            "Installing node-build to {}",
            self.node_build_path().display()
        );
        create_dir_all(self.node_build_path().parent().unwrap())?;
        let git = Git::new(self.node_build_path());
        git.clone("https://github.com/nodenv/node-build.git")?;
        Ok(())
    }
    fn update_node_build(&self) -> Result<()> {
        // TODO: do not update if recently updated
        debug!(
            "Updating node-build in {}",
            self.node_build_path().display()
        );
        let git = Git::new(self.node_build_path());
        git.update(None)?;
        Ok(())
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        self.install_or_update_node_build()?;
        let output = cmd!(self.node_build_bin(), "--definitions").read()?;
        let versions = output
            .split('\n')
            .filter(|s| regex!(r"^[0-9].+$").is_match(s))
            .map(|s| s.to_string())
            .collect();
        Ok(versions)
    }

    fn node_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/node")
    }

    fn npm_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin/npm")
    }

    fn install_default_packages(
        &self,
        settings: &Settings,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        let body = fs::read_to_string(&*env::RTX_NODE_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default package: {}", package));
            let npm = self.npm_path(tv);
            let mut cmd = CmdLineRunner::new(settings, npm);
            cmd.with_pr(pr).arg("install").arg("--global").arg(package);
            let mut path = split_paths(&env::var_os("PATH").unwrap()).collect::<Vec<_>>();
            path.insert(0, tv.install_path().join("bin"));
            cmd.env("PATH", join_paths(path)?);
            cmd.execute()?;
        }
        Ok(())
    }

    fn install_npm_shim(&self, tv: &ToolVersion) -> Result<()> {
        fs::write(self.npm_path(tv), include_str!("assets/node_npm_shim"))?;
        Ok(())
    }

    fn test_node(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        let mut cmd = CmdLineRunner::new(&config.settings, self.node_path(tv));
        cmd.with_pr(pr).arg("-v");
        cmd.execute()
    }

    fn test_npm(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport) -> Result<()> {
        let mut cmd = CmdLineRunner::new(&config.settings, self.npm_path(tv));
        cmd.with_pr(pr).arg("-v");
        cmd.execute()
    }
}

impl Plugin for NodePlugin {
    fn name(&self) -> &PluginName {
        &self.name
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.remote_version_cache
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
            ("lts", "18"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Ok(aliases)
    }

    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        if self.legacy_file_support {
            Ok(vec![".node-version".into(), ".nvmrc".into()])
        } else {
            Ok(vec![])
        }
    }

    fn external_commands(&self) -> Result<Vec<Vec<String>>> {
        Ok(vec![vec![self.name.clone(), "nodebuild".into()]])
    }

    fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
        match command {
            "nodebuild" => {
                self.install_or_update_node_build()?;
                cmd::cmd(self.node_build_bin(), args).run()?;
            }
            _ => unreachable!(),
        }
        exit(0);
    }

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
    ) -> Result<()> {
        self.install_node_build()?;
        pr.set_message("running node-build");
        let mut cmd = CmdLineRunner::new(&config.settings, self.node_build_bin());
        cmd.with_pr(pr).arg(tv.version.as_str());
        if matches!(&tv.request, ToolVersionRequest::Ref { .. }) || *RTX_NODE_FORCE_COMPILE {
            let make_opts = String::from(" -j") + &RTX_NODE_CONCURRENCY.to_string();
            cmd.env(
                "MAKE_OPTS",
                env::var("MAKE_OPTS").unwrap_or_default() + &make_opts,
            );
            cmd.env(
                "NODE_MAKE_OPTS",
                env::var("NODE_MAKE_OPTS").unwrap_or_default() + &make_opts,
            );
            cmd.arg("--compile");
        }
        if *RTX_NODE_VERBOSE_INSTALL {
            cmd.arg("--verbose");
        }
        cmd.arg(tv.install_path());
        cmd.execute()?;
        self.test_node(config, tv, pr)?;
        self.install_npm_shim(tv)?;
        self.test_npm(config, tv, pr)?;
        self.install_default_packages(&config.settings, tv, pr)?;
        Ok(())
    }
}
