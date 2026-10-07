use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
/// One that fails is reported and started again, backing off while it
/// keeps failing; one that exits successfully (history disabled, or another
/// watcher already running) has nothing to do and stays stopped.
struct LocalWatcher(Option<Supervised>);

struct Supervised {
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
}

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
        if once {
            // one pass each; the local one first, so its output is not
            // interleaved with this one's
            let status = crate::system::history::local::command(&args)?.status()?;
            if !status.success() {
                warn!("history: the local-only history watcher failed: {status}");
            }
            return Ok(Self(None));
        }
        let child = Arc::new(Mutex::new(Some(
            crate::system::history::local::command(&args)?.spawn()?,
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let supervised = Supervised {
            child: child.clone(),
            stop: stop.clone(),
        };
        std::thread::spawn(move || supervise(&args, &child, &stop));
        Ok(Self(Some(supervised)))
    }
}

fn supervise(args: &[&str], child: &Mutex<Option<Child>>, stop: &AtomicBool) {
    const MAX_DELAY: Duration = Duration::from_secs(300);
    let mut delay = Duration::from_secs(5);
    let mut started = Instant::now();
    let mut restart_at: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_secs(1));
        let mut guard = child.lock().unwrap_or_else(|err| err.into_inner());
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if let Some(running) = guard.as_mut()
            && let Ok(Some(status)) = running.try_wait()
        {
            *guard = None;
            if status.success() {
                return;
            }
            // one that ran a while failed afresh: start over from the
            // shortest delay
            if started.elapsed() > MAX_DELAY {
                delay = Duration::from_secs(5);
            }
            warn!(
                "history: the local-only history watcher exited ({status}); local-only files are not saved until it restarts in {}s",
                delay.as_secs()
            );
            restart_at = Some(Instant::now() + delay);
            delay = (delay * 2).min(MAX_DELAY);
        }
        if guard.is_none()
            && let Some(at) = restart_at
            && Instant::now() >= at
        {
            restart_at = None;
            match crate::system::history::local::command(args)
                .and_then(|mut command| command.spawn().map_err(Into::into))
            {
                Ok(spawned) => {
                    started = Instant::now();
                    *guard = Some(spawned);
                }
                Err(err) => {
                    warn!("history: could not restart the local-only history watcher: {err:#}");
                    restart_at = Some(Instant::now() + delay);
                    delay = (delay * 2).min(MAX_DELAY);
                }
            }
        }
    }
}

impl Drop for LocalWatcher {
    fn drop(&mut self) {
        if let Some(supervised) = &self.0 {
            supervised.stop.store(true, Ordering::Relaxed);
            let mut guard = supervised
                .child
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
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
