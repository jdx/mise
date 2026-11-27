use std::path::{Path, PathBuf};

use crate::args::BackendArg;
use crate::toolset::{ToolRequest, ToolSource, ToolVersion};
use crate::ui::style::display_path_escaped;
use eyre::Report;
use thiserror::Error;

pub(crate) use mise_util::errors::ProcessError;

#[derive(Debug, Error)]
pub enum Error {
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
    /// Every release matching a request is newer than `minimum_release_age`.
    #[error("{0}")]
    ReleaseAgeHidAllVersions(String),
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
    #[error("{}", .1.message(.0))]
    UntrustedConfig(PathBuf, Untrusted),
    #[error(
        "The environment file or script {} is not trusted, and paranoid mode ties trust to a file's contents.\nReview it, then trust it with `mise trust` or `mise trust {}`. See https://mise.jdx.dev/paranoid.html#environment-files for more information.",
        display_path_escaped(.0),
        display_path_escaped(.0)
    )]
    UntrustedEnvFile(PathBuf),
    /// A parent stopped waiting for its nested tasks because another task failed.
    #[error("task sequence aborted due to failure")]
    TaskSequenceAborted,
    #[error("{}", format_install_failures(.failed_installations))]
    InstallFailed {
        successful_installations: Vec<ToolVersion>,
        failed_installations: Vec<(ToolRequest, Report)>,
    },
}

/// Why a config file is refused, carried by [`Error::UntrustedConfig`] so the places that skip an
/// untrusted config can say what to do about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Untrusted {
    /// Nobody has trusted it, and `mise trust` would.
    #[default]
    NotTrusted,
    /// Paranoid mode reaches it through a symlink inside this project. Trust cannot help: paranoid
    /// mode approves a file by its content, and a link inside a project is that project's to
    /// change.
    LinkedInProject(PathBuf),
}

impl Untrusted {
    /// The error for the config at `path`.
    pub fn message(&self, path: &Path) -> String {
        match self {
            Untrusted::NotTrusted => format!(
                "Config files in {} are not trusted.\nTrust them with `mise trust`. See https://mise.jdx.dev/cli/trust.html for more information.",
                display_path_escaped(path)
            ),
            Untrusted::LinkedInProject(project) => format!(
                "{} is reached through a symlink inside {}, and paranoid mode cannot approve it: \
                 it approves a file by its content, and a link inside a project is that project's \
                 to change.\n{}",
                display_path_escaped(path),
                display_path_escaped(project),
                self.remedy(path)
            ),
        }
    }

    /// What the user can do to load the config at `path`.
    pub fn remedy(&self, path: &Path) -> String {
        match self {
            Untrusted::NotTrusted => "run `mise trust` to enable it".to_string(),
            Untrusted::LinkedInProject(project) => {
                let link = outermost_link(path, project);
                let copy = if !link.is_dir() {
                    "the file"
                } else {
                    "the directory it leads to"
                };
                format!(
                    "Replace the link {} with a copy of {copy} and run `mise trust`, or add {} to `trusted_config_paths`.",
                    display_path_escaped(&link),
                    display_path_escaped(project)
                )
            }
        }
    }
}

/// The outermost symlink between `project` and `path`, `path` included, or `path` when there is
/// none. That is the link to replace: replacing a file below a linked `.config` would leave the
/// directory linked. Both paths are made absolute without following links, so a relative path
/// from `mise trust .` still finds a linked directory.
fn outermost_link(path: &Path, project: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let project = std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf());
    path.ancestors()
        .take_while(|p| *p != project && p.starts_with(&project))
        .filter(|p| p.is_symlink())
        .last()
        .unwrap_or(&path)
        .to_path_buf()
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
pub fn split_install_result(
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
    pub fn get_exit_status(err: &Report) -> Option<i32> {
        ProcessError::get_exit_status(err)
    }

    /// See [`ProcessError::is_killed_by_signal`].
    pub fn is_killed_by_signal(err: &Report) -> bool {
        ProcessError::is_killed_by_signal(err)
    }

    pub fn is_sigint(err: &Report) -> bool {
        ProcessError::is_sigint(err)
    }

    pub fn is_task_interrupted_before_start(err: &Report) -> bool {
        ProcessError::is_task_interrupted_before_start(err)
    }

    pub fn is_task_sequence_aborted(err: &Report) -> bool {
        matches!(
            err.downcast_ref::<Error>(),
            Some(Error::TaskSequenceAborted)
        )
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

    pub fn is_release_age_hid_all_versions(err: &Report) -> bool {
        err.chain().any(|source| {
            matches!(
                source.downcast_ref::<Error>(),
                Some(Error::ReleaseAgeHidAllVersions(_))
            )
        })
    }

    pub fn is_not_in_lockfile(err: &Report) -> bool {
        err.chain().any(|source| {
            matches!(
                source.downcast_ref::<Error>(),
                Some(Error::NotInLockfile { .. })
            )
        })
    }
}

#[cfg(test)]
mod docs_reference;

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

    #[test]
    fn linked_remedy_names_the_outermost_link() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = tmp.path().join("shared");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(shared.join("mise")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(shared.join("mise/config.toml"), "").unwrap();
        std::os::unix::fs::symlink(&shared, project.join(".config")).unwrap();
        std::os::unix::fs::symlink(shared.join("mise/config.toml"), project.join("mise.toml"))
            .unwrap();
        let why = Untrusted::LinkedInProject(project.clone());

        let in_dir = project.join(".config/mise/config.toml");
        assert_eq!(outermost_link(&in_dir, &project), project.join(".config"));
        assert!(why.remedy(&in_dir).contains("the directory it leads to"));

        let file = project.join("mise.toml");
        assert_eq!(outermost_link(&file, &project), file);
        assert!(why.remedy(&file).contains("a copy of the file"));
    }
}
