use std::fmt::Write;
use std::process::exit;

use color_eyre::eyre::Result;
use console::{pad_str, style, Alignment};
use indenter::indented;
use indoc::formatdoc;

use crate::build_time::built_info;
use crate::cli::command::Command;
use crate::cli::version::VERSION;
use crate::config::Config;
use crate::git::Git;
use crate::output::Output;
use crate::plugins::PluginType;
use crate::shell::ShellType;
use crate::toolset::ToolsetBuilder;
use crate::{cli, cmd, dirs};
use crate::{duration, env};

/// Check rtx installation for possible problems.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Doctor {}

impl Command for Doctor {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config)?;
        rtxprintln!(out, "{}", rtx_version());
        rtxprintln!(out, "{}", build_info());
        rtxprintln!(out, "{}", shell());
        rtxprintln!(out, "{}", rtx_data_dir());
        rtxprintln!(out, "{}", rtx_env_vars());
        rtxprintln!(
            out,
            "{}\n{}\n",
            style("settings:").bold(),
            indent(config.settings.to_string())
        );
        rtxprintln!(out, "{}", render_config_files(&config));
        rtxprintln!(out, "{}", render_plugins(&config));
        rtxprintln!(
            out,
            "{}\n{}\n",
            style("toolset:").bold(),
            indent(ts.to_string())
        );

        let mut checks = Vec::new();
        for plugin in config.tools.values() {
            if !plugin.is_installed() {
                checks.push(format!("plugin {} is not installed", &plugin.name));
                continue;
            }
        }

        if let Some(latest) = cli::version::check_for_new_version(duration::HOURLY) {
            checks.push(format!(
                "new rtx version {} available, currently on {}",
                latest,
                env!("CARGO_PKG_VERSION")
            ));
        }

        if !config.is_activated() && !shims_on_path() {
            let cmd = style("rtx help activate").yellow().for_stderr();
            let url = style("https://rtx.pub").underlined().for_stderr();
            let shims = style(dirs::SHIMS.display()).cyan().for_stderr();
            checks.push(formatdoc!(
                r#"rtx is not activated, run {cmd} or
                   read documentation at {url} for activation instructions.
                   Alternatively, add the shims directory {shims} to PATH.
                   Using the shims directory is preferred for non-interactive setups."#
            ));
        }

        if checks.is_empty() {
            rtxprintln!(out, "No problems found");
        } else {
            let checks_plural = if checks.len() == 1 { "" } else { "s" };
            let summary = format!("{} problem{checks_plural} found:", checks.len());
            rtxprintln!(out, "{}", style(summary).red().bold());
            for check in &checks {
                rtxprintln!(out, "{}\n", check);
            }
            exit(1);
        }

        Ok(())
    }
}

fn shims_on_path() -> bool {
    env::PATH.contains(&*dirs::SHIMS)
}

fn rtx_data_dir() -> String {
    let mut s = style("rtx data directory:\n").bold().to_string();
    s.push_str(&format!("  {}\n", env::RTX_DATA_DIR.to_string_lossy()));
    s
}

fn rtx_env_vars() -> String {
    let vars = env::vars()
        .filter(|(k, _)| k.starts_with("RTX_"))
        .collect::<Vec<(String, String)>>();
    let mut s = style("rtx environment variables:\n").bold().to_string();
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
        s.push_str(&format!("  {}\n", f.display()));
    }
    s
}

fn render_plugins(config: &Config) -> String {
    let mut s = style("plugins:\n").bold().to_string();
    let plugins = config
        .tools
        .values()
        .filter(|p| p.is_installed())
        .collect::<Vec<_>>();
    let max_plugin_name_len = plugins.iter().map(|p| p.name.len()).max().unwrap_or(0) + 2;
    for p in plugins {
        let padded_name = pad_str(&p.name, max_plugin_name_len, Alignment::Left, None);
        let si = match p.plugin.get_type() {
            PluginType::External => {
                let git = Git::new(p.plugin_path.clone());
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

fn rtx_version() -> String {
    let mut s = style("rtx version:\n").bold().to_string();
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
  $ <bold>rtx doctor</bold>
  [WARN] plugin node is not installed
"#
);
