#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]

use std::{
    panic,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::cli::Cli;
use crate::cli::version::VERSION;
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
pub(crate) mod gitlab;
mod gpg;
mod hash;
mod hook_env;
mod hooks;
mod http;
mod install_context;
mod lock_file;
mod lockfile;
pub(crate) mod logger;
pub(crate) mod maplit;
mod migrate;
mod minisign;
pub(crate) mod parallel;
mod path;
mod path_env;
mod plugins;
mod rand;
mod redactions;
mod registry;
pub(crate) mod result;
mod runtime_symlinks;
mod shell;
mod shims;
mod shorthands;
mod sops;
mod sysconfig;
pub(crate) mod task;
pub(crate) mod tera;
pub(crate) mod timeout;
mod toml;
mod toolset;
mod ui;
mod uv;
mod versions_host;
mod watch_files;
mod wildcard;

pub(crate) use crate::exit::exit;
pub(crate) use crate::result::Result;
use crate::ui::multi_progress_report::MultiProgressReport;

fn main() -> eyre::Result<()> {
    let nprocs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or_default();
    let threads = crate::env::MISE_JOBS.unwrap_or(nprocs).max(8);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(threads)
        .build()?
        .block_on(main_())
}

async fn main_() -> eyre::Result<()> {
    color_eyre::install()?;
    install_panic_hook();
    unsafe {
        path_absolutize::update_cwd();
    }
    measure!("main", {
        let args = env::args().collect_vec();
        match Cli::run(&args)
            .await
            .with_section(|| VERSION.to_string().header("Version:"))
        {
            Ok(()) => Ok(()),
            Err(err) => handle_err(err),
        }?;
    });
    if let Some(mpr) = MultiProgressReport::try_get() {
        mpr.stop()?;
    }
    Ok(())
}

fn handle_err(err: Report) -> eyre::Result<()> {
    if let Some(err) = err.downcast_ref::<std::io::Error>() {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
    }
    show_github_rate_limit_err(&err);
    if *env::MISE_FRIENDLY_ERROR
        || (!cfg!(debug_assertions) && log::max_level() < log::LevelFilter::Debug)
    {
        display_friendly_err(&err);
        exit(1);
    }
    let async_backtrace = async_backtrace::taskdump_tree(true);
    Err(err.section(async_backtrace.header("Async Tasks")))
}

fn show_github_rate_limit_err(err: &Report) {
    let msg = format!("{err:?}");
    if msg.contains("HTTP status client error (403 Forbidden) for url (https://api.github.com") {
        warn!(
            "GitHub API returned a 403 Forbidden error. This likely means you have exceeded the rate limit."
        );
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

static ASYNC_PANIC_OCCURRED: AtomicBool = AtomicBool::new(false);

pub fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        if tokio::runtime::Handle::try_current().is_ok()
            && !ASYNC_PANIC_OCCURRED.swap(true, Ordering::SeqCst)
        {
            let bt = async_backtrace::backtrace();
            let mut bt_buffer = String::new();
            if let Some(bt) = bt {
                let locations = &*bt;
                for (index, loc) in locations.iter().enumerate() {
                    bt_buffer.push_str(&format!("{index:3}: {loc:?}\n"));
                }
            } else {
                bt_buffer.push_str("[no accessible async backtrace]");
            }
            let all = async_backtrace::taskdump_tree(true);
            eprintln!(
                "=== Async Backtrace (panic occurred in tokio runtime) ===\n\
                {bt_buffer}\n\
                ------- TASK DUMP TREE -------\n\
                {all}\n\
                === End Async Backtrace ===\n"
            );
        }

        default_hook(panic_info);
    }));
}
