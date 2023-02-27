extern crate core;
#[macro_use]
extern crate log;

use color_eyre::eyre::Result;
use color_eyre::{Help, SectionExt};
use console::Term;

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
mod dirs;
mod env;
mod env_diff;
mod errors;
mod fake_asdf;
mod file;
mod git;
mod hook_env;
mod logger;
mod plugins;
pub mod runtimes;
mod shell;
mod shims;
mod shorthands;
mod ui;

mod direnv;
mod hash;
mod toolset;

#[cfg(test)]
mod test;

fn main() -> Result<()> {
    color_eyre::install()?;
    let log_level = *env::RTX_LOG_LEVEL;
    logger::init(log_level, *env::RTX_LOG_FILE_LEVEL);
    handle_ctrlc();

    let mut result = run(&env::ARGS).with_section(|| VERSION.to_string().header("Version:"));
    if log_level < log::LevelFilter::Debug {
        result = result.with_suggestion(|| "Run with RTX_DEBUG=1 for more information.".to_string())
    }
    result
}

fn run(args: &Vec<String>) -> Result<()> {
    let out = &mut Output::new();

    // show version before loading config in case of error
    cli::version::print_version_if_requested(&env::ARGS, out);

    let config = Config::load()?;
    let config = shims::handle_shim(config, args, out)?;
    if hook_env::should_exit_early(&config) {
        return Ok(());
    }
    let cli = Cli::new_with_external_commands(&config)?;
    cli.run(config, args, out)
}

fn handle_ctrlc() {
    ctrlc::set_handler(move || {
        let _ = Term::stderr().show_cursor();
        debug!("Ctrl-C pressed, exiting...");
        std::process::exit(1);
    })
    .expect("Error setting Ctrl-C handler");
}
