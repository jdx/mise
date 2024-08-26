use std::process::exit;

use color_eyre::{Section, SectionExt};
use eyre::Report;
use itertools::Itertools;

use crate::cli::version::VERSION;
use crate::cli::Cli;

#[cfg(test)]
#[macro_use]
mod test;

#[macro_use]
mod output;

#[macro_use]
mod regex;

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
mod env;
mod env_diff;
mod errors;
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
mod logger;
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

fn main() -> eyre::Result<()> {
    let args = env::args().collect_vec();
    errors::install()?;

    match Cli::run(&args).with_section(|| VERSION.to_string().header("Version:")) {
        Ok(()) => Ok(()),
        Err(err) => handle_err(err),
    }
}

fn handle_err(err: Report) -> eyre::Result<()> {
    if let Some(err) = err.downcast_ref::<std::io::Error>() {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
    }
    eprintln!("{err:?}");
    exit(1);
}
