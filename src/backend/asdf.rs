use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fmt::{Debug, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::{Arc, RwLock};

use clap::Command;
use color_eyre::eyre::{bail, eyre, Result, WrapErr};
use console::style;
use itertools::Itertools;
use rayon::prelude::*;
use url::Url;

use crate::backend::{ABackend, Backend, BackendList, BackendType};
use crate::cache::CacheManager;
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::default_shorthands::{DEFAULT_SHORTHANDS, TRUSTED_SHORTHANDS};
use crate::env::MISE_FETCH_REMOTE_VERSIONS_TIMEOUT;
use crate::env_diff::{EnvDiff, EnvDiffOperation};
use crate::errors::Error::PluginNotInstalled;
use crate::file::display_path;
use crate::git::Git;
use crate::hash::hash_to_str;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::mise_plugin_toml::MisePluginToml;
use crate::plugins::Script::{Download, ExecEnv, Install, ParseLegacyFile};
use crate::plugins::{APlugin, Plugin, PluginType, Script, ScriptManager};
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::timeout::run_with_timeout;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::prompt;
use crate::{dirs, env, file, http};

/// This represents a plugin installed to ~/.local/share/mise/plugins
pub struct Asdf {
    pub fa: BackendArg,
    pub name: String,
    pub plugin_path: PathBuf,
    pub repo_url: Option<String>,
    pub toml: MisePluginToml,
    plugin: Box<AsdfPlugin>,
    script_man: ScriptManager,
    cache: ExternalPluginCache,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_stable_cache: CacheManager<Option<String>>,
    alias_cache: CacheManager<Vec<(String, String)>>,
    legacy_filename_cache: CacheManager<Vec<String>>,
}

impl Asdf {
    pub fn new(name: String) -> Self {
        let plugin_path = dirs::PLUGINS.join(&name);
        let mut toml_path = plugin_path.join("mise.plugin.toml");
        if plugin_path.join("rtx.plugin.toml").exists() {
            toml_path = plugin_path.join("rtx.plugin.toml");
        }
        let toml = MisePluginToml::from_file(&toml_path).unwrap();
        let fa = BackendArg::new(BackendType::Asdf, &name);
        Self {
            script_man: build_script_man(&name, &plugin_path),
            cache: ExternalPluginCache::default(),
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE)
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-all")),
            latest_stable_cache: CacheManager::new(
                fa.cache_path.join("latest_stable-$KEY.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE)
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/latest-stable")),
            alias_cache: CacheManager::new(fa.cache_path.join("aliases-$KEY.msgpack.z"))
                .with_fresh_file(plugin_path.clone())
                .with_fresh_file(plugin_path.join("bin/list-aliases")),
            legacy_filename_cache: CacheManager::new(
                fa.cache_path.join("legacy_filenames-$KEY.msgpack.z"),
            )
            .with_fresh_file(plugin_path.clone())
            .with_fresh_file(plugin_path.join("bin/list-legacy-filenames")),
            plugin_path,
            repo_url: None,
            toml,
            name,
            fa,
        }
    }
    pub fn plugin(&self) -> &dyn Plugin {
        &*self.plugin
    }

    pub fn list() -> Result<BackendList> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .into_par_iter()
            .map(|name| Arc::new(Self::new(name)) as ABackend)
            .collect())
    }

    fn get_repo_url(&self, config: &Config) -> Result<String> {
        self.repo_url
            .clone()
            .or_else(|| config.get_repo_url(&self.name))
            .ok_or_else(|| eyre!("No repository found for plugin {}", self.name))
    }

    fn install(&self, pr: &dyn SingleReport) -> Result<()> {
        let config = Config::get();
        let repository = self.get_repo_url(&config)?;
        let (repo_url, repo_ref) = Git::split_url_and_ref(&repository);
        debug!("install {} {:?}", self.name, repository);

        if self.is_installed() {
            self.uninstall(pr)?;
        }

        let git = Git::new(self.plugin_path.to_path_buf());
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

    fn fetch_versions(&self) -> Result<Option<Vec<String>>> {
        if !*env::MISE_USE_VERSIONS_HOST {
            return Ok(None);
        }
        // ensure that we're using a default shorthand plugin
        let git = Git::new(self.plugin_path.to_path_buf());
        let normalized_remote = normalize_remote(&git.get_remote_url().unwrap_or_default())
            .unwrap_or("INVALID_URL".into());
        let shorthand_remote = DEFAULT_SHORTHANDS
            .get(self.name.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if normalized_remote != normalize_remote(&shorthand_remote).unwrap_or_default() {
            return Ok(None);
        }
        let versions =
            match HTTP_FETCH.get_text(format!("http://mise-versions.jdx.dev/{}", self.name)) {
                Err(err) if http::error_code(&err) == Some(404) => return Ok(None),
                res => res?,
            };
        let versions = versions
            .lines()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect_vec();
        match versions.is_empty() {
            true => Ok(None),
            false => Ok(Some(versions)),
        }
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        let settings = Settings::try_get()?;
        match self.fetch_versions() {
            Ok(Some(versions)) => return Ok(versions),
            Err(err) => warn!(
                "Failed to fetch remote versions for plugin {}: {}",
                style(&self.name).blue().for_stderr(),
                err
            ),
            _ => {}
        };
        let cmd = self.script_man.cmd(&Script::ListAll);
        let result = run_with_timeout(
            move || {
                let result = cmd.stdout_capture().stderr_capture().unchecked().run()?;
                Ok(result)
            },
            *MISE_FETCH_REMOTE_VERSIONS_TIMEOUT,
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
        } else if settings.verbose {
            display_stderr();
        }

        Ok(stdout
            .split_whitespace()
            .map(|v| regex!(r"^v(\d+)").replace(v, "$1").to_string())
            .collect())
    }

    fn fetch_legacy_filenames(&self) -> Result<Vec<String>> {
        let stdout = self.script_man.read(&Script::ListLegacyFilenames)?;
        Ok(self.parse_legacy_filenames(&stdout))
    }
    fn parse_legacy_filenames(&self, data: &str) -> Vec<String> {
        data.split_whitespace().map(|v| v.into()).collect()
    }
    fn fetch_latest_stable(&self) -> Result<Option<String>> {
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

    fn has_list_alias_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListAliases)
    }
    fn has_list_legacy_filenames_script(&self) -> bool {
        self.script_man.script_exists(&Script::ListLegacyFilenames)
    }
    fn has_latest_stable_script(&self) -> bool {
        self.script_man.script_exists(&Script::LatestStable)
    }
    fn fetch_aliases(&self) -> Result<Vec<(String, String)>> {
        let stdout = self.script_man.read(&Script::ListAliases)?;
        Ok(self.parse_aliases(&stdout))
    }
    fn parse_aliases(&self, data: &str) -> Vec<(String, String)> {
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

    fn fetch_cached_legacy_file(&self, legacy_file: &Path) -> Result<Option<String>> {
        let fp = self.legacy_cache_file_path(legacy_file);
        if !fp.exists() || fp.metadata()?.modified()? < legacy_file.metadata()?.modified()? {
            return Ok(None);
        }

        Ok(Some(fs::read_to_string(fp)?.trim().into()))
    }

    fn legacy_cache_file_path(&self, legacy_file: &Path) -> PathBuf {
        self.fa
            .cache_path
            .join("legacy")
            .join(&self.name)
            .join(hash_to_str(&legacy_file.to_string_lossy()))
            .with_extension("txt")
    }

    fn write_legacy_cache(&self, legacy_file: &Path, legacy_version: &str) -> Result<()> {
        let fp = self.legacy_cache_file_path(legacy_file);
        file::create_dir_all(fp.parent().unwrap())?;
        file::write(fp, legacy_version)?;
        Ok(())
    }

    fn fetch_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<String>> {
        let list_bin_paths = self.plugin_path.join("bin/list-bin-paths");
        let bin_paths = if matches!(tv.request, ToolRequest::System(_)) {
            Vec::new()
        } else if list_bin_paths.exists() {
            let sm = self.script_man_for_tv(tv)?;
            // TODO: find a way to enable this without deadlocking
            // for (t, tv) in ts.list_current_installed_versions(config) {
            //     if t.name == self.name {
            //         continue;
            //     }
            //     for p in t.list_bin_paths(config, ts, &tv)? {
            //         sm.prepend_path(p);
            //     }
            // }
            let output = sm.cmd(&Script::ListBinPaths).read()?;
            output
                .split_whitespace()
                .map(|f| {
                    if f == "." {
                        String::new()
                    } else {
                        f.to_string()
                    }
                })
                .collect()
        } else {
            vec!["bin".into()]
        };
        Ok(bin_paths)
    }
    fn fetch_exec_env(&self, ts: &Toolset, tv: &ToolVersion) -> Result<BTreeMap<String, String>> {
        let mut sm = self.script_man_for_tv(tv)?;
        for p in ts.list_paths() {
            sm.prepend_path(p);
        }
        let script = sm.get_script_path(&ExecEnv);
        let ed = EnvDiff::from_bash_script(&script, &sm.env)?;
        let env = ed
            .to_patches()
            .into_iter()
            .filter_map(|p| match p {
                EnvDiffOperation::Add(key, value) => Some((key, value)),
                EnvDiffOperation::Change(key, value) => Some((key, value)),
                _ => None,
            })
            .collect();
        Ok(env)
    }

    fn script_man_for_tv(&self, tv: &ToolVersion) -> Result<ScriptManager> {
        let config = Config::get();
        let mut sm = self.script_man.clone();
        for (key, value) in &tv.request.options() {
            let k = format!("RTX_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
            let k = format!("MISE_TOOL_OPTS__{}", key.to_uppercase());
            sm = sm.with_env(k, value.clone());
        }
        if let Some(project_root) = &config.project_root {
            let project_root = project_root.to_string_lossy().to_string();
            sm = sm.with_env("RTX_PROJECT_ROOT", project_root.clone());
            sm = sm.with_env("MISE_PROJECT_ROOT", project_root);
        }
        let install_type = match &tv.request {
            ToolRequest::Version { .. } | ToolRequest::Prefix { .. } => "version",
            ToolRequest::Ref { .. } => "ref",
            ToolRequest::Path(_, _) => "path",
            ToolRequest::Sub { .. } => "sub",
            ToolRequest::System(_) => {
                panic!("should not be called for system tool")
            }
        };
        let install_version = match &tv.request {
            ToolRequest::Ref { ref_: v, .. } => v, // should not have "ref:" prefix
            _ => &tv.version,
        };
        // add env vars from .mise.toml files
        for (key, value) in config.env()? {
            sm = sm.with_env(key, value.clone());
        }
        let install = tv.install_path().to_string_lossy().to_string();
        let download = tv.download_path().to_string_lossy().to_string();
        sm = sm
            .with_env("ASDF_DOWNLOAD_PATH", &download)
            .with_env("ASDF_INSTALL_PATH", &install)
            .with_env("ASDF_INSTALL_TYPE", install_type)
            .with_env("ASDF_INSTALL_VERSION", install_version)
            .with_env("RTX_DOWNLOAD_PATH", &download)
            .with_env("RTX_INSTALL_PATH", &install)
            .with_env("RTX_INSTALL_TYPE", install_type)
            .with_env("RTX_INSTALL_VERSION", install_version)
            .with_env("MISE_DOWNLOAD_PATH", download)
            .with_env("MISE_INSTALL_PATH", install)
            .with_env("MISE_INSTALL_TYPE", install_type)
            .with_env("MISE_INSTALL_VERSION", install_version);
        Ok(sm)
    }

    fn exec_hook(&self, pr: &dyn SingleReport, hook: &str) -> Result<()> {
        self.exec_hook_env(pr, hook, Default::default())
    }
    fn exec_hook_env(
        &self,
        pr: &dyn SingleReport,
        hook: &str,
        env: HashMap<OsString, OsString>,
    ) -> Result<()> {
        let script = Script::Hook(hook.to_string());
        let mut sm = self.script_man.clone();
        sm.env.extend(env);
        if sm.script_exists(&script) {
            pr.set_message(format!("executing {hook} hook"));
            sm.run_by_line(&script, pr)?;
        }
        Ok(())
    }

    fn exec_hook_post_plugin_update(
        &self,
        pr: &dyn SingleReport,
        pre: String,
        post: String,
    ) -> Result<()> {
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

impl Eq for Asdf {}

impl PartialEq for Asdf {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for Asdf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Backend for Asdf {
    fn fa(&self) -> &BackendArg {
        &self.fa
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Asdf
    }

    fn get_dependencies(&self, tvr: &ToolRequest) -> Result<Vec<BackendArg>> {
        let out = match tvr.backend().name.as_str() {
            "poetry" | "pipenv" | "pipx" => vec!["python"],
            "elixir" => vec!["erlang"],
            _ => vec![],
        };
        Ok(out.into_iter().map(|s| s.into()).collect())
    }

    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .wrap_err_with(|| {
                eyre!(
                    "Failed listing remote versions for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> Result<Option<String>> {
        if !self.has_latest_stable_script() {
            return self.latest_version(Some("latest".into()));
        }
        self.latest_stable_cache
            .get_or_try_init(|| self.fetch_latest_stable())
            .wrap_err_with(|| {
                eyre!(
                    "Failed fetching latest stable version for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })
            .cloned()
    }

    fn get_remote_url(&self) -> Option<String> {
        let git = Git::new(self.plugin_path.to_path_buf());
        git.get_remote_url().or_else(|| self.repo_url.clone())
    }

    fn is_installed(&self) -> bool {
        self.plugin_path.exists()
    }

    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        if let Some(data) = &self.toml.list_aliases.data {
            return Ok(self.parse_aliases(data).into_iter().collect());
        }
        if !self.has_list_alias_script() {
            return Ok(BTreeMap::new());
        }
        let aliases = self
            .alias_cache
            .get_or_try_init(|| self.fetch_aliases())
            .wrap_err_with(|| {
                eyre!(
                    "Failed fetching aliases for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })?
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(aliases)
    }

    fn legacy_filenames(&self) -> Result<Vec<String>> {
        if let Some(data) = &self.toml.list_legacy_filenames.data {
            return Ok(self.parse_legacy_filenames(data));
        }
        if !self.has_list_legacy_filenames_script() {
            return Ok(vec![]);
        }
        self.legacy_filename_cache
            .get_or_try_init(|| self.fetch_legacy_filenames())
            .wrap_err_with(|| {
                eyre!(
                    "Failed fetching legacy filenames for plugin {}",
                    style(&self.name).blue().for_stderr(),
                )
            })
            .cloned()
    }

    fn parse_legacy_file(&self, legacy_file: &Path) -> Result<String> {
        if let Some(cached) = self.fetch_cached_legacy_file(legacy_file)? {
            return Ok(cached);
        }
        trace!("parsing legacy file: {}", legacy_file.to_string_lossy());
        let script = ParseLegacyFile(legacy_file.to_string_lossy().into());
        let legacy_version = match self.script_man.script_exists(&script) {
            true => self.script_man.read(&script)?,
            false => fs::read_to_string(legacy_file)?,
        }
        .trim()
        .to_string();

        self.write_legacy_cache(legacy_file, &legacy_version)?;
        Ok(legacy_version)
    }

    fn external_commands(&self) -> Result<Vec<Command>> {
        let command_path = self.plugin_path.join("lib/commands");
        if !self.is_installed() || !command_path.exists() || self.name == "direnv" {
            // asdf-direnv is disabled since it conflicts with mise's built-in direnv functionality
            return Ok(vec![]);
        }
        let mut commands = vec![];
        for p in file::ls(&command_path)? {
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

    fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
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

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        let mut sm = self.script_man_for_tv(&ctx.tv)?;

        for p in ctx.ts.list_paths() {
            sm.prepend_path(p);
        }

        let run_script = |script| sm.run_by_line(script, ctx.pr.as_ref());

        if sm.script_exists(&Download) {
            ctx.pr.set_message("downloading".into());
            run_script(&Download)?;
        }
        ctx.pr.set_message("installing".into());
        run_script(&Install)?;
        file::remove_dir(&self.fa.downloads_path)?;

        Ok(())
    }

    fn uninstall_version_impl(&self, pr: &dyn SingleReport, tv: &ToolVersion) -> Result<()> {
        if self.plugin_path.join("bin/uninstall").exists() {
            self.script_man_for_tv(tv)?
                .run_by_line(&Script::Uninstall, pr)?;
        }
        Ok(())
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        Ok(self
            .cache
            .list_bin_paths(self, tv, || self.fetch_bin_paths(tv))?
            .into_iter()
            .map(|path| tv.install_short_path().join(path))
            .collect())
    }

    fn exec_env(
        &self,
        config: &Config,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        if matches!(tv.request, ToolRequest::System(_)) {
            return Ok(BTreeMap::new());
        }
        if !self.script_man.script_exists(&ExecEnv) || *env::__MISE_SCRIPT {
            // if the script does not exist, or we're already running from within a script,
            // the second is to prevent infinite loops
            return Ok(BTreeMap::new());
        }
        self.cache
            .exec_env(config, self, tv, || self.fetch_exec_env(ts, tv))
    }
}

impl Debug for Asdf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalPlugin")
            .field("name", &self.name)
            .field("plugin_path", &self.plugin_path)
            .field("cache_path", &self.fa.cache_path)
            .field("downloads_path", &self.fa.downloads_path)
            .field("installs_path", &self.fa.installs_path)
            .field("repo_url", &self.repo_url)
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct ExternalPluginCache {
    list_bin_paths: RwLock<HashMap<ToolRequest, CacheManager<Vec<String>>>>,
    exec_env: RwLock<HashMap<ToolRequest, CacheManager<BTreeMap<String, String>>>>,
}

impl ExternalPluginCache {
    pub fn list_bin_paths<F>(
        &self,
        plugin: &Asdf,
        tv: &ToolVersion,
        fetch: F,
    ) -> eyre::Result<Vec<String>>
    where
        F: FnOnce() -> eyre::Result<Vec<String>>,
    {
        let mut w = self.list_bin_paths.write().unwrap();
        let cm = w.entry(tv.request.clone()).or_insert_with(|| {
            let list_bin_paths_filename = match &plugin.toml.list_bin_paths.cache_key {
                Some(key) => {
                    let config = Config::get();
                    let key = render_cache_key(&config, tv, key);
                    let filename = format!("{}-$KEY.msgpack.z", key);
                    tv.cache_path().join("list_bin_paths").join(filename)
                }
                None => tv.cache_path().join("list_bin_paths-$KEY.msgpack.z"),
            };
            CacheManager::new(list_bin_paths_filename)
                .with_fresh_file(dirs::DATA.to_path_buf())
                .with_fresh_file(plugin.plugin_path.clone())
                .with_fresh_file(tv.install_path())
        });
        cm.get_or_try_init(fetch).cloned()
    }

    pub fn exec_env<F>(
        &self,
        config: &Config,
        plugin: &Asdf,
        tv: &ToolVersion,
        fetch: F,
    ) -> eyre::Result<BTreeMap<String, String>>
    where
        F: FnOnce() -> eyre::Result<BTreeMap<String, String>>,
    {
        let mut w = self.exec_env.write().unwrap();
        let cm = w.entry(tv.request.clone()).or_insert_with(|| {
            let exec_env_filename = match &plugin.toml.exec_env.cache_key {
                Some(key) => {
                    let key = render_cache_key(config, tv, key);
                    let filename = format!("{}-$KEY.msgpack.z", key);
                    tv.cache_path().join("exec_env").join(filename)
                }
                None => tv.cache_path().join("exec_env-$KEY.msgpack.z"),
            };
            CacheManager::new(exec_env_filename)
                .with_fresh_file(dirs::DATA.to_path_buf())
                .with_fresh_file(plugin.plugin_path.clone())
                .with_fresh_file(tv.install_path())
        });
        cm.get_or_try_init(fetch).cloned()
    }
}

fn render_cache_key(config: &Config, tv: &ToolVersion, cache_key: &[String]) -> String {
    let elements = cache_key
        .iter()
        .map(|tmpl| {
            let s = parse_template(config, tv, tmpl).unwrap();
            let s = s.trim().to_string();
            trace!("cache key element: {} -> {}", tmpl.trim(), s);
            let mut s = hash_to_str(&s);
            s.truncate(10);
            s
        })
        .collect::<Vec<String>>();
    elements.join("-")
}

fn parse_template(config: &Config, tv: &ToolVersion, tmpl: &str) -> eyre::Result<String> {
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("project_root", &config.project_root);
    ctx.insert("opts", &tv.request.options());
    get_tera(
        config
            .project_root
            .as_ref()
            .or(env::current_dir().as_ref().ok())
            .map(|p| p.as_path()),
    )
    .render_str(tmpl, &ctx)
    .wrap_err_with(|| eyre!("failed to parse template: {tmpl}"))
}

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}

#[cfg(test)]
mod tests {
    use test_log::test;

    use crate::test::reset;

    use super::*;

    #[test]
    fn test_debug() {
        reset();
        let plugin = Asdf::new(String::from("dummy"));
        assert!(format!("{:?}", plugin).starts_with("ExternalPlugin { name: \"dummy\""));
    }
}
