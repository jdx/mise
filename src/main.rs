extern crate core;
#[macro_use]
extern crate eyre;
#[macro_use]
extern crate indoc;
#[cfg(test)]
#[macro_use]
extern crate insta;
#[cfg(test)]
#[macro_use]
mod test;

use std::process::exit;

use color_eyre::{Help, Report, SectionExt};
use console::{style, Term};
use eyre::Result;

use crate::cli::version::VERSION;
use crate::cli::Cli;
use crate::config::{Config, Settings};

#[macro_use]
mod output;

#[macro_use]
mod regex;

#[macro_use]
mod cmd;

pub mod build_time;
mod cache;
mod cli;
mod config;
mod default_shorthands;
mod direnv;
mod dirs;
pub mod duration;
mod env;
mod env_diff;
mod errors;
mod fake_asdf;
mod file;
mod git;
pub mod github;
mod hash;
mod hook_env;
mod http;
mod install_context;
mod lock_file;
mod logger;
mod migrate;
mod path_env;
mod plugins;
mod rand;
mod runtime_symlinks;
mod shell;
mod shims;
mod shorthands;
pub mod tera;
pub mod timeout;
mod toml;
mod toolset;
mod ui;

fn main() -> Result<()> {
    rayon::spawn(|| {
        Cli::new(); // this is slow so we memoize it in the background
    });
    *env::ARGS.write().unwrap() = env::args().collect();
    color_eyre::install()?;
    let cli_settings = Cli::new().settings(&env::ARGS.read().unwrap());
    Settings::add_partial(cli_settings);
    let log_level = logger::init();
    handle_ctrlc();

    match run().with_section(|| VERSION.to_string().header("Version:")) {
        Ok(()) => Ok(()),
        Err(err) if log_level < log::LevelFilter::Debug => {
            display_friendly_err(err);
            exit(1);
        }
        Err(err) => {
            Err(err).suggestion("Run with --verbose or RTX_VERBOSE=1 for more information.")
        }
    }
}

fn run() -> Result<()> {
    // show version before loading config in case of error
    cli::version::print_version_if_requested();
    migrate::run();

    let config = Config::try_get()?;
    shims::handle_shim(&config)?;
    if config.should_exit_early {
        return Ok(());
    }
    let cli = Cli::new_with_external_commands(&config);
    cli.run(&env::ARGS.read().unwrap())
}

fn handle_ctrlc() {
    let _ = ctrlc::set_handler(move || {
        let _ = Term::stderr().show_cursor();
        debug!("Ctrl-C pressed, exiting...");
        exit(1);
    });
}

fn display_friendly_err(err: Report) {
    let dim = |s| style(s).dim().for_stderr();
    let dim_red = |s| style(s).dim().red().for_stderr();
    for err in err.chain() {
        eprintln!("{} {}", dim_red("rtx"), err);
    }
    eprintln!(
        "{} {}",
        dim_red("rtx"),
        dim("Run with RTX_DEBUG=1 for more information")
    );
}
