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
extern crate pretty_assertions;
#[macro_use]
extern crate contracts;
#[macro_use]
extern crate strum;

use std::process::exit;

use color_eyre::{Section, SectionExt};
use eyre::Report;
use itertools::Itertools;

use crate::cli::version::VERSION;
use crate::cli::Cli;
use crate::ui::style;

#[cfg(test)]
#[macro_use]
mod test;

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
mod forge;
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
mod task;
pub mod tera;
pub mod timeout;
mod toml;
mod toolset;
mod ui;

fn main() -> eyre::Result<()> {
    let args = env::args().collect_vec();
    color_eyre::install()?;

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
    if log::max_level() < log::LevelFilter::Debug {
        display_friendly_err(err);
        exit(1);
    }
    Err(err)
}

fn display_friendly_err(err: Report) {
    for err in err.chain() {
        error!("{err}");
    }
    let msg = style::edim("Run with --verbose or MISE_VERBOSE=1 for more information");
    error!("{msg}");
}
