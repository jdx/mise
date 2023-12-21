use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::Command;
use eyre::Result;

use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::duration::DAILY;
use crate::env::{RTX_NODE_COMPILE, RTX_NODE_CONCURRENCY, RTX_NODE_MAKE_OPTS, RTX_NODE_MIRROR_URL};
use crate::file::create_dir_all;
use crate::git::Git;
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::plugins::core::CorePlugin;
use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::SingleReport;
use crate::{cmd, env, file};

#[derive(Debug)]
pub struct NodeBuildPlugin {
    core: CorePlugin,
}

impl NodeBuildPlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("node"),
        }
    }

    fn node_build_path(&self) -> PathBuf {
        self.core.cache_path.join("node-build")
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

    fn lock_node_build(&self) -> Result<fslock::LockFile> {
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
        git.clone(&env::RTX_NODE_BUILD_REPO)?;
        Ok(())
    }
    fn update_node_build(&self) -> Result<()> {
        if self.node_build_recently_updated()? {
            return Ok(());
        }
        debug!(
            "Updating node-build in {}",
            self.node_build_path().display()
        );
        let git = Git::new(self.node_build_path());
        let node_build_path = self.node_build_path();
        CorePlugin::run_fetch_task_with_timeout(move || {
            git.update(None)?;
            file::touch_dir(&node_build_path)?;
            Ok(())
        })
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_rtx() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => rtxwarn!("failed to fetch remote versions: {}", e),
        }
        self.install_or_update_node_build()?;
        let node_build_bin = self.node_build_bin();
        CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(node_build_bin, "--definitions").read()?;
            let versions = output
                .split('\n')
                .filter(|s| regex!(r"^[0-9].+$").is_match(s))
                .map(|s| s.to_string())
                .collect();
            Ok(versions)
        })
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
        pr: &dyn SingleReport,
    ) -> Result<()> {
        let body = file::read_to_string(&*env::RTX_NODE_DEFAULT_PACKAGES_FILE).unwrap_or_default();
        for package in body.lines() {
            let package = package.split('#').next().unwrap_or_default().trim();
            if package.is_empty() {
                continue;
            }
            pr.set_message(format!("installing default package: {}", package));
            let npm = self.npm_path(tv);
            CmdLineRunner::new(npm)
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

    fn test_node(&self, config: &Config, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("node -v".into());
        CmdLineRunner::new(self.node_path(tv))
            .with_pr(pr)
            .arg("-v")
            .envs(&config.env)
            .execute()
    }

    fn test_npm(&self, config: &Config, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("npm -v".into());
        CmdLineRunner::new(self.npm_path(tv))
            .env("PATH", CorePlugin::path_env_with_tv_path(tv)?)
            .with_pr(pr)
            .arg("-v")
            .envs(&config.env)
            .execute()
    }

    fn node_build_recently_updated(&self) -> Result<bool> {
        let updated_at = file::modified_duration(&self.node_build_path())?;
        Ok(updated_at < DAILY)
    }

    fn verbose_install(&self) -> bool {
        let settings = Settings::get();
        let verbose_env = *env::RTX_NODE_VERBOSE_INSTALL;
        verbose_env == Some(true) || (settings.verbose && verbose_env != Some(false))
    }
}

impl Plugin for NodeBuildPlugin {
    fn name(&self) -> &str {
        "node"
    }

    fn list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
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

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".node-version".into(), ".nvmrc".into()])
    }

    fn parse_legacy_file(&self, path: &Path) -> Result<String> {
        let body = file::read_to_string(path)?;
        // trim "v" prefix
        let body = body.trim().strip_prefix('v').unwrap_or(&body);
        // replace lts/* with lts
        let body = body.replace("lts/*", "lts");
        Ok(body)
    }

    fn external_commands(&self) -> Result<Vec<Command>> {
        // sort of a hack to get this not to display for nodejs
        let topic = Command::new("node")
            .about("Commands for the node plugin")
            .subcommands(vec![Command::new("node-build")
                .about("Use/manage rtx's internal node-build")
                .arg(
                    clap::Arg::new("args")
                        .num_args(1..)
                        .allow_hyphen_values(true)
                        .trailing_var_arg(true),
                )]);
        Ok(vec![topic])
    }

    fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
        match command {
            "node-build" => {
                self.install_or_update_node_build()?;
                cmd::cmd(self.node_build_bin(), args).run()?;
            }
            _ => unreachable!(),
        }
        exit(0);
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let config = Config::get();
        self.install_node_build()?;
        ctx.pr.set_message("running node-build".into());
        let mut cmd = CmdLineRunner::new(self.node_build_bin())
            .with_pr(ctx.pr.as_ref())
            .env("NODE_BUILD_MIRROR_URL", RTX_NODE_MIRROR_URL.to_string())
            .envs(&config.env)
            .arg(ctx.tv.version.as_str());
        if matches!(&ctx.tv.request, ToolVersionRequest::Ref { .. }) || *RTX_NODE_COMPILE {
            let mut make_opts = RTX_NODE_MAKE_OPTS.clone().unwrap_or_default();
            if let Some(concurrency) = *RTX_NODE_CONCURRENCY {
                make_opts = format!("{} -j{}", make_opts, concurrency);
            }
            if let Some(node_make_opts) = &*RTX_NODE_MAKE_OPTS {
                make_opts = format!("{} {}", make_opts, node_make_opts);
            }
            cmd = cmd.env("NODE_MAKE_OPTS", make_opts);
            if let Some(cflags) = &*env::RTX_NODE_CFLAGS {
                cmd = cmd.env("NODE_CFLAGS", cflags);
            }
            if let Some(node_configure_opts) = &*env::RTX_NODE_CONFIGURE_OPTS {
                cmd = cmd.env("NODE_CONFIGURE_OPTS", node_configure_opts);
            }
            if let Some(make_install_opts) = &*env::RTX_NODE_MAKE_INSTALL_OPTS {
                cmd = cmd.env("NODE_MAKE_INSTALL_OPTS", make_install_opts);
            }
            cmd = cmd.arg("--compile");
        }
        if self.verbose_install() {
            cmd = cmd.arg("--verbose");
        }
        cmd.arg(&ctx.tv.install_path()).execute()?;
        self.test_node(&config, &ctx.tv, ctx.pr.as_ref())?;
        self.install_npm_shim(&ctx.tv)?;
        self.test_npm(&config, &ctx.tv, ctx.pr.as_ref())?;
        self.install_default_packages(&config, &ctx.tv, ctx.pr.as_ref())?;
        Ok(())
    }
}
