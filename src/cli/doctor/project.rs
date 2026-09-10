use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eyre::{Result, bail};
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
    checks: Vec<CheckResult>,
}

impl Project {
    pub(crate) async fn run(self, parent_json: bool) -> Result<()> {
        // SIGINT already cancels the command future in Cli::run. Handle the
        // other normal supervisor shutdown signals here as well, so dropping
        // the bounded runner closes its owned group even under a nested task.
        #[cfg(unix)]
        let result = {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate = signal(SignalKind::terminate())?;
            let mut hangup = signal(SignalKind::hangup())?;
            tokio::select! {
                result = self.check() => result,
                _ = terminate.recv() => return Err(crate::request_exit(143)),
                _ = hangup.recv() => return Err(crate::request_exit(129)),
            }
        };
        #[cfg(not(unix))]
        let result = self.check().await;
        let report = match result {
            Ok(report) => report,
            Err(err) => Report {
                checks: vec![CheckResult {
                    name: "configuration".into(),
                    description: Some("Load the project's configuration and environment".into()),
                    source: None,
                    status: Status::Error,
                    message: Some(format!("{err:#}")),
                    hint: None,
                }],
            },
        };
        if self.json || parent_json {
            miseprintln!("{}", serde_json::to_string_pretty(&report)?);
        } else if report.checks.is_empty() {
            miseprintln!("No project checks configured. Add [doctor.checks.<name>] to mise.toml.");
        } else {
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
        if report
            .checks
            .iter()
            .any(|check| matches!(check.status, Status::Fail | Status::Error))
        {
            return Err(crate::request_exit(1));
        }
        Ok(())
    }

    async fn check(&self) -> Result<Report> {
        if Settings::safe_mode() {
            bail!("Project diagnostic commands cannot run in safe mode");
        }
        let config = Config::get().await?;
        let mut checks = BTreeMap::new();
        // Config files are ordered from highest to lowest precedence. Replace
        // whole named checks so a local command cannot inherit a stale remedy.
        for (path, cf) in config.config_files.iter().rev() {
            for (name, check) in cf.doctor_config().checks {
                checks.insert(name, (path.clone(), cf.config_root(), check));
            }
        }
        let mut report = Report::default();
        if checks.is_empty() {
            return Ok(report);
        }
        let toolset = ToolsetBuilder::new()
            .with_resolve_options(ResolveOptions {
                offline: true,
                ..Default::default()
            })
            .build(&config)
            .await?;
        // No tool installation, task dependencies, or task hooks are run here.
        let environment = toolset.full_env(&config).await;
        for (name, (source, root, check)) in checks {
            let mut result = CheckResult {
                name,
                description: check.description.clone(),
                source: Some(source),
                status: Status::Pass,
                message: None,
                hint: check.hint.clone(),
            };
            if check
                .os
                .as_ref()
                .is_some_and(|os| !os.iter().any(|os| os.as_ref() == std::env::consts::OS))
            {
                result.status = Status::Skipped;
                result.message = Some("Check does not apply to this operating system".into());
            } else {
                match &environment {
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
            report.checks.push(result);
        }
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
    let shell = match &check.shell {
        Some(shell) => shell.clone(),
        None => Settings::get().default_inline_shell()?,
    };
    let Some((program, args)) = shell.split_first() else {
        bail!("Check shell must not be empty");
    };
    CmdLineRunner::new(program)
        .cmd_body_args(args, &check.run)
        .current_dir(
            check
                .dir
                .as_ref()
                .map(|dir| root.join(dir))
                .unwrap_or_else(|| root.to_path_buf()),
        )
        .env_clear()
        .envs(environment)
        .with_timeout(timeout)
        .output_isolated(64 * 1024)
        .await
}
