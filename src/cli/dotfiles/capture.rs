use std::process::{Command, ExitStatus};

use eyre::{Result, WrapErr};

use crate::system::history::OperationScope;
use crate::system::history::store::{OperationKind, Summary};

/// Record tracked files before and after an external command
///
/// Runs the command directly, inheriting its terminal and environment. Keeps
/// a linked checkpoint pair, including when the command fails or changes no
/// files. Capture failures warn and never replace the command's exit status.
/// Only tracked files are recorded: package, service, and other system effects
/// are not reversible. Concurrent editor changes are part of the same interval.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesCapture {
    /// Describe this operation in history
    #[usage(long, value_name = "LABEL")]
    label: Option<String>,

    /// Command and arguments to run, after --
    #[usage(double_dash = "required", required = true)]
    command: Vec<String>,
}

impl DotfilesCapture {
    pub(crate) async fn run(self) -> Result<()> {
        let (program, args) = self
            .command
            .split_first()
            .ok_or_else(|| eyre::eyre!("provide a command after --"))?;
        let scope = match OperationScope::begin_kind(
            OperationKind::Capture,
            "bootstrap dotfiles capture",
            false,
        )
        .await
        {
            Ok(scope) => Some(scope),
            Err(err) => {
                warn!("history: cannot capture this command: {err:#}");
                None
            }
        };
        if let Some(scope) = &scope {
            scope.prepare_capture(self.label.as_deref());
            if scope.before().is_none() {
                warn!("history: no protective checkpoint is available for this command");
            }
        }
        let result = Command::new(program)
            .args(args)
            .status()
            .wrap_err_with(|| format!("could not run {program}"));
        if let Some(scope) = scope {
            scope.refresh_tracked().await;
            scope.prepare_capture(self.label.as_deref());
            let error = match &result {
                Ok(status) if status.success() => None,
                Ok(status) => Some(format!("command exited with {status}")),
                Err(err) => Some(format!("{err:#}")),
            };
            scope.finish(
                error,
                Some(Summary {
                    message: self.label,
                }),
            );
        }
        let status = result?;
        if status.success() {
            Ok(())
        } else {
            Err(crate::request_exit(exit_code(status)))
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}
