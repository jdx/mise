extern crate core;
#[macro_use]
extern crate log;

use color_eyre::eyre::Result;

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
mod ui;

mod direnv;
mod hash;
mod shorthand;
mod shorthand_list;
mod toolset;

#[cfg(test)]
mod test;

fn main() -> Result<()> {
    color_eyre::install()?;
    let log_level = *env::RTX_LOG_LEVEL;
    logger::init(log_level, *env::RTX_LOG_FILE_LEVEL);

    match run(&env::ARGS) {
        Err(err) if log_level < log::LevelFilter::Debug => {
            error!("{err}");
            // TODO: tell user they can use --log-level when it's implemented
            error!("Run with RTX_DEBUG=1 for more information.");
            error!("rtx {}", *VERSION);
            std::process::exit(1);
        }
        result => result,
    }
}

fn run(args: &Vec<String>) -> Result<()> {
    let out = &mut Output::new();

    // show version before loading config in case of error
    cli::version::print_version_if_requested(&env::ARGS, out);

    let config = Config::load()?;
    if hook_env::should_exit_early(&config) {
        return Ok(());
    }
    let cli = Cli::new_with_external_commands(&config)?;
    cli.run(config, args, out)
}
