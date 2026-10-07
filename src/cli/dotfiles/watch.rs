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
/// With a connected setup repository the watcher also synchronizes per
/// `settings.history.sync`: in `sync` mode it publishes within
/// `history.sync_interval` after a save, fetches every
/// `history.fetch_interval`, and applies incoming changes once the complete setup is conflict-free;
/// in `fetch-only` mode it only fetches; in `manual` mode it does nothing
/// on the network. A failed sync backs off and is retried while saving
/// continues. `--once` runs one reconcile and one such synchronization.
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
    /// Reconcile and synchronize once and exit (for timers and cron)
    #[usage(long)]
    once: bool,

    /// One JSON object per line instead of log lines
    #[usage(long, short = 'J')]
    json: bool,
}

impl DotfilesWatch {
    pub(crate) async fn run(self) -> Result<()> {
        // local-only history is watched by the same command in the local
        // scope, for as long as this one runs
        let _local = LocalWatcher::start(self.once, self.json).await?;
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

/// The watcher of this machine's local-only history, stopped with this one.
struct LocalWatcher(Option<std::process::Child>);

impl LocalWatcher {
    async fn start(once: bool, json: bool) -> Result<Self> {
        if crate::system::history::local::active() {
            return Ok(Self(None));
        }
        let config = crate::config::Config::get().await?;
        if crate::system::history::tracked::TrackedSet::from_config(&config)?
            .local
            .is_empty()
        {
            return Ok(Self(None));
        }
        let mut args = vec!["dot", "watch"];
        if once {
            args.push("--once");
        }
        if json {
            args.push("--json");
        }
        let mut command = crate::system::history::local::command(args)?;
        if once {
            // one pass each; the local one first, so its output is not
            // interleaved with this one's
            let status = command.status()?;
            if !status.success() {
                warn!("history: the local-only history watcher failed: {status}");
            }
            return Ok(Self(None));
        }
        Ok(Self(Some(command.spawn()?)))
    }
}

impl Drop for LocalWatcher {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise dot watch</bold>
    $ <bold>mise dot watch --once</bold>      # one reconcile, for a timer
    $ <bold>mise dot watch --json</bold>
"#
);
