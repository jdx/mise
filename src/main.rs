use crate::cli::version::VERSION;
use crate::cli::Cli;
use color_eyre::{Section, SectionExt};
use eyre::Report;
use indoc::indoc;
use itertools::Itertools;

#[cfg(test)]
#[macro_use]
mod test;

#[macro_use]
mod output;

#[macro_use]
mod hint;

#[macro_use]
mod timings;

#[macro_use]
mod cmd;

mod aqua;
mod backend;
pub(crate) mod build_time;
mod cache;
mod cli;
mod config;
mod direnv;
mod dirs;
pub(crate) mod duration;
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
mod lockfile;
pub(crate) mod logger;
pub(crate) mod maplit;
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
mod tokio;
mod toml;
mod toolset;
mod ui;
mod versions_host;

pub(crate) use crate::exit::exit;
pub(crate) use crate::toolset::install_state;

fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    measure!("main", {
        let args = env::args().collect_vec();
        match Cli::run(&args).with_section(|| VERSION.to_string().header("Version:")) {
            Ok(()) => Ok(()),
            Err(err) => handle_err(err),
        }?;
    });
    Ok(())
}

fn handle_err(err: Report) -> eyre::Result<()> {
    if let Some(err) = err.downcast_ref::<std::io::Error>() {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
    }
    show_github_rate_limit_err(&err);
    if cfg!(not(debug_assertions)) && log::max_level() < log::LevelFilter::Debug {
        display_friendly_err(&err);
        exit(1);
    }
    Err(err)
}

fn show_github_rate_limit_err(err: &Report) {
    let msg = format!("{err:?}");
    if msg.contains("HTTP status client error (403 Forbidden) for url (https://api.github.com") {
        warn!("GitHub API returned a 403 Forbidden error. This likely means you have exceeded the rate limit.");
        if env::GITHUB_TOKEN.is_none() {
            warn!(indoc!(
                r#"GITHUB_TOKEN is not set. This means mise is making unauthenticated requests to GitHub which have a lower rate limit.
                   To increase the rate limit, set the GITHUB_TOKEN environment variable to a GitHub personal access token.
                   Create a token at https://github.com/settings/tokens and set it as GITHUB_TOKEN in your environment.
                   You do not need to give this token any scopes."#
            ));
        }
    }
}

fn display_friendly_err(err: &Report) {
    for err in err.chain() {
        error!("{err}");
    }
    let msg = ui::style::edim("Run with --verbose or MISE_VERBOSE=1 for more information");
    error!("{msg}");
}
