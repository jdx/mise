use eyre::{Result, bail};

use crate::system::history::sync::SyncMode;
use crate::system::history::sync::origin;

/// Connect or disconnect the setup repository
///
/// `set <url>` connects the ordinary tracked-file repository to an origin.
/// All committed history becomes eligible for synchronization, including
/// intermediate commits made before connecting. Preview the sync mode and
/// tracked paths before confirming. Encrypted-file policy is checked across
/// every reachable commit; unrelated histories are never replaced.
/// The connection is written to machine-local `[history.origin]` configuration;
/// the mode is `settings.history.sync`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesOrigin {
    #[usage(subcommand)]
    command: Option<DotfilesOriginCommands>,

    /// Disconnect: remove `[history.origin]` (local checkpoints and fetched refs stay)
    #[usage(long, effect = "destructive")]
    remove: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum DotfilesOriginCommands {
    Set(DotfilesOriginSet),
}

/// Connect a setup repository
#[derive(Debug, usage_rs::Args)]
pub(crate) struct DotfilesOriginSet {
    /// The repository url (any git url; a private repository is recommended)
    url: String,

    /// The setup branch (default: the repository's own default branch)
    #[usage(long, value_name = "BRANCH")]
    branch: Option<String>,

    /// How the repository is used: sync, fetch-only, or manual
    ///
    /// Prompts when omitted. With --yes, accepts the configured mode (default: sync),
    /// including automatic publication and incoming writes. Use --sync manual
    /// to keep automatic local history without automatic network activity.
    #[usage(long, value_name = "MODE")]
    sync: Option<String>,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

impl DotfilesOrigin {
    pub(crate) async fn run(self) -> Result<()> {
        match self.command {
            Some(DotfilesOriginCommands::Set(cmd)) => cmd.run().await,
            None if self.remove => origin::remove(),
            None => {
                match crate::system::history::config::origin()? {
                    Some((path, origin)) => {
                        miseprintln!(
                            "{} (branch {}) declared in {}; mode {}",
                            origin.url,
                            origin.branch,
                            crate::file::display_path(&path),
                            SyncMode::current()?.as_str()
                        );
                    }
                    None => miseprintln!(
                        "no setup repository is connected; `mise bootstrap dotfiles origin set <url>` connects one"
                    ),
                }
                Ok(())
            }
        }
    }
}

impl DotfilesOriginSet {
    async fn run(self) -> Result<()> {
        if !crate::config::Settings::get().history.enabled {
            bail!("history is disabled (history.enabled = false)");
        }
        let mode = match self.sync.as_deref() {
            Some(mode) => SyncMode::parse(mode)?,
            None if self.yes || crate::config::Settings::get().yes => SyncMode::current()?,
            None => match crate::ui::prompt::confirm_with_default(
                "Automatically publish saved edits AND apply incoming changes to live files? Choose no for manual sharing; local autosave continues in either mode.",
                false,
            )? {
                crate::ui::prompt::Confirmation::Yes => SyncMode::Sync,
                crate::ui::prompt::Confirmation::No => SyncMode::Manual,
                crate::ui::prompt::Confirmation::Unavailable => {
                    bail!(
                        "not connected: choose --sync manual, --sync sync, or --sync fetch-only to connect without a mode prompt"
                    );
                }
            },
        };
        let (store, tracked, _) = super::history::open().await?;
        if let Some(reason) = store.unavailable() {
            bail!("cannot connect a setup repository: {reason}");
        }
        origin::set(
            &store,
            &tracked,
            &origin::SetOptions {
                url: self.url.clone(),
                branch: self.branch.clone(),
                mode,
                yes: self.yes,
            },
        )
        .await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles origin set https://github.com/you/setup.git</bold>
    $ <bold>mise bootstrap dotfiles origin set git@github.com:you/setup.git --sync manual</bold>
    $ <bold>mise bootstrap dotfiles origin</bold>              # what is connected
    $ <bold>mise bootstrap dotfiles origin --remove</bold>
"#
);
