use crate::exit;

use console::{pad_str, style, Alignment};
use indoc::formatdoc;
use itertools::Itertools;
use rayon::prelude::*;

use crate::backend::BackendType;
use crate::build_time::built_info;
use crate::cli::version;
use crate::cli::version::VERSION;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::git::Git;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::PluginType;
use crate::shell::ShellType;
use crate::toolset::{Toolset, ToolsetBuilder};
use crate::ui::{info, style};
use crate::{backend, cmd, dirs, duration, env, file, shims};

/// Check mise installation for possible problems
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "dr", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Doctor {
    #[clap(skip)]
    errors: Vec<String>,
    #[clap(skip)]
    warnings: Vec<String>,
}

impl Doctor {
    pub fn run(mut self) -> eyre::Result<()> {
        info::inline_section("version", &*VERSION)?;
        #[cfg(unix)]
        info::inline_section("activated", yn(env::is_activated()))?;
        info::inline_section("shims_on_path", yn(shims_on_path()))?;

        info::section("build_info", build_info())?;
        info::section("shell", shell())?;
        info::section("dirs", mise_dirs())?;

        match Config::try_get() {
            Ok(config) => self.analyze_config(config)?,
            Err(err) => self.errors.push(format!("failed to load config: {err}")),
        }

        self.analyze_plugins();

        info::section("env_vars", mise_env_vars())?;
        self.analyze_settings()?;

        if let Some(latest) = version::check_for_new_version(duration::HOURLY) {
            self.errors.push(format!(
                "new mise version {latest} available, currently on {}",
                *version::V
            ));
        }

        if self.warnings.is_empty() {
            miseprintln!("No warnings found");
        } else {
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
        match Settings::try_get() {
            Ok(settings) => {
                info::section("settings", settings)?;
            }
            Err(err) => self.errors.push(format!("failed to load settings: {err}")),
        }
        Ok(())
    }
    fn analyze_config(&mut self, config: impl AsRef<Config>) -> eyre::Result<()> {
        let config = config.as_ref();

        info::section("config_files", render_config_files(config))?;
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

        match ToolsetBuilder::new().build(config) {
            Ok(ts) => {
                self.analyze_shims(&ts);
                self.analyze_toolset(&ts)?;
            }
            Err(err) => self.errors.push(format!("failed to load toolset: {}", err)),
        }

        Ok(())
    }

    fn analyze_toolset(&mut self, ts: &Toolset) -> eyre::Result<()> {
        let tools = ts
            .list_current_versions()
            .into_iter()
            .map(|(f, tv)| match f.is_version_installed(&tv, true) {
                true => (tv.to_string(), style::nstyle("")),
                false => (tv.to_string(), style::ndim("(missing)")),
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
            .collect::<Vec<_>>()
            .join("\n");

        info::section("toolset", tools)?;
        Ok(())
    }

    fn analyze_shims(&mut self, toolset: &Toolset) {
        let mise_bin = file::which("mise").unwrap_or(env::MISE_BIN.clone());

        if let Ok((missing, extra)) = shims::get_shim_diffs(mise_bin, toolset) {
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
        time!("doctor::analyze_shims: {:?}");
    }

    fn analyze_plugins(&mut self) {
        for plugin in backend::list() {
            let is_core = CORE_PLUGINS.contains_key(plugin.id());
            let plugin_type = plugin.get_plugin_type();

            if is_core && matches!(plugin_type, PluginType::Asdf | PluginType::Vfox) {
                self.warnings
                    .push(format!("plugin {} overrides a core plugin", &plugin.id()));
            }
        }
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

fn mise_dirs() -> String {
    [
        ("data", &*dirs::DATA),
        ("config", &*dirs::CONFIG),
        ("cache", &*dirs::CACHE),
        ("state", &*dirs::STATE),
        ("shims", &*dirs::SHIMS),
    ]
    .iter()
    .map(|(k, p)| format!("{k}: {}", display_path(p)))
    .join("\n")
}

fn mise_env_vars() -> String {
    let vars = env::vars()
        .filter(|(k, _)| k.starts_with("MISE_"))
        .collect::<Vec<(String, String)>>();
    if vars.is_empty() {
        return "(none)".to_string();
    }
    vars.iter().map(|(k, v)| format!("{k}={v}")).join("\n")
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
    let backends = backend::list_backend_types()
        .into_iter()
        .filter(|f| *f != BackendType::Asdf);
    for b in backends {
        s.push(format!("{}", b));
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
        .into_par_iter()
        .map(|p| {
            let padded_name = pad_str(p.id(), max_plugin_name_len, Alignment::Left, None);
            let extra = match p.get_plugin_type() {
                PluginType::Asdf | PluginType::Vfox => {
                    let git = Git::new(dirs::PLUGINS.join(p.id()));
                    match git.get_remote_url() {
                        Some(url) => {
                            let sha = git
                                .current_sha_short()
                                .unwrap_or_else(|_| "(unknown)".to_string());
                            format!("{url}#{sha}")
                        }
                        None => "".to_string(),
                    }
                }
                PluginType::Core => "(core)".to_string(),
            };
            format!("{padded_name}  {}", style::ndim(extra))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_info() -> String {
    let mut s = vec![];
    s.push(format!("Target: {}", built_info::TARGET));
    s.push(format!("Features: {}", built_info::FEATURES_STR));
    s.push(format!("Built: {}", built_info::BUILT_TIME_UTC));
    s.push(format!("Rust Version: {}", built_info::RUSTC_VERSION));
    s.push(format!("Profile: {}", built_info::PROFILE));
    s.join("\n")
}

fn shell() -> String {
    match ShellType::load().map(|s| s.to_string()) {
        Some(shell) => {
            let shell_cmd = if env::SHELL.ends_with(shell.as_str()) {
                &*env::SHELL
            } else {
                &shell
            };
            let version = cmd!(shell_cmd, "--version")
                .read()
                .unwrap_or_else(|e| format!("failed to get shell version: {}", e));
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
