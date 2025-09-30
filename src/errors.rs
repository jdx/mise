use std::path::PathBuf;
use std::process::ExitStatus;

use crate::cli::args::BackendArg;
use crate::env::RUST_BACKTRACE;
use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolSource, ToolVersion};
use eyre::Report;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("[{ts}] {tr}: {source:#}")]
    FailedToResolveVersion {
        tr: Box<ToolRequest>,
        ts: ToolSource,
        source: Report,
    },
    #[error("[{0}] plugin not installed")]
    PluginNotInstalled(String),
    #[error("{0}@{1} not installed")]
    VersionNotInstalled(Box<BackendArg>, String),
    #[error("{} exited with non-zero status: {}", .0, render_exit_status(.1))]
    ScriptFailed(String, Option<ExitStatus>),
    #[error(
        "Config files in {} are not trusted.\nTrust them with `mise trust`. See https://mise.jdx.dev/cli/trust.html for more information.",
        display_path(.0)
    )]
    UntrustedConfig(PathBuf),
    #[error("{}", format_install_failures(.failed_installations))]
    InstallFailed {
        successful_installations: Vec<ToolVersion>,
        failed_installations: Vec<(ToolRequest, Report)>,
    },
}

fn render_exit_status(exit_status: &Option<ExitStatus>) -> String {
    match exit_status.and_then(|s| s.code()) {
        Some(exit_status) => format!("exit code {exit_status}"),
        None => "no exit status".into(),
    }
}

fn format_install_failures(failed_installations: &[(ToolRequest, Report)]) -> String {
    if failed_installations.is_empty() {
        return "Installation failed".to_string();
    }

    // For a single failure, show the underlying error directly to preserve
    // the original error location for better debugging
    if failed_installations.len() == 1 {
        let (tr, error) = &failed_installations[0];
        // Show the underlying error with the tool context
        return format!(
            "Failed to install {}@{}: {}",
            tr.ba().full(),
            tr.version(),
            if *RUST_BACKTRACE {
                format!("{error}")
            } else {
                format!("{error:?}")
            }
        );
    }

    // For multiple failures, show a summary and then each error
    let mut output = vec![];
    let failed_tools: Vec<String> = failed_installations
        .iter()
        .map(|(tr, _)| format!("{}@{}", tr.ba().full(), tr.version()))
        .collect();

    output.push(format!(
        "Failed to install tools: {}",
        failed_tools.join(", ")
    ));

    // Show detailed errors for each failure
    for (tr, error) in failed_installations.iter() {
        let error_str = if *RUST_BACKTRACE {
            format!("{error}")
        } else {
            format!("{error:?}")
        };
        output.push(format!(
            "\n{}@{}: {error_str}",
            tr.ba().full(),
            tr.version()
        ));
    }

    output.join("\n")
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
