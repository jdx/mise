use eyre::Result;
use std::path::Path;

mod add;
mod apply;
mod diff;
mod edit;
mod status;
mod unapply;

pub(crate) use add::DotfilesAdd;
pub(crate) use apply::DotfilesApply;
pub(crate) use diff::DotfilesDiff;
pub(crate) use edit::DotfilesEdit;
pub(crate) use status::DotfilesStatus;
pub(crate) use unapply::DotfilesUnapply;

/// Load, validate, and filter whole-file and edit requests with the same
/// target semantics for every command that acts on both kinds of entry.
fn select_requests(
    config: &crate::config::Config,
    targets: &[String],
) -> Result<(
    Vec<crate::system::files::FileRequest>,
    Vec<crate::system::edits::EditRequest>,
)> {
    let all_files = crate::system::files::files_from_config(config)?;
    crate::system::files::validate_composed_file_footprints(&all_files)?;
    let files = all_files
        .iter()
        .filter(|req| crate::system::files::matches_target(&req.target, &req.target_raw, targets))
        .cloned()
        .collect::<Vec<_>>();
    let all_edits = crate::system::edits::edits_from_config(config)?;
    let edits = all_edits
        .iter()
        .filter(|req| crate::system::edits::matches_target(req, targets))
        .cloned()
        .collect::<Vec<_>>();
    if files.is_empty()
        && edits.is_empty()
        && !targets.is_empty()
        && (!all_files.is_empty() || !all_edits.is_empty())
    {
        eyre::bail!("no dotfiles matched target filter: {}", targets.join(", "));
    }
    Ok((files, edits))
}

/// Manage dotfiles from `[dotfiles]` (deprecated)
///
/// Use `mise bootstrap dotfiles` instead.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, hide = true)]
pub(crate) struct Dotfiles {
    #[usage(subcommand)]
    command: Commands,
}

#[derive(Debug, usage_rs::Subcommands)]
enum Commands {
    #[usage(hide = true)]
    Add(add::DotfilesAdd),
    #[usage(hide = true)]
    Apply(apply::DotfilesApply),
    #[usage(hide = true)]
    Diff(diff::DotfilesDiff),
    #[usage(hide = true)]
    Edit(edit::DotfilesEdit),
    #[usage(hide = true)]
    Status(status::DotfilesStatus),
    #[usage(hide = true)]
    Unapply(unapply::DotfilesUnapply),
}

impl Dotfiles {
    pub(crate) async fn run(self) -> Result<()> {
        deprecated_at!(
            "2027.2.0",
            "2028.2.0",
            "cli.dotfiles",
            "`mise dotfiles ...` is deprecated. Use `mise bootstrap dotfiles ...` instead."
        );
        match self.command {
            Commands::Add(cmd) => cmd.run().await,
            Commands::Apply(cmd) => cmd.run().await.map(|_| ()),
            Commands::Diff(cmd) => cmd.run().await,
            Commands::Edit(cmd) => cmd.run().await,
            Commands::Status(cmd) => cmd.run().await,
            Commands::Unapply(cmd) => cmd.run().await,
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
