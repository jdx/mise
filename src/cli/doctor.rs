use std::fmt::Write;
use std::process::exit;

use console::{pad_str, style, Alignment};
use eyre::Result;
use indenter::indented;

use crate::build_time::built_info;
use crate::cli::version;
use crate::cli::version::VERSION;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::git::Git;
use crate::plugins::PluginType;
use crate::shell::ShellType;
use crate::toolset::Toolset;
use crate::toolset::ToolsetBuilder;
use crate::ui::style;
use crate::{cmd, dirs, forge};
use crate::{duration, env};
use crate::{file, shims};

/// Check mise installation for possible problems
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "dr", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Doctor {
    #[clap(skip)]
    checks: Vec<String>,
}

impl Doctor {
    pub fn run(mut self) -> Result<()> {
        miseprintln!("{}", mise_version())?;
        miseprintln!("{}", build_info())?;
        miseprintln!("{}", shell())?;
        miseprintln!("{}", mise_dirs())?;
        miseprintln!("{}", mise_env_vars())?;

        match Settings::try_get() {
            Ok(settings) => {
                miseprintln!(
                    "{}\n{}\n",
                    style("settings:").bold(),
                    indent(settings.to_string())
                )?;
            }
            Err(err) => warn!("failed to load settings: {}", err),
        }

        match Config::try_get() {
            Ok(config) => self.analyze_config(config)?,
            Err(err) => {
                self.checks.push(format!("failed to load config: {}", err));
            }
        }

        if let Some(latest) = version::check_for_new_version(duration::HOURLY) {
            self.checks.push(format!(
                "new mise version {latest} available, currently on {}",
                *version::V
            ));
        }

        if self.checks.is_empty() {
            miseprintln!("No problems found")?;
        } else {
            let checks_plural = if self.checks.len() == 1 { "" } else { "s" };
            let summary = format!("{} problem{checks_plural} found:", self.checks.len());
            miseprintln!("{}", style(summary).red().bold())?;
            for check in &self.checks {
                miseprintln!("{}\n", check)?;
            }
            exit(1);
        }

        Ok(())
    }

    fn analyze_config(&mut self, config: impl AsRef<Config>) -> Result<()> {
        let config = config.as_ref();

        let yn = |b| {
            if b {
                style("yes").green()
            } else {
                style("no").red()
            }
        };
        miseprintln!("activated: {}", yn(config.is_activated()))?;
        miseprintln!("shims_on_path: {}", yn(shims_on_path()))?;

        miseprintln!("{}", render_config_files(config))?;
        miseprintln!("{}", render_plugins())?;

        for plugin in forge::list() {
            if !plugin.is_installed() {
                self.checks
                    .push(format!("plugin {} is not installed", &plugin.id()));
                continue;
            }
        }

        if !config.is_activated() && !shims_on_path() {
            let cmd = style::nyellow("mise help activate");
            let url = style::nunderline("https://mise.jdx.dev");
            let shims = style::ncyan(dirs::SHIMS.display());
            self.checks.push(formatdoc!(
                r#"mise is not activated, run {cmd} or
                    read documentation at {url} for activation instructions.
                    Alternatively, add the shims directory {shims} to PATH.
                    Using the shims directory is preferred for non-interactive setups."#
            ));
        }

        match ToolsetBuilder::new().build(config) {
            Ok(ts) => {
                self.analyze_shims(&ts);
                let tools = ts
                    .list_current_versions()
                    .into_iter()
                    .map(
                        |(forge, version)| match forge.is_version_installed(&version) {
                            true => version.to_string(),
                            false => format!("{version} (missing)"),
                        },
                    )
                    .collect::<Vec<String>>()
                    .join("\n");

                miseprintln!("{}\n{}\n", style("toolset:").bold(), indent(tools))?;
            }
            Err(err) => self.checks.push(format!("failed to load toolset: {}", err)),
        }

        Ok(())
    }

    fn analyze_shims(&mut self, toolset: &Toolset) {
        let mise_bin = file::which("mise").unwrap_or(env::MISE_BIN.clone());

        if let Ok((missing, extra)) = shims::get_shim_diffs(mise_bin, toolset) {
            let cmd = style::nyellow("mise reshim");

            if !missing.is_empty() {
                self.checks.push(formatdoc!(
                    "shims are missing, run {cmd} to create them
                     Missing shims: {missing}",
                    missing = missing.join(", ")
                ));
            }

            if !extra.is_empty() {
                self.checks.push(formatdoc!(
                    "unused shims are present, run {cmd} to remove them
                     Unused shims: {extra}",
                    extra = extra.join(", ")
                ));
            }
        }
    }
}

fn shims_on_path() -> bool {
    env::PATH.contains(&*dirs::SHIMS)
}

fn mise_dirs() -> String {
    let mut s = style("mise dirs:\n").bold().to_string();
    s.push_str(&format!("  data: {}\n", dirs::DATA.to_string_lossy()));
    s.push_str(&format!("  config: {}\n", dirs::CONFIG.to_string_lossy()));
    s.push_str(&format!("  cache: {}\n", dirs::CACHE.to_string_lossy()));
    s.push_str(&format!("  state: {}\n", dirs::STATE.to_string_lossy()));
    s.push_str(&format!("  shims: {}\n", dirs::SHIMS.to_string_lossy()));
    s
}

fn mise_env_vars() -> String {
    let vars = env::vars()
        .filter(|(k, _)| k.starts_with("MISE_"))
        .collect::<Vec<(String, String)>>();
    let mut s = style("mise environment variables:\n").bold().to_string();
    if vars.is_empty() {
        s.push_str("  (none)\n");
    }
    for (k, v) in vars {
        s.push_str(&format!("  {}={}\n", k, v));
    }
    s
}

fn render_config_files(config: &Config) -> String {
    let mut s = style("config files:\n").bold().to_string();
    for f in config.config_files.keys().rev() {
        s.push_str(&format!("  {}\n", display_path(f)));
    }
    s
}

fn render_plugins() -> String {
    let mut s = style("plugins:\n").bold().to_string();
    let plugins = forge::list()
        .into_iter()
        .filter(|p| p.is_installed())
        .collect::<Vec<_>>();
    let max_plugin_name_len = plugins.iter().map(|p| p.id().len()).max().unwrap_or(0) + 2;
    for p in plugins {
        let padded_name = pad_str(p.id(), max_plugin_name_len, Alignment::Left, None);
        let si = match p.get_plugin_type() {
            PluginType::External => {
                let git = Git::new(dirs::PLUGINS.join(p.id()));
                match git.get_remote_url() {
                    Some(url) => {
                        let sha = git
                            .current_sha_short()
                            .unwrap_or_else(|_| "(unknown)".to_string());
                        format!("  {padded_name} {url}#{sha}\n")
                    }
                    None => format!("  {padded_name}\n"),
                }
            }
            PluginType::Core => format!("  {padded_name} (core)\n"),
        };
        s.push_str(&si);
    }
    s
}

fn mise_version() -> String {
    let mut s = style("mise version:\n").bold().to_string();
    s.push_str(&format!("  {}\n", *VERSION));
    s
}

fn build_info() -> String {
    let mut s = style("build:\n").bold().to_string();
    s.push_str(&format!("  Target: {}\n", built_info::TARGET));
    s.push_str(&format!("  Features: {}\n", built_info::FEATURES_STR));
    s.push_str(&format!("  Built: {}\n", built_info::BUILT_TIME_UTC));
    s.push_str(&format!("  Rust Version: {}\n", built_info::RUSTC_VERSION));
    s.push_str(&format!("  Profile: {}\n", built_info::PROFILE));
    s
}

fn shell() -> String {
    let mut s = style("shell:\n").bold().to_string();
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
            let out = format!("{}\n{}\n", shell_cmd, version);
            s.push_str(&indent(out));
        }
        None => s.push_str("  (unknown)\n"),
    }
    s
}

fn indent(s: String) -> String {
    let mut out = String::new();
    write!(indented(&mut out).with_str("  "), "{}", s).unwrap();
    out
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise doctor</bold>
    [WARN] plugin node is not installed
"#
);
