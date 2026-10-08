use eyre::{Result, bail};

use crate::system::history::sync::SyncMode;
use crate::system::history::sync::origin;

/// Show, connect, or disconnect the origin repository
///
/// The origin is a Git repository that shares the history of your tracked
/// files with your other machines. With no subcommand, prints the connected
/// URL, its branch, the config file that declares it, and the sync mode (the
/// `history.sync` setting). `--remove` disconnects; local checkpoints and
/// fetched history stay.
///
/// See https://mise.jdx.dev/dotfiles/sync.html
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot origin", help = "Show the connected repository"),
    example(
        "mise dot origin set git@github.com:you/dotfiles.git",
        help = "Connect a repository"
    ),
    example("mise dot origin --remove", help = "Disconnect the repository")
)]
pub(crate) struct DotfilesOrigin {
    #[usage(subcommand)]
    command: Option<DotfilesOriginCommands>,

    /// Disconnect the origin by removing `[history.origin]`; local checkpoints and
    /// fetched history stay
    #[usage(long, effect = "destructive")]
    remove: bool,
}

#[derive(Debug, usage_rs::Subcommands)]
enum DotfilesOriginCommands {
    Set(DotfilesOriginSet),
}

/// Connect an origin repository
///
/// Use a private repository: every saved checkpoint is shared, including those
/// saved before you connect. mise shows the sync mode and the tracked files
/// before you confirm, then runs the first sync. It never replaces a repository
/// that has unrelated history, and it refuses to push older plaintext versions
/// of files you now encrypt. The connection is written to `[history.origin]` in
/// `config.local.toml` next to your global config, so it stays on this machine.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise dot origin set https://github.com/you/dotfiles.git",
        help = "Connect and choose a sync mode at the prompt"
    ),
    example(
        "mise dot origin set git@github.com:you/dotfiles.git --sync manual",
        help = "Connect without automatic network activity"
    )
)]
pub(crate) struct DotfilesOriginSet {
    /// The repository URL (any Git URL; use a private repository)
    url: String,

    /// The branch to sync (default: the repository's default branch)
    ///
    /// Reconnecting a repository this machine already follows keeps that
    /// connection's branch. A repository with no branches gets `main`, which the
    /// first push creates.
    #[usage(long, value_name = "BRANCH")]
    branch: Option<String>,

    /// How to sync with the repository: sync, fetch-only, or manual
    ///
    /// `sync` pushes saved checkpoints and applies incoming changes
    /// automatically. `fetch-only` fetches automatically and never pushes.
    /// `manual` makes no automatic network calls: `mise dot sync` pushes and
    /// fetches, and `mise dot pull` applies, only when you run them; local
    /// history is still saved automatically. When omitted, mise prompts; with
    /// `--yes`, it uses the `history.sync` setting.
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
                        "no setup repository is connected; `mise dot origin set <url>` connects one"
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
                crate::ui::prompt::Confirmation::Unanswered => {
                    bail!(
                        "not connected: stdin ended before an answer; choose --sync manual, --sync sync, or --sync fetch-only"
                    );
                }
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
            },
        )
        .await
    }
}
