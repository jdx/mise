use eyre::{Result, bail};

use crate::system::history::sync::SyncMode;
use crate::system::history::sync::origin;

/// Connect, disconnect, or purge the setup repository
///
/// Experimental: enable with `mise settings experimental=true`.
///
/// `set <url>` connects one repository that holds the shared setup branch
/// and this machine's recovery refs. Before anything leaves the machine it
/// prints exactly what will happen: the sync mode, what is shared per
/// stream, what is not and why, what is backed up (in plain form), names
/// that look like secrets, private content already committed upstream,
/// and how an existing repository would be adopted. The declaration goes
/// to `[history.origin]` in the global config; the mode to
/// `settings.history.sync`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesOrigin {
    #[usage(subcommand)]
    command: Option<DotfilesOriginCommands>,

    /// Disconnect: remove `[history.origin]` (local checkpoints and fetched refs stay)
    #[usage(long, effect = "destructive")]
    remove: bool,

    /// Delete this machine's recovery refs from the origin, then disconnect
    #[usage(long, effect = "destructive")]
    purge: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
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

    /// The setup branch
    #[usage(long, value_name = "BRANCH", default = "main")]
    branch: String,

    /// This machine's name in the repository (default: the hostname)
    #[usage(long, value_name = "NAME")]
    name: Option<String>,

    /// How the repository is used: sync (default), fetch-only, or manual
    #[usage(long, value_name = "MODE", default = "sync")]
    sync: String,

    /// Upload the checkpoints recorded before now too
    #[usage(long)]
    include_existing: bool,

    /// Continue although the repository's history holds private content
    #[usage(long)]
    allow_committed_private: bool,

    /// Encrypt machine recovery refs (not supported yet)
    #[usage(long)]
    encrypt_backups: bool,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

impl DotfilesOrigin {
    pub(crate) async fn run(self) -> Result<()> {
        if self.command.is_some() || self.remove || self.purge {
            crate::config::Settings::get().ensure_experimental("dotfile tracking")?;
        }
        match self.command {
            Some(DotfilesOriginCommands::Set(cmd)) => cmd.run().await,
            None if self.purge => {
                let (store, _, _) = super::history::open().await?;
                origin::purge(&store, self.yes)
            }
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
        let mode = SyncMode::parse(&self.sync)?;
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
                name: self.name.clone(),
                mode,
                include_existing: self.include_existing,
                allow_committed_private: self.allow_committed_private,
                encrypt_backups: self.encrypt_backups,
                yes: self.yes,
            },
        )
        .await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles origin set https://github.com/you/setup.git</bold>
    $ <bold>mise bootstrap dotfiles origin set git@github.com:you/setup.git --name laptop --sync manual</bold>
    $ <bold>mise bootstrap dotfiles origin</bold>              # what is connected
    $ <bold>mise bootstrap dotfiles origin --remove</bold>
"#
);
