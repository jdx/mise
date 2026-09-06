use eyre::Result;

use crate::system::history::watch::runtime::{self, WatchOptions};

/// Save tracked files as they change
///
/// Runs in the foreground: installs filesystem watches for every autosaved
/// tracked entry, saves a checkpoint once a changed file has been quiet for
/// `history.watch.debounce` (a file that keeps changing never delays the
/// others; `history.watch.max_interval` saves it regardless), and
/// reconciles the whole set at startup, every `history.watch.reconcile`,
/// and when the configuration changes. Manual-save entries are never
/// watched.
///
/// The `history-watch` built-in service runs this for you:
///
///     [bootstrap.services.mise-history]
///     builtin = "history-watch"
///
/// Exit codes: 0 when history is disabled or another watcher already runs;
/// 1 when git is unusable, the store cannot open, or no watch can be
/// installed. A capture that fails is retried with backoff and never drops
/// the pending changes; one that would overlap another history operation
/// is deferred.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesWatch {
    /// Reconcile once and exit (for timers and cron)
    #[usage(long)]
    once: bool,

    /// One JSON object per line instead of log lines
    #[usage(long, short = 'J')]
    json: bool,
}

impl DotfilesWatch {
    pub(crate) async fn run(self) -> Result<()> {
        crate::config::Settings::get().ensure_experimental("dotfile tracking")?;
        let code = runtime::run(WatchOptions {
            once: self.once,
            json: self.json,
        })
        .await?;
        if code != 0 {
            return Err(crate::request_exit(code));
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles watch</bold>
    $ <bold>mise bootstrap dotfiles watch --once</bold>      # one reconcile, for a timer
    $ <bold>mise bootstrap dotfiles watch --json</bold>
"#
);
