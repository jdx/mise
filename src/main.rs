extern crate core;
#[macro_use]
extern crate log;

use std::process::exit;

use color_eyre::eyre::Result;
use color_eyre::{Help, Report, SectionExt};
use console::{style, Term};

use crate::cli::version::VERSION;
use crate::cli::Cli;
use crate::config::Config;
use crate::output::Output;

#[macro_use]
mod output;

#[macro_use]
mod regex;

pub mod build_time;
mod cache;
mod cli;
mod cmd;
mod config;
mod default_shorthands;
mod direnv;
mod dirs;
mod env;
mod env_diff;
mod errors;
mod fake_asdf;
mod file;
mod git;
mod hash;
mod hook_env;
mod lock_file;
mod logger;
mod plugins;
pub mod runtimes;
mod shell;
mod shims;
mod shorthands;
#[cfg(test)]
mod test;
mod toolset;
mod ui;

fn main() -> Result<()> {
    color_eyre::install()?;
    let log_level = *env::RTX_LOG_LEVEL;
    logger::init(log_level, *env::RTX_LOG_FILE_LEVEL);
    handle_ctrlc();

    match run(&env::ARGS).with_section(|| VERSION.to_string().header("Version:")) {
        Ok(()) => Ok(()),
        Err(err) if log_level < log::LevelFilter::Debug => {
            display_friendly_err(err);
            exit(1);
        }
        Err(err) => Err(err).suggestion("Run with RTX_DEBUG=1 for more information."),
    }
}

fn run(args: &Vec<String>) -> Result<()> {
    let out = &mut Output::new();

    // show version before loading config in case of error
    cli::version::print_version_if_requested(&env::ARGS, out);

    let config = Config::load()?;
    let config = shims::handle_shim(config, args, out)?;
    if config.should_exit_early {
        return Ok(());
    }
    let cli = Cli::new_with_external_commands(&config);
    cli.run(config, args, out)
}

fn handle_ctrlc() {
    ctrlc::set_handler(move || {
        let _ = Term::stderr().show_cursor();
        debug!("Ctrl-C pressed, exiting...");
        exit(1);
    })
    .expect("Error setting Ctrl-C handler");
}

fn display_friendly_err(err: Report) {
    let dim = |s| style(s).dim().for_stderr();
    let dim_red = |s| style(s).dim().red().for_stderr();
    eprintln!("{} {}", dim_red("rtx"), err);
    eprintln!(
        "{} {}",
        dim_red("rtx"),
        dim("Run with RTX_DEBUG=1 for more information")
    );
}
