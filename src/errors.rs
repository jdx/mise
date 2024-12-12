use std::path::PathBuf;
use std::process::ExitStatus;

use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolSource};
use eyre::Report;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("[{ts}] {tr}: {source:#}")]
    FailedToResolveVersion {
        tr: ToolRequest,
        ts: ToolSource,
        source: Report,
    },
    #[error("[{0}] plugin not installed")]
    PluginNotInstalled(String),
    #[error("{0}@{1} not installed")]
    VersionNotInstalled(BackendArg, String),
    #[error("{} exited with non-zero status: {}", .0, render_exit_status(.1))]
    ScriptFailed(String, Option<ExitStatus>),
    #[error(
        "Config files in {} are not trusted.\nTrust them with `mise trust`.",
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
    pub fn get_exit_status(err: &Report) -> Option<i32> {
        if let Some(Error::ScriptFailed(_, Some(status))) = err.downcast_ref::<Error>() {
            status.code()
        } else {
            None
        }
    }

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
