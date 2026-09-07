use eyre::{Result, bail};

use crate::system::history::sync::SyncMode;
use crate::system::history::sync::origin;

/// Connect, disconnect, or purge the setup repository
///
///
/// `set <url>` connects one repository that holds the shared setup branch
/// and this machine's recovery refs. Before anything leaves the machine it
/// prints exactly what will happen: the sync mode, what is shared per
/// stream, what is not and why, what is backed up (in plain form, or
/// encrypted with `--encrypt-backups`), names that look like secrets,
/// private content already committed upstream, and how an existing
/// repository would be adopted. The declaration goes to `[history.origin]`
/// in the global config; the mode to `settings.history.sync`.
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

    /// How the repository is used: sync, fetch-only, or manual
    ///
    /// Prompts when omitted. With --yes, accepts the configured mode (default: sync),
    /// including automatic publication and incoming writes. Use --sync manual
    /// to keep automatic local history without automatic network activity.
    #[usage(long, value_name = "MODE")]
    sync: Option<String>,

    /// Upload the checkpoints recorded before now too
    #[usage(long)]
    include_existing: bool,

    /// Continue although the repository's history holds private content
    #[usage(long)]
    allow_committed_private: bool,

    /// Encrypt this machine's recovery refs with age for its recipients (see --recipient)
    ///
    /// Every backed-up checkpoint becomes one age payload: file names,
    /// descriptions, and content are readable only with a recipient's
    /// identity. Setup configuration stays plaintext; dotfiles can use `encrypt = true`.
    #[usage(long)]
    encrypt_backups: bool,

    /// A backup recipient (repeatable): an age public key (`age1…`), an SSH public key (`ssh-ed25519 …`), or a path to a `.pub` file or an age identity file
    ///
    /// Default: this machine's own age key (`~/.config/mise/age.txt` or
    /// `settings.age.key_file`) and `~/.ssh/id_ed25519.pub` / `id_rsa.pub`.
    /// When none exists, an age identity is generated at the key file path.
    #[usage(long, value_name = "RECIPIENT", requires = "encrypt_backups")]
    recipient: Vec<String>,

    /// Skip the confirmation prompt
    #[usage(long, short)]
    yes: bool,
}

impl DotfilesOrigin {
    pub(crate) async fn run(self) -> Result<()> {
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
                            "{} (branch {}) declared in {}; mode {}; machine backups {}",
                            origin.url,
                            origin.branch,
                            crate::file::display_path(&path),
                            SyncMode::current()?.as_str(),
                            if origin.encrypt_backups {
                                format!(
                                    "encrypted (age) for {} recipient(s)",
                                    origin.recipients.len()
                                )
                            } else {
                                "plaintext".to_string()
                            }
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
                name: self.name.clone(),
                mode,
                include_existing: self.include_existing,
                allow_committed_private: self.allow_committed_private,
                encrypt_backups: self.encrypt_backups,
                recipients: self.recipient.clone(),
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
    $ <bold>mise bootstrap dotfiles origin set git@github.com:you/setup.git --encrypt-backups</bold>
    $ <bold>mise bootstrap dotfiles origin set git@github.com:you/setup.git --encrypt-backups --recipient age1… --recipient ~/.ssh/id_ed25519.pub</bold>
    $ <bold>mise bootstrap dotfiles origin</bold>              # what is connected
    $ <bold>mise bootstrap dotfiles origin --remove</bold>
"#
);
