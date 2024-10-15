use crate::config::{Config, Settings, SETTINGS};
use crate::errors::Error::PluginNotInstalled;
use crate::file::{display_path, remove_all};
use crate::git::Git;
use crate::plugins::{Plugin, PluginList, PluginType, Script, ScriptManager};
use crate::result::Result;
use crate::timeout::run_with_timeout;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::prompt;
use crate::{dirs, exit, lock_file, registry};
use clap::Command;
use console::style;
use contracts::requires;
use eyre::{bail, eyre, Context};
use itertools::Itertools;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use xx::{file, regex};

#[derive(Debug)]
pub struct AsdfPlugin {
    pub name: String,
    pub plugin_path: PathBuf,
    pub repo: Mutex<Git>,
    pub repo_url: Option<String>,
    pub script_man: ScriptManager,
}

pub static ASDF_PLUGIN_NAMES: Lazy<BTreeSet<String>> = Lazy::new(|| match AsdfPlugin::list() {
    Ok(plugins) => plugins.into_iter().map(|p| p.name().to_string()).collect(),
    Err(err) => {
        warn!("Failed to list vfox plugins: {err}");
        BTreeSet::new()
    }
});

impl AsdfPlugin {
    #[requires(!name.is_empty())]
    pub fn new(name: String) -> Self {
        let plugin_path = dirs::PLUGINS.join(&name);
        let repo = Git::new(&plugin_path);
        Self {
            script_man: build_script_man(&name, &plugin_path),
            name,
            repo_url: None,
            repo: Mutex::new(repo),
            plugin_path,
        }
    }

    pub fn list() -> eyre::Result<PluginList> {
        let settings = Settings::get();
        match file::ls(*dirs::PLUGINS) {
            Ok(dirs) => {
                let plugins = dirs
                    .into_par_iter()
                    .filter(|dir| dir.is_dir())
                    .map(|dir| {
                        let name = dir.file_name().unwrap().to_string_lossy().to_string();
                        Box::new(AsdfPlugin::new(name)) as Box<dyn Plugin>
                    })
                    .filter(|p| !settings.disable_tools.contains(p.name()))
                    .collect();
                Ok(plugins)
            }
            Err(_) => Ok(PluginList::new()),
        }
    }

    fn repo(&self) -> MutexGuard<Git> {
        self.repo.lock().unwrap()
    }

    fn get_repo_url(&self, config: &Config) -> eyre::Result<String> {
        self.repo_url
            .clone()
            .or_else(|| self.repo().get_remote_url())
            .or_else(|| config.get_repo_url(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))
    }

    fn exec_hook_post_plugin_update(
        &self,
        pr: &dyn SingleReport,
        pre: String,
        post: String,
    ) -> eyre::Result<()> {
        if pre != post {
            let env = [
                ("ASDF_PLUGIN_PREV_REF", pre.clone()),
                ("ASDF_PLUGIN_POST_REF", post.clone()),
                ("MISE_PLUGIN_PREV_REF", pre),
                ("MISE_PLUGIN_POST_REF", post),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
            self.exec_hook_env(pr, "post-plugin-update", env)?;
        }
        Ok(())
    }

    fn exec_hook(&self, pr: &dyn SingleReport, hook: &str) -> eyre::Result<()> {
        self.exec_hook_env(pr, hook, Default::default())
    }
    fn exec_hook_env(
        &self,
        pr: &dyn SingleReport,
        hook: &str,
        env: HashMap<OsString, OsString>,
    ) -> eyre::Result<()> {
        let script = Script::Hook(hook.to_string());
        let mut sm = self.script_man.clone();
        sm.env.extend(env);
        if sm.script_exists(&script) {
            pr.set_message(format!("executing {hook} hook"));
            sm.run_by_line(&script, pr)?;
        }
        Ok(())
    }
    pub fn fetch_remote_versions(&self) -> eyre::Result<Vec<String>> {
        let cmd = self.script_man.cmd(&Script::ListAll);
        let result = run_with_timeout(
            move || {
                let result = cmd.stdout_capture().stderr_capture().unchecked().run()?;
                Ok(result)
            },
            SETTINGS.fetch_remote_versions_timeout(),
        )
        .wrap_err_with(|| {
            let script = self.script_man.get_script_path(&Script::ListAll);
            eyre!("Failed to run {}", display_path(script))
        })?;
        let stdout = String::from_utf8(result.stdout).unwrap();
        let stderr = String::from_utf8(result.stderr).unwrap().trim().to_string();

        let display_stderr = || {
            if !stderr.is_empty() {
                eprintln!("{stderr}");
            }
        };
        if !result.status.success() {
            let s = Script::ListAll;
            match result.status.code() {
                Some(code) => bail!("error running {}: exited with code {}\n{}", s, code, stderr),
                None => bail!("error running {}: terminated by signal\n{}", s, stderr),
            };
        } else if Settings::get().verbose {
            display_stderr();
        }

        Ok(stdout
            .split_whitespace()
            .map(|v| regex!(r"^v(\d+)").replace(v, "$1").to_string())
            .collect())
    }
    pub fn fetch_latest_stable(&self) -> eyre::Result<Option<String>> {
        let latest_stable = self
            .script_man
            .read(&Script::LatestStable)?
            .trim()
            .to_string();
        Ok(if latest_stable.is_empty() {
            None
        } else {
            Some(latest_stable)
        })
    }

    pub fn fetch_legacy_filenames(&self) -> eyre::Result<Vec<String>> {
        let stdout = self.script_man.read(&Script::ListLegacyFilenames)?;
        Ok(self.parse_legacy_filenames(&stdout))
    }
    pub fn parse_legacy_filenames(&self, data: &str) -> Vec<String> {
        data.split_whitespace().map(|v| v.into()).collect()
    }
    pub fn has_list_alias_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListAliases)
    }
    pub fn has_list_legacy_filenames_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListLegacyFilenames)
    }
    pub fn has_latest_stable_script(&self) -> bool {
        self.script_man.script_exists(&Script::LatestStable)
    }
    pub fn fetch_aliases(&self) -> eyre::Result<Vec<(String, String)>> {
        let stdout = self.script_man.read(&Script::ListAliases)?;
        Ok(self.parse_aliases(&stdout))
    }
    pub(crate) fn parse_aliases(&self, data: &str) -> Vec<(String, String)> {
        data.lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace().collect_vec();
                if parts.len() != 2 {
                    if !parts.is_empty() {
                        trace!("invalid alias line: {}", line);
                    }
                    return None;
                }
                Some((parts.remove(0).into(), parts.remove(0).into()))
            })
            .collect()
    }
}

impl Plugin for AsdfPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> PathBuf {
        self.plugin_path.clone()
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Asdf
    }

    fn get_remote_url(&self) -> eyre::Result<Option<String>> {
        let url = self.repo().get_remote_url();
        Ok(url.or(self.repo_url.clone()))
    }

    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo().current_abbrev_ref().map(Some)
    }

    fn current_sha_short(&self) -> eyre::Result<Option<String>> {
        if !self.is_installed() {
            return Ok(None);
        }
        self.repo().current_sha_short().map(Some)
    }

    fn is_installed(&self) -> bool {
        self.plugin_path.exists()
    }

    fn is_installed_err(&self) -> eyre::Result<()> {
        if self.is_installed() {
            return Ok(());
        }
        Err(eyre!("asdf plugin {} is not installed", self.name())
            .wrap_err("run with --yes to install plugin automatically"))
    }

    fn ensure_installed(&self, mpr: &MultiProgressReport, force: bool) -> Result<()> {
        let config = Config::get();
        let settings = Settings::try_get()?;
        if !force {
            if self.is_installed() {
                return Ok(());
            }
            if !settings.yes && self.repo_url.is_none() {
                let url = self.get_repo_url(&config).unwrap_or_default();
                if !registry::is_trusted_plugin(self.name(), &url) {
                    warn!(
                        "⚠️ {} is a community-developed plugin – {}",
                        style(&self.name).blue(),
                        style(url.trim_end_matches(".git")).yellow()
                    );
                    if settings.paranoid {
                        bail!("Paranoid mode is enabled, refusing to install community-developed plugin");
                    }
                    if !prompt::confirm_with_all(format!(
                        "Would you like to install {}?",
                        self.name
                    ))? {
                        Err(PluginNotInstalled(self.name.clone()))?
                    }
                }
            }
        }
        let prefix = format!("plugin:{}", style(&self.name).blue().for_stderr());
        let pr = mpr.add(&prefix);
        let _lock = lock_file::get(&self.plugin_path, force)?;
        self.install(pr.as_ref())
    }

    fn update(&self, pr: &dyn SingleReport, gitref: Option<String>) -> Result<()> {
        let plugin_path = self.plugin_path.to_path_buf();
        if plugin_path.is_symlink() {
            warn!(
                "plugin:{} is a symlink, not updating",
                style(&self.name).blue().for_stderr()
            );
            return Ok(());
        }
        let git = Git::new(plugin_path);
        if !git.is_repo() {
            warn!(
                "plugin:{} is not a git repository, not updating",
                style(&self.name).blue().for_stderr()
            );
            return Ok(());
        }
        pr.set_message("updating git repo".into());
        let (pre, post) = git.update(gitref)?;
        let sha = git.current_sha_short()?;
        let repo_url = self.get_remote_url()?.unwrap_or_default();
        self.exec_hook_post_plugin_update(pr, pre, post)?;
        pr.finish_with_message(format!(
            "{repo_url}#{}",
            style(&sha).bright().yellow().for_stderr(),
        ));
        Ok(())
    }

    fn uninstall(&self, pr: &dyn SingleReport) -> Result<()> {
        if !self.is_installed() {
            return Ok(());
        }
        self.exec_hook(pr, "pre-plugin-remove")?;
        pr.set_message("uninstalling".into());

        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("removing {}", display_path(dir)));
            remove_all(dir).wrap_err_with(|| {
                format!(
                    "Failed to remove directory {}",
                    style(display_path(dir)).cyan().for_stderr()
                )
            })
        };

        rmdir(&self.plugin_path)?;

        Ok(())
    }

    fn install(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        let config = Config::get();
        let repository = self.get_repo_url(&config)?;
        let (repo_url, repo_ref) = Git::split_url_and_ref(&repository);
        debug!("asdf_plugin[{}]:install {:?}", self.name, repository);

        if self.is_installed() {
            self.uninstall(pr)?;
        }

        if regex!(r"^[/~]").is_match(&repo_url) {
            Err(eyre!(
                r#"Invalid repository URL: {repo_url}
If you are trying to link to a local directory, use `mise plugins link` instead.
Plugins could support local directories in the future but for now a symlink is required which `mise plugins link` will create for you."#
            ))?;
        }
        let git = Git::new(&self.plugin_path);
        pr.set_message(format!("cloning {repo_url}"));
        git.clone(&repo_url)?;
        if let Some(ref_) = &repo_ref {
            pr.set_message(format!("checking out {ref_}"));
            git.update(Some(ref_.to_string()))?;
        }
        self.exec_hook(pr, "post-plugin-add")?;

        let sha = git.current_sha_short()?;
        pr.finish_with_message(format!(
            "{repo_url}#{}",
            style(&sha).bright().yellow().for_stderr(),
        ));
        Ok(())
    }

    fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        let command_path = self.plugin_path.join("lib/commands");
        if !self.is_installed() || !command_path.exists() || self.name == "direnv" {
            // asdf-direnv is disabled since it conflicts with mise's built-in direnv functionality
            return Ok(vec![]);
        }
        let mut commands = vec![];
        for p in crate::file::ls(&command_path)? {
            let command = p.file_name().unwrap().to_string_lossy().to_string();
            if !command.starts_with("command-") || !command.ends_with(".bash") {
                continue;
            }
            let command = command
                .strip_prefix("command-")
                .unwrap()
                .strip_suffix(".bash")
                .unwrap()
                .split('-')
                .map(|s| s.to_string())
                .collect::<Vec<String>>();
            commands.push(command);
        }
        if commands.is_empty() {
            return Ok(vec![]);
        }

        let topic = Command::new(self.name.clone())
            .about(format!("Commands provided by {} plugin", &self.name))
            .subcommands(commands.into_iter().map(|cmd| {
                Command::new(cmd.join("-"))
                    .about(format!("{} command", cmd.join("-")))
                    .arg(
                        clap::Arg::new("args")
                            .num_args(1..)
                            .allow_hyphen_values(true)
                            .trailing_var_arg(true),
                    )
            }));
        Ok(vec![topic])
    }

    fn execute_external_command(&self, command: &str, args: Vec<String>) -> eyre::Result<()> {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name.clone()).into());
        }
        let script = Script::RunExternalCommand(
            self.plugin_path
                .join("lib/commands")
                .join(format!("command-{command}.bash")),
            args,
        );
        let result = self.script_man.cmd(&script).unchecked().run()?;
        exit(result.status.code().unwrap_or(-1));
    }
}

fn build_script_man(name: &str, plugin_path: &Path) -> ScriptManager {
    let plugin_path_s = plugin_path.to_string_lossy().to_string();
    ScriptManager::new(plugin_path.to_path_buf())
        .with_env("ASDF_PLUGIN_PATH", plugin_path_s.clone())
        .with_env("RTX_PLUGIN_PATH", plugin_path_s.clone())
        .with_env("RTX_PLUGIN_NAME", name.to_string())
        .with_env("RTX_SHIMS_DIR", *dirs::SHIMS)
        .with_env("MISE_PLUGIN_NAME", name.to_string())
        .with_env("MISE_PLUGIN_PATH", plugin_path)
        .with_env("MISE_SHIMS_DIR", *dirs::SHIMS)
}
