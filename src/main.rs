use crate::cli::version::VERSION;
use crate::cli::Cli;
use color_eyre::{Section, SectionExt};
use eyre::Report;
use itertools::Itertools;

#[cfg(test)]
#[macro_use]
mod test;

#[macro_use]
mod output;

#[macro_use]
mod cmd;

mod backend;
pub(crate) mod build_time;
mod cache;
mod cli;
mod config;
mod default_shorthands;
mod direnv;
mod dirs;
pub(crate) mod duration;
pub(crate) mod eager;
mod env;
mod env_diff;
mod errors;
mod exit;
#[cfg_attr(windows, path = "fake_asdf_windows.rs")]
mod fake_asdf;
mod file;
mod git;
pub(crate) mod github;
mod hash;
mod hook_env;
mod http;
mod install_context;
mod lock_file;
pub(crate) mod logger;
mod migrate;
mod path_env;
mod plugins;
mod rand;
mod registry;
pub(crate) mod result;
mod runtime_symlinks;
mod shell;
mod shims;
mod shorthands;
pub(crate) mod task;
pub(crate) mod tera;
pub(crate) mod timeout;
mod toml;
mod toolset;
mod ui;

pub use crate::exit::exit;

fn main() -> eyre::Result<()> {
    output::get_time_diff("", ""); // throwaway call to initialize the timer
    eager::early_init();
    let args = env::args().collect_vec();
    color_eyre::install()?;
    time!("main start");

    match Cli::run(&args).with_section(|| VERSION.to_string().header("Version:")) {
        Ok(()) => Ok(()),
        Err(err) => handle_err(err),
    }?;
    time!("main done");
    Ok(())
}

fn handle_err(err: Report) -> eyre::Result<()> {
    if let Some(err) = err.downcast_ref::<std::io::Error>() {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
    }
    if cfg!(not(debug_assertions)) && log::max_level() < log::LevelFilter::Debug {
        display_friendly_err(err);
        exit(1);
    }
    Err(err)
}

fn display_friendly_err(err: Report) {
    for err in err.chain() {
        error!("{err}");
    }
    let msg = ui::style::edim("Run with --verbose or MISE_VERBOSE=1 for more information");
    error!("{msg}");
}
