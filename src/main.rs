extern crate core;
#[macro_use]
extern crate indoc;
#[cfg(test)]
#[macro_use]
extern crate insta;
#[macro_use]
extern crate miette;
#[macro_use]
extern crate strum;

use itertools::Itertools;
use miette::{MietteHandlerOpts, Result};

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
mod task;
pub mod tera;
pub mod timeout;
mod toml;
mod toolset;
mod ui;

fn main() -> Result<()> {
    let args = env::args().collect_vec();

    // TODO: figure out how to display version/help
    // let theme = GraphicalTheme::default();
    // let handler = GraphicalReportHandler::new_themed(theme);
    miette::set_hook(Box::new(|_| {
        Box::new(MietteHandlerOpts::default().with_cause_chain().build())
    }))?;
    Cli::run(&args)
    // match Cli::run(&args).with_context(|| miette!(help = format!("Version: {}", &*VERSION))) {
    //     Ok(()) => Ok(()),
    //     Err(err) => Err(err),
    // Err(err) if log::max_level() < log::LevelFilter::Debug => {
    //     display_friendly_err(err);
    //     exit(1);
    // }
    // Err(err) => {
    //     Err(err).suggestion("Run with --verbose or MISE_VERBOSE=1 for more information.")
    // }
}

// fn display_friendly_err(err: Report) {
//     for err in err.chain() {
//         error!("{err}");
//     }
//     let dim = |s| style(s).dim().for_stderr();
//     error!(
//         "{}",
//         dim("Run with --verbose or MISE_VERBOSE=1 for more information")
//     );
// }
