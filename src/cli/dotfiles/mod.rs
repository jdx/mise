use clap::Subcommand;
use eyre::{Result, eyre};
use std::path::Path;

mod add;
mod apply;
mod edit;
mod status;

pub(crate) use apply::DotfilesApply;
pub(crate) use status::DotfilesStatus;

/// [experimental] Manage dotfiles from `[dotfiles]`
///
/// Dotfiles are config files symlinked, copied, or rendered to target paths,
/// plus marker-delimited blocks or single lines in files mise doesn't own.
/// Unlike `[tools]`, dotfiles are only acted on when explicitly requested with
/// `mise dotfiles apply`, `mise bootstrap dotfiles apply`, or `mise bootstrap`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Dotfiles {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Add(add::DotfilesAdd),
    Apply(apply::DotfilesApply),
    Edit(edit::DotfilesEdit),
    Status(status::DotfilesStatus),
}

impl Dotfiles {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Add(cmd) => cmd.run().await,
            Commands::Apply(cmd) => cmd.run().await,
            Commands::Edit(cmd) => cmd.run().await,
            Commands::Status(cmd) => cmd.run().await,
        }
    }
}

fn open_in_editor(file: &Path) -> Result<()> {
    let (program, mut args) = split_editor_command(&crate::env::EDITOR)?;
    args.push(file.as_os_str().into());
    crate::cmd::cmd(&program, args).run()?;
    Ok(())
}

fn split_editor_command(editor: &str) -> Result<(String, Vec<std::ffi::OsString>)> {
    let mut parts = shell_words::split(editor)
        .map_err(|e| eyre!("failed to parse EDITOR/VISUAL value {:?}: {}", editor, e))?
        .into_iter();
    let program = parts
        .next()
        .ok_or_else(|| eyre!("EDITOR/VISUAL is empty"))?;
    Ok((program, parts.map(Into::into).collect()))
}
