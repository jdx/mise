mod path;

use crate::{exit, plugins::PluginEnum};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::backend_type::BackendType;
use crate::build_time::built_info;
use crate::cli::version;
use crate::cli::version::VERSION;
use crate::config::{Config, IGNORED_CONFIG_FILES};
use crate::env::PATH_KEY;
use crate::file::display_path;
use crate::git::Git;
use crate::plugins::PluginType;
use crate::plugins::core::CORE_PLUGINS;
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::ui::{info, style};
use crate::{backend, cmd, dirs, duration, env, file, shims};
use console::{Alignment, pad_str, style};
use heck::ToSnakeCase;
use indexmap::IndexMap;
use indoc::formatdoc;
use itertools::Itertools;
use std::env::split_paths;
use std::path::{Path, PathBuf};
use strum::IntoEnumIterator;

/// Check mise installation for possible problems
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "dr", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Doctor {
    #[clap(subcommand)]
    subcommand: Option<Commands>,
    #[clap(skip)]
    errors: Vec<String>,
    #[clap(skip)]
    warnings: Vec<String>,
    #[clap(long, short = 'J')]
    json: bool,
}

#[derive(Debug, clap::Subcommand)]
pub enum Commands {
    Path(path::Path),
}

impl Doctor {
    pub async fn run(self) -> eyre::Result<()> {
        if let Some(cmd) = self.subcommand {
            match cmd {
                Commands::Path(cmd) => cmd.run().await,
            }
        } else if self.json {
            self.doctor_json().await
        } else {
            self.doctor().await
        }
    }

    async fn doctor_json(mut self) -> crate::Result<()> {
        let mut data: BTreeMap<String, _> = BTreeMap::new();
        data.insert(
            "version".into(),
            serde_json::Value::String(VERSION.to_string()),
        );
        data.insert("activated".into(), env::is_activated().into());
        data.insert("shims_on_path".into(), shims_on_path().into());
        if env::is_activated() && shims_on_path() {
            self.errors.push("shims are on PATH and mise is also activated. You should only use one of these methods.".to_string());
        }
        data.insert(
            "build_info".into(),
            build_info()
                .into_iter()
                .map(|(k, v)| (k.to_snake_case(), v))
                .collect(),
        );
        let shell = shell();
        let mut shell_lines = shell.lines();
        let mut shell = serde_json::Map::new();
        if let Some(name) = shell_lines.next() {
            shell.insert("name".into(), name.into());
        }
        if let Some(version) = shell_lines.next() {
            shell.insert("version".into(), version.into());
        }
        data.insert("shell".into(), shell.into());
        data.insert(
            "dirs".into(),
            mise_dirs()
                .into_iter()
                .map(|(k, p)| (k, p.to_string_lossy().to_string()))
                .collect(),
        );
        data.insert("env_vars".into(), mise_env_vars().into_iter().collect());
        data.insert(
            "settings".into(),
            serde_json::from_str(&cmd!(&*env::MISE_BIN, "settings", "-J").read()?)?,
        );

        let config = Config::get().await?;
        let ts = config.get_toolset().await?;
        self.analyze_shims(&config, ts).await;
        self.analyze_plugins();
        data.insert(
            "paths".into(),
            self.paths(ts)
                .await?
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        );
        data.insert(
            "config_files".into(),
            config
                .config_files
                .keys()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        );
        data.insert(
            "ignored_config_files".into(),
            IGNORED_CONFIG_FILES
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        );

        let tools = ts.list_versions_by_plugin().into_iter().map(|(f, tv)| {
            let versions: serde_json::Value = tv
                .iter()
                .map(|tv: &ToolVersion| {
                    let mut tool = serde_json::Map::new();
                    match f.is_version_installed(&config, tv, true) {
                        true => {
                            tool.insert("version".into(), tv.version.to_string().into());
                        }
                        false => {
                            tool.insert("version".into(), tv.version.to_string().into());
                            tool.insert("missing".into(), true.into());
                            self.errors.push(format!(
                                "tool {tv} is not installed, install with `mise install`"
                            ));
                        }
                    }
                    serde_json::Value::from(tool)
                })
                .collect();
            (f.ba().to_string(), versions)
        });
        data.insert("toolset".into(), tools.collect());

        if !self.errors.is_empty() {
            data.insert("errors".into(), self.errors.clone().into_iter().collect());
        }
        if !self.warnings.is_empty() {
            data.insert(
                "warnings".into(),
                self.warnings.clone().into_iter().collect(),
            );
        }

        let out = serde_json::to_string_pretty(&data)?;
        println!("{out}");

        if !self.errors.is_empty() {
            exit(1);
        }
        Ok(())
    }

    async fn doctor(mut self) -> eyre::Result<()> {
        info::inline_section("version", &*VERSION)?;
        #[cfg(unix)]
        info::inline_section("activated", yn(env::is_activated()))?;
        info::inline_section("shims_on_path", yn(shims_on_path()))?;
        if env::is_activated() && shims_on_path() {
            self.errors.push("shims are on PATH and mise is also activated. You should only use one of these methods.".to_string());
        }

        let build_info = build_info()
            .into_iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .join("\n");
        info::section("build_info", build_info)?;
        info::section("shell", shell())?;
        let mise_dirs = mise_dirs()
            .into_iter()
            .map(|(k, p)| format!("{k}: {}", display_path(p)))
            .join("\n");
        info::section("dirs", mise_dirs)?;

        match Config::get().await {
            Ok(config) => self.analyze_config(&config).await?,
            Err(err) => self.errors.push(format!("failed to load config: {err}")),
        }

        self.analyze_plugins();

        let env_vars = mise_env_vars()
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .join("\n");
        if env_vars.is_empty() {
            info::section("env_vars", "(none)")?;
        } else {
            info::section("env_vars", env_vars)?;
        }
        self.analyze_settings()?;

        if let Some(latest) = version::check_for_new_version(duration::HOURLY).await {
            version::show_latest().await;
            self.errors.push(format!(
                "new mise version {latest} available, currently on {}",
                *version::V
            ));
        }

        miseprintln!();

        if !self.warnings.is_empty() {
            let warnings_plural = if self.warnings.len() == 1 { "" } else { "s" };
            let warning_summary =
                format!("{} warning{warnings_plural} found:", self.warnings.len());
            miseprintln!("{}\n", style(warning_summary).yellow().bold());
            for (i, check) in self.warnings.iter().enumerate() {
                let num = style::nyellow(format!("{}.", i + 1));
                miseprintln!("{num} {}\n", info::indent_by(check, "   ").trim_start());
            }
        }

        if self.errors.is_empty() {
            miseprintln!("No problems found");
        } else {
            let errors_plural = if self.errors.len() == 1 { "" } else { "s" };
            let error_summary = format!("{} problem{errors_plural} found:", self.errors.len());
            miseprintln!("{}\n", style(error_summary).red().bold());
            for (i, check) in self.errors.iter().enumerate() {
                let num = style::nred(format!("{}.", i + 1));
                miseprintln!("{num} {}\n", info::indent_by(check, "   ").trim_start());
            }
            exit(1);
        }

        Ok(())
    }

    fn analyze_settings(&mut self) -> eyre::Result<()> {
        match cmd!("mise", "settings").read() {
            Ok(settings) => {
                info::section("settings", settings)?;
            }
            Err(err) => self.errors.push(format!("failed to load settings: {err}")),
        }
        Ok(())
    }
    async fn analyze_config(&mut self, config: &Arc<Config>) -> eyre::Result<()> {
        info::section("config_files", render_config_files(config))?;
        if IGNORED_CONFIG_FILES.is_empty() {
            println!();
            info::inline_section("ignored_config_files", "(none)")?;
        } else {
            info::section(
                "ignored_config_files",
                IGNORED_CONFIG_FILES.iter().map(display_path).join("\n"),
            )?;
        }
        info::section("backends", render_backends())?;
        info::section("plugins", render_plugins())?;

        for backend in backend::list() {
            if let Some(plugin) = backend.plugin() {
                if !plugin.is_installed() {
                    self.errors
                        .push(format!("plugin {} is not installed", &plugin.name()));
                    continue;
                }
            }
        }

        if !env::is_activated() && !shims_on_path() {
            let shims = style::ncyan(display_path(*dirs::SHIMS));
            if cfg!(windows) {
                self.errors.push(formatdoc!(
                    r#"mise shims are not on PATH
                    Add this directory to PATH: {shims}"#
                ));
            } else {
                let cmd = style::nyellow("mise help activate");
                let url = style::nunderline("https://mise.jdx.dev");
                self.errors.push(formatdoc!(
                    r#"mise is not activated, run {cmd} or
                        read documentation at {url} for activation instructions.
                        Alternatively, add the shims directory {shims} to PATH.
                        Using the shims directory is preferred for non-interactive setups."#
                ));
            }
        }

        match ToolsetBuilder::new().build(config).await {
            Ok(ts) => {
                self.analyze_shims(config, &ts).await;
                self.analyze_toolset(&ts).await?;
                self.analyze_paths(&ts).await?;
            }
            Err(err) => self.errors.push(format!("failed to load toolset: {err}")),
        }

        Ok(())
    }

    async fn analyze_toolset(&mut self, ts: &Toolset) -> eyre::Result<()> {
        let config = Config::get().await?;
        let tools = ts
            .list_current_versions()
            .into_iter()
            .map(|(f, tv)| match f.is_version_installed(&config, &tv, true) {
                true => (tv.to_string(), style::nstyle("")),
                false => {
                    self.errors.push(format!(
                        "tool {tv} is not installed, install with `mise install`"
                    ));
                    (tv.to_string(), style::ndim("(missing)"))
                }
            })
            .collect_vec();
        let max_tool_len = tools
            .iter()
            .map(|(t, _)| t.len())
            .max()
            .unwrap_or(0)
            .min(20);
        let tools = tools
            .into_iter()
            .map(|(t, s)| format!("{}  {s}", pad_str(&t, max_tool_len, Alignment::Left, None)))
            .sorted()
            .collect::<Vec<_>>()
            .join("\n");

        info::section("toolset", tools)?;
        Ok(())
    }

    async fn analyze_shims(&mut self, config: &Arc<Config>, toolset: &Toolset) {
        let mise_bin = file::which("mise").unwrap_or(env::MISE_BIN.clone());

        if let Ok((missing, extra)) = shims::get_shim_diffs(config, mise_bin, toolset).await {
            let cmd = style::nyellow("mise reshim");

            if !missing.is_empty() {
                self.errors.push(formatdoc!(
                    "shims are missing, run {cmd} to create them
                     Missing shims: {missing}",
                    missing = missing.into_iter().join(", ")
                ));
            }

            if !extra.is_empty() {
                self.errors.push(formatdoc!(
                    "unused shims are present, run {cmd} to remove them
                     Unused shims: {extra}",
                    extra = extra.into_iter().join(", ")
                ));
            }
        }
        time!("doctor::analyze_shims");
    }

    fn analyze_plugins(&mut self) {
        for plugin in backend::list() {
            let is_core = CORE_PLUGINS.contains_key(plugin.id());
            let plugin_type = plugin.get_plugin_type();

            if is_core && matches!(plugin_type, Some(PluginType::Asdf | PluginType::Vfox)) {
                self.warnings
                    .push(format!("plugin {} overrides a core plugin", &plugin.id()));
            }
        }
    }

    async fn paths(&mut self, ts: &Toolset) -> eyre::Result<Vec<PathBuf>> {
        let config = Config::get().await?;
        let env = ts.full_env(&config).await?;
        let path = env
            .get(&*PATH_KEY)
            .ok_or_else(|| eyre::eyre!("Path not found"))?;
        Ok(split_paths(path).collect())
    }

    async fn analyze_paths(&mut self, ts: &Toolset) -> eyre::Result<()> {
        let paths = self
            .paths(ts)
            .await?
            .into_iter()
            .map(display_path)
            .join("\n");

        info::section("path", paths)?;
        Ok(())
    }
}

fn shims_on_path() -> bool {
    env::PATH.contains(&dirs::SHIMS.to_path_buf())
}

fn yn(b: bool) -> String {
    if b {
        style("yes").green().to_string()
    } else {
        style("no").red().to_string()
    }
}

fn mise_dirs() -> Vec<(String, &'static Path)> {
    [
        ("cache", &*dirs::CACHE),
        ("config", &*dirs::CONFIG),
        ("data", &*dirs::DATA),
        ("shims", &*dirs::SHIMS),
        ("state", &*dirs::STATE),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), **v))
    .collect()
}

fn mise_env_vars() -> Vec<(String, String)> {
    const REDACT_KEYS: &[&str] = &[
        "MISE_GITHUB_TOKEN",
        "MISE_GITLAB_TOKEN",
        "MISE_GITHUB_ENTERPRISE_TOKEN",
        "MISE_GITLAB_ENTERPRISE_TOKEN",
    ];
    env::vars()
        .filter(|(k, _)| k.starts_with("MISE_"))
        .map(|(k, v)| {
            let v = if REDACT_KEYS.contains(&k.as_str()) {
                style::ndim("REDACTED").to_string()
            } else {
                v
            };
            (k, v)
        })
        .collect()
}

fn render_config_files(config: &Config) -> String {
    config
        .config_files
        .keys()
        .rev()
        .map(display_path)
        .join("\n")
}

fn render_backends() -> String {
    let mut s = vec![];
    for b in BackendType::iter().filter(|b| b != &BackendType::Unknown) {
        s.push(format!("{b}"));
    }
    s.join("\n")
}

fn render_plugins() -> String {
    let plugins = backend::list()
        .into_iter()
        .filter(|b| {
            b.plugin()
                .is_some_and(|p| p.is_installed() && b.get_type() == BackendType::Asdf)
        })
        .collect::<Vec<_>>();
    let max_plugin_name_len = plugins
        .iter()
        .map(|p| p.id().len())
        .max()
        .unwrap_or(0)
        .min(40);
    plugins
        .into_iter()
        .filter(|b| b.plugin().is_some())
        .map(|p| {
            let p = p.plugin().unwrap();
            let padded_name = pad_str(p.name(), max_plugin_name_len, Alignment::Left, None);
            let extra = match p {
                PluginEnum::Asdf(_) | PluginEnum::Vfox(_) => {
                    let git = Git::new(dirs::PLUGINS.join(p.name()));
                    match git.get_remote_url() {
                        Some(url) => {
                            let sha = git
                                .current_sha_short()
                                .unwrap_or_else(|_| "(unknown)".to_string());
                            format!("{url}#{sha}")
                        }
                        None => "".to_string(),
                    }
                } // TODO: PluginType::Core => "(core)".to_string(),
            };
            format!("{padded_name}  {}", style::ndim(extra))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_info() -> IndexMap<String, &'static str> {
    let mut s = IndexMap::new();
    s.insert("Target".into(), built_info::TARGET);
    s.insert("Features".into(), built_info::FEATURES_STR);
    s.insert("Built".into(), built_info::BUILT_TIME_UTC);
    s.insert("Rust Version".into(), built_info::RUSTC_VERSION);
    s.insert("Profile".into(), built_info::PROFILE);
    s
}

fn shell() -> String {
    match env::MISE_SHELL.map(|s| s.to_string()) {
        Some(shell) => {
            let shell_cmd = if env::SHELL.ends_with(shell.as_str()) {
                &*env::SHELL
            } else {
                &shell
            };
            let version = cmd!(shell_cmd, "--version")
                .read()
                .unwrap_or_else(|e| format!("failed to get shell version: {e}"));
            format!("{shell_cmd}\n{version}")
        }
        None => "(unknown)".to_string(),
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise doctor</bold>
    [WARN] plugin node is not installed
"#
);
