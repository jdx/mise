use std::process::ExitStatus;

use thiserror::Error;

use crate::plugins::PluginName;

#[derive(Error, Debug)]
pub enum Error {
    #[error("[{0}] plugin not installed")]
    PluginNotInstalled(PluginName),
    #[error("{0}@{1} not installed")]
    VersionNotInstalled(PluginName, String),
    #[error("{0}@{1} not found")]
    VersionNotFound(PluginName, String),
    #[error("[{}] script exited with non-zero status: {}", .0, render_exit_status(.1))]
    ScriptFailed(PluginName, Option<ExitStatus>),
}

fn render_exit_status(exit_status: &Option<ExitStatus>) -> String {
    match exit_status.and_then(|s| s.code()) {
        Some(exit_status) => format!("exit code {exit_status}"),
        None => "no exit status".into(),
    }
}
