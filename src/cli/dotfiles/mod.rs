use clap::Subcommand;
use eyre::{Result, eyre};
use std::path::Path;

mod add;
mod apply;
mod edit;
mod status;

pub(crate) use apply::DotfilesApply;
pub(crate) use status::DotfilesStatus;

/// Manage dotfiles from `[dotfiles]`
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

/// Config files mise skipped because they're untrusted (declining the trust
/// prompt adds them to the ignore list) but that do declare `[dotfiles]`.
/// Their entries never reach these commands, so "nothing configured" reads as
/// a config mistake when the real answer is that the file wasn't loaded.
///
/// Reading and parsing the TOML here is inert — nothing is templated or
/// executed, we only look for the table's presence.
pub(crate) fn ignored_configs_with_dotfiles() -> Vec<&'static Path> {
    crate::config::IGNORED_CONFIG_FILES
        .iter()
        .filter(|path| {
            crate::file::read_to_string(path)
                .ok()
                .and_then(|body| body.parse::<toml::Table>().ok())
                .is_some_and(|table| table.contains_key("dotfiles"))
        })
        .map(|path| path.as_path())
        .collect()
}

/// Explain the empty `[dotfiles]` when it's really an untrusted config.
pub(crate) fn warn_if_dotfiles_ignored() {
    let ignored = ignored_configs_with_dotfiles();
    if ignored.is_empty() {
        return;
    }
    warn!(
        "[dotfiles] in these config files was skipped because they are not trusted:\n{}\nRun `mise trust` in that directory to use them.",
        ignored
            .iter()
            .map(|p| format!("  {}", crate::file::display_path(p)))
            .collect::<Vec<_>>()
            .join("\n")
    );
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
