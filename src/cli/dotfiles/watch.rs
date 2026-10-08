use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eyre::Result;

use crate::system::history::watch::runtime::{self, WatchOptions};

/// Save tracked files as they change
///
/// Runs in the foreground and saves a checkpoint once a changed file has been
/// quiet for `history.watch.debounce`. A file that keeps changing does not
/// delay the others and is still saved every `history.watch.max_interval`. The
/// watcher also rescans every tracked path at startup, every
/// `history.watch.reconcile`, and when the configuration changes. Entries
/// tracked with `--no-autosave` are not watched.
///
/// With an origin connected, the watcher also syncs according to the
/// `history.sync` setting. In `sync` mode it pushes within
/// `history.sync_interval` of a save, fetches every `history.fetch_interval`,
/// and applies incoming changes when there are no conflicts. In `fetch-only`
/// mode it only fetches; in `manual` mode it does not use the network. A
/// failed sync is retried with backoff while saving continues. `--once` runs
/// one rescan and one sync, then exits.
///
/// The built-in `history-watch` service runs this for you:
///
///     [bootstrap.services.mise-history]
///     builtin = "history-watch"
///
/// Exits 0 when history is disabled or another watcher is already running,
/// and 1 when Git is unusable, the store cannot be opened, or no watch can be
/// installed. A failed save is retried with backoff and never drops the
/// pending changes, and a save that would overlap another history operation
/// is deferred. With `--once`, nothing is retried: a save that fails or is
/// deferred, or a sync that fails, exits 1.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example("mise dot watch", help = "Watch and save until stopped"),
    example("mise dot watch --once", help = "Rescan and sync once, for a timer"),
    example(
        "mise dot watch --json",
        help = "Print JSON lines instead of log lines"
    )
)]
pub(crate) struct DotfilesWatch {
    /// Rescan and sync once, then exit (for timers and cron)
    #[usage(long)]
    once: bool,

    /// Print one JSON object per line instead of log lines
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
