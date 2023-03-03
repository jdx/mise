use color_eyre::eyre::Result;
use console::style;
use indenter::indented;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use std::fmt::Write;
use std::process::exit;

use crate::cli::command::Command;
use crate::cli::version::VERSION;
use crate::config::Config;
use crate::env;
use crate::{cli, cmd};

use crate::output::Output;
use crate::shell::ShellType;
use crate::toolset::ToolsetBuilder;

/// Check rtx installation for possible problems.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Doctor {}

impl Command for Doctor {
    fn run(self, mut config: Config, out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config)?;
        rtxprintln!(out, "{}", rtx_version());
        rtxprintln!(out, "{}", shell());
        rtxprintln!(out, "{}", rtx_env_vars());
        rtxprintln!(
            out,
            "{}\n{}\n",
            style("config:").bold(),
            indent(config.to_string())
        );
        rtxprintln!(
            out,
            "{}\n{}\n",
            style("settings:").bold(),
            indent(config.settings.to_string())
        );
        rtxprintln!(
            out,
            "{}\n{}\n",
            style("toolset:").bold(),
            indent(ts.to_string())
        );

        let mut checks = Vec::new();
        for plugin in config.plugins.values() {
            if !plugin.is_installed() {
                checks.push(format!("plugin {} is not installed", plugin.name));
                continue;
            }
        }

        if let Some(latest) = cli::version::check_for_new_version() {
            checks.push(format!(
                "new rtx version {} available, currently on {}",
                latest,
                env!("CARGO_PKG_VERSION")
            ));
        }

        if !config.is_activated() {
            let cmd = style("rtx activate").yellow().for_stderr();
            checks.push(format!(
                "rtx is not activated, run `{cmd}` for setup instructions"
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

fn rtx_version() -> String {
    let mut s = style("rtx version:\n").bold().to_string();
    s.push_str(&format!("  {}\n", *VERSION));
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

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx doctor
      [WARN] plugin nodejs is not installed
    "#, style("Examples:").bold().underlined()}
});
