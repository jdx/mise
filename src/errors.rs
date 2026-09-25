use std::path::PathBuf;

use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolSource, ToolVersion};
use eyre::Report;
use thiserror::Error;

pub(crate) use mise_util::errors::ProcessError;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("{0}")]
    UnsupportedTarget(String),
    #[error("[{ts}] {tr}: {source:#}")]
    FailedToResolveVersion {
        tr: Box<ToolRequest>,
        ts: ToolSource,
        source: Report,
    },
    #[error("failed to resolve required rolling channel {backend}@{version}")]
    RequiredChannelResolution {
        backend: Box<BackendArg>,
        version: String,
    },
    #[error("{tool}@{version} is not in the lockfile\nhint: {hint}")]
    NotInLockfile {
        tool: String,
        version: String,
        hint: String,
    },
    #[error("[{0}] plugin not installed")]
    PluginNotInstalled(String),
    #[error("{0}@{1} not installed")]
    VersionNotInstalled(Box<BackendArg>, String),
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

/// When the version list could not be fetched, the version being installed
/// was never checked against it, so the install error alone can mislead.
fn version_listing_hint(tr: &ToolRequest) -> String {
    crate::backend::version_listing_failure(tr.ba())
        .map(|cause| {
            format!(
                "\nnote: {}@{} was not checked against its version list, which could not be fetched: {cause}",
                tr.ba().full(),
                tr.version()
            )
        })
        .unwrap_or_default()
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
        // Use {:#} to show full error chain (includes wrapped errors)
        return format!(
            "Failed to install {}@{}: {:#}{}",
            tr.ba().full(),
            tr.version(),
            error,
            version_listing_hint(tr)
        );
    }

    // For multiple failures, show a summary and then each error
    // Sort by tool name for deterministic output (parallel installs complete in arbitrary order)
    let mut sorted_failures: Vec<_> = failed_installations
        .iter()
        .map(|(tr, err)| {
            (
                format!("{}@{}", tr.ba().full(), tr.version()),
                format!("{err:#}{}", version_listing_hint(tr)),
            )
        })
        .collect();
    sorted_failures.sort_by(|a, b| a.0.cmp(&b.0));

    let mut output = vec![];
    let failed_tools: Vec<&str> = sorted_failures
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();

    output.push(format!(
        "Failed to install tools: {}",
        failed_tools.join(", ")
    ));

    // Show detailed errors for each failure (in sorted order)
    // Use {:#} to show full error chain (includes wrapped errors)
    for (name, error) in sorted_failures.iter() {
        output.push(format!("\n{name}: {error}"));
    }

    output.join("\n")
}

/// Split an install result into successful versions and a result preserving any error.
pub(crate) fn split_install_result(
    result: Result<Vec<ToolVersion>, Report>,
) -> (Vec<ToolVersion>, Result<(), Report>) {
    match result {
        Ok(versions) => (versions, Ok(())),
        Err(err) => {
            let versions = match err.downcast_ref::<Error>() {
                Some(Error::InstallFailed {
                    successful_installations,
                    ..
                }) => successful_installations.clone(),
                _ => vec![],
            };
            (versions, Err(err))
        }
    }
}

impl Error {
    pub(crate) fn get_exit_status(err: &Report) -> Option<i32> {
        ProcessError::get_exit_status(err)
    }

    /// See [`ProcessError::is_killed_by_signal`].
    pub(crate) fn is_killed_by_signal(err: &Report) -> bool {
        ProcessError::is_killed_by_signal(err)
    }

    pub(crate) fn is_sigint(err: &Report) -> bool {
        ProcessError::is_sigint(err)
    }

    pub(crate) fn is_task_interrupted_before_start(err: &Report) -> bool {
        ProcessError::is_task_interrupted_before_start(err)
    }

    pub(crate) fn is_argument_err(err: &Report) -> bool {
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

    pub(crate) fn is_required_channel_resolution_err(err: &Report) -> bool {
        err.chain().any(|source| {
            matches!(
                source.downcast_ref::<Error>(),
                Some(Error::RequiredChannelResolution { .. })
            )
        })
    }

    pub(crate) fn is_not_in_lockfile(err: &Report) -> bool {
        err.chain().any(|source| {
            matches!(
                source.downcast_ref::<Error>(),
                Some(Error::NotInLockfile { .. })
            )
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn detects_not_in_lockfile() {
        let err = Report::new(Error::NotInLockfile {
            tool: "usage".into(),
            version: "latest".into(),
            hint: "Run `mise install` without --locked to update the lockfile".into(),
        });

        assert!(Error::is_not_in_lockfile(&err));
        assert!(!Error::is_required_channel_resolution_err(&err));
    }
}
