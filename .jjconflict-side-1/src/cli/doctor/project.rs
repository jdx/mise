use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::{Result, bail};
use futures_util::{StreamExt, stream};
use serde::Serialize;

use crate::cmd::CmdLineRunner;
use crate::config::doctor::DoctorCheck;
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::toolset::{ResolveOptions, ToolsetBuilder};

/// Run the project's diagnostic checks
///
/// Checks are declared in [doctor.checks.<name>] in mise.toml. Each command runs
/// with the project's installed tools and environment. Checks should inspect
/// state; mise does not sandbox them or run their suggested remedies.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct Project {
    /// Output the complete report as JSON
    #[usage(long, short = 'J')]
    json: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Pass,
    Fail,
    Error,
    Skipped,
}

#[derive(Debug, Serialize)]
struct CheckResult {
    name: String,
    description: Option<String>,
    source: Option<PathBuf>,
    status: Status,
    message: Option<String>,
    hint: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct Report {
    errors: Vec<String>,
    checks: Vec<CheckResult>,
}

impl Project {
    pub(crate) async fn run(self, parent_json: bool) -> Result<()> {
        // SIGINT already cancels the command future in Cli::run. Handle the
        // other normal supervisor shutdown signals here as well, so dropping
        // the bounded runner closes its owned group even under a nested task.
        // Signals the parent chose to ignore (for example SIGHUP under nohup)
        // stay ignored: registering a handler would otherwise replace SIG_IGN.
        #[cfg(unix)]
        let result = {
            let mut terminate = shutdown_signal(nix::libc::SIGTERM)?;
            let mut hangup = shutdown_signal(nix::libc::SIGHUP)?;
            tokio::select! {
                result = self.check() => result,
                _ = recv_or_pending(&mut terminate) => return Err(crate::request_exit(143)),
                _ = recv_or_pending(&mut hangup) => return Err(crate::request_exit(129)),
            }
        };
        #[cfg(not(unix))]
        let result = self.check().await;
        let report = match result {
            Ok(report) => report,
            Err(err) => Report {
                checks: vec![],
                errors: vec![format!("{err:#}")],
            },
        };
        if self.json || parent_json {
            miseprintln!("{}", serde_json::to_string_pretty(&report)?);
        } else if report.checks.is_empty() && report.errors.is_empty() {
            miseprintln!("No project checks configured. Add [doctor.checks.<name>] to mise.toml.");
        } else {
            for error in &report.errors {
                miseprintln!("ERROR {error}");
            }
            for check in &report.checks {
                let status = match check.status {
                    Status::Pass => "PASS",
                    Status::Fail => "FAIL",
                    Status::Error => "ERROR",
                    Status::Skipped => "SKIP",
                };
                miseprintln!(
                    "{status} {}: {}",
                    check.name,
                    check.description.as_deref().unwrap_or(&check.name)
                );
                if let Some(message) = &check.message {
                    miseprintln!("  {message}");
                }
                if matches!(check.status, Status::Fail | Status::Error)
                    && let Some(hint) = &check.hint
                {
                    miseprintln!("  {hint}");
                }
            }
        }
        if !report.errors.is_empty()
            || report
                .checks
                .iter()
                .any(|check| matches!(check.status, Status::Fail | Status::Error))
        {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }

    async fn check(&self) -> Result<Report> {
        let config = Config::get().await?;
        let mut checks = BTreeMap::new();
        // Config files are ordered from highest to lowest precedence. Replace
        // whole named checks so a local command cannot inherit a stale remedy.
        for (path, cf) in config.config_files.iter().rev() {
            let declared = cf.doctor_config().checks;
            if declared.is_empty() {
                continue;
            }
            // Match task conventions: project configuration (including a
            // mise.toml directly in $HOME) anchors at its config root, while
            // global and system configuration use the invocation directory.
            let root = if cf.provenance().scope().is_project() {
                cf.config_root()
            } else {
                std::env::current_dir()?
            };
            for (name, check) in declared {
                checks.insert(name, (path.clone(), root.clone(), check));
            }
        }
        let mut report = Report::default();
        if checks.is_empty() {
            return Ok(report);
        }
        // No tool installation, task dependencies, or task hooks are run here.
        // Preserve every declared check even if building the toolset fails.
        let environment = async {
            if !checks.values().any(|(_, _, check)| check.applies()) {
                return Ok(EnvMap::default());
            }
            Settings::ensure_not_safe("Running project diagnostic commands")?;
            let toolset = ToolsetBuilder::new()
                .with_resolve_options(ResolveOptions {
                    offline: true,
                    ..Default::default()
                })
                .build(&config)
                .await?;
            toolset.full_env(&config).await
        }
        .await;
        report.checks = stream::iter(checks)
            .map(|(name, (source, root, check))| {
                let environment = &environment;
                let config = &config;
                async move {
                    let mut result = CheckResult {
                        name,
                        description: check.description.clone(),
                        source: Some(source),
                        status: Status::Pass,
                        message: None,
                        hint: check.hint.clone(),
                    };
                    if !check.applies() {
                        result.status = Status::Skipped;
                        result.message =
                            Some("Check does not apply to this operating system".into());
                    } else {
                        match environment {
                            Ok(environment) => match run_check(&check, &root, environment).await {
                                Ok(output) => {
                                    if !output.status.success() {
                                        result.status = Status::Fail;
                                        result.message =
                                            Some(format!("Command exited with {}", output.status));
                                    }
                                }
                                Err(err) => {
                                    result.status = Status::Error;
                                    result.message = Some(format!("{err:#}"));
                                }
                            },
                            Err(err) => {
                                result.status = Status::Error;
                                result.message =
                                    Some(format!("Unable to prepare project environment: {err:#}"));
                            }
                        }
                    }
                    result.message = result.message.map(|message| config.redact(&message));
                    result.hint = result.hint.map(|hint| config.redact(&hint));
                    result.description = result
                        .description
                        .map(|description| config.redact(&description));
                    result
                }
            })
            // Unordered so a slow probe does not hold back admission of the
            // remaining checks; the report is sorted by name afterwards.
            .buffer_unordered(crate::jobs::normalize(Settings::get().jobs))
            .collect()
            .await;
        report.checks.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(report)
    }
}

async fn run_check(
    check: &DoctorCheck,
    root: &Path,
    environment: &EnvMap,
) -> Result<std::process::Output> {
    if check.run.trim().is_empty() {
        bail!("Check command must not be empty");
    }
    let timeout = check
        .timeout
        .as_deref()
        .map(crate::duration::parse_duration)
        .transpose()?
        .unwrap_or(Duration::from_secs(10));
    if timeout.is_zero() {
        bail!("Check timeout must be greater than zero");
    }
    let mut shell = match &check.shell {
        Some(shell) => crate::path::split_shell_command(shell)?,
        None => Settings::get().default_inline_shell()?,
    };
    Settings::get().maybe_no_profile(&mut shell);
    let Some((program, args)) = shell
        .split_first()
        .filter(|(program, _)| !program.trim().is_empty())
    else {
        bail!("Check shell must not be empty");
    };
    CmdLineRunner::new(program)
        .cmd_body_args(args, &check.run)
        .current_dir(
            check
                .dir
                .as_ref()
                .map(|dir| root.join(crate::file::replace_path(dir)))
                .unwrap_or_else(|| root.to_path_buf()),
        )
        .env_clear()
        .envs(environment)
        .with_timeout(timeout)
        .output_isolated(64 * 1024)
        .await
}

/// Subscribe to a shutdown signal unless the process inherited it as ignored.
#[cfg(unix)]
fn shutdown_signal(signum: i32) -> Result<Option<tokio::signal::unix::Signal>> {
    use tokio::signal::unix::{SignalKind, signal};
    // SAFETY: a null new action only queries the current disposition; `old`
    // is a valid, zeroed sigaction that the kernel fills in.
    let ignored = unsafe {
        let mut old: nix::libc::sigaction = std::mem::zeroed();
        nix::libc::sigaction(signum, std::ptr::null(), &mut old) == 0
            && old.sa_sigaction == nix::libc::SIG_IGN
    };
    if ignored {
        return Ok(None);
    }
    Ok(Some(signal(SignalKind::from_raw(signum))?))
}

#[cfg(unix)]
async fn recv_or_pending(signal: &mut Option<tokio::signal::unix::Signal>) {
    match signal {
        Some(signal) => {
            signal.recv().await;
        }
        None => std::future::pending().await,
    }
}
