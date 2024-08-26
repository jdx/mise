use std::fmt;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolSource};
use crate::{env, ui};
use eyre::{EyreHandler, Report};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to resolve {tr} from {ts}: {source:#}")]
    FailedToResolveVersion {
        tr: ToolRequest,
        ts: ToolSource,
        source: Report,
    },
    #[error("[{0}] plugin not installed")]
    PluginNotInstalled(String),
    #[error("{0}@{1} not installed")]
    VersionNotInstalled(String, String),
    #[error("{} exited with non-zero status: {}", .0, render_exit_status(.1))]
    ScriptFailed(String, Option<ExitStatus>),
    #[error(
        "Config file {} is not trusted.\nTrust it with `mise trust`.",
        display_path(.0)
    )]
    UntrustedConfig(PathBuf),
}

fn render_exit_status(exit_status: &Option<ExitStatus>) -> String {
    match exit_status.and_then(|s| s.code()) {
        Some(exit_status) => format!("exit code {exit_status}"),
        None => "no exit status".into(),
    }
}

impl Error {
    pub fn is_argument_err(err: &Report) -> bool {
        err.downcast_ref::<Error>()
            .map(|e| {
                matches!(
                    e,
                    Error::FailedToResolveVersion {
                        ts: ToolSource::Argument,
                        ..
                    }
                )
            })
            .unwrap_or(false)
    }
}

fn debug_enabled() -> bool {
    // TODO:
    // if cfg!(debug_assertions) {
    //     return true;
    // }
    if env::args()
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--debug" || arg == "--trace")
    {
        return true;
    }
    if let Ok(log_level) = env::var("MISE_LOG_LEVEL") {
        if log_level == "debug" || log_level == "trace" {
            return true;
        }
    }
    env::var_is_true("MISE_DEBUG") || env::var_is_true("MISE_TRACE")
}

pub fn install() -> eyre::Result<()> {
    if debug_enabled() {
        color_eyre::install()?;
    } else {
        let hook = Hook {};
        eyre::set_hook(Box::new(move |e| Box::new(hook.make_handler(e))))?;
    }
    Ok(())
}

pub struct Hook {}

struct Handler {}

impl Hook {
    fn make_handler(&self, _error: &(dyn std::error::Error + 'static)) -> Handler {
        Handler {}
    }
}

impl EyreHandler for Handler {
    fn debug(
        &self,
        error: &(dyn std::error::Error + 'static),
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if f.alternate() {
            return fmt::Debug::fmt(error, f);
        }

        let mise = ui::style::ered("mise");
        let mut error = Some(error);

        // let mut suggestions = vec![];

        while let Some(e) = error {
            write!(f, "{mise} {e}\n")?;
            error = e.source();
        }

        let msg = ui::style::edim("Run with --verbose or MISE_VERBOSE=1 for more information");
        write!(f, "{mise} {msg}")?;

        Ok(())
    }
}
