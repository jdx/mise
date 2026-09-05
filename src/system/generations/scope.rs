//! The generation a bootstrap command is recording into.
//!
//! A [`GenerationScope`] is opened at the start of a mutating command and
//! finished at its end. It is process-global so the apply code deep inside
//! `system::*` can append journal entries through [`record`] without every
//! signature threading a writer, and it exports [`ENV_VAR`] so child `mise`
//! processes spawned by hooks attach to the parent's generation instead of
//! opening their own.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use eyre::Result;

use super::journal::JournalEntry;
use super::shadow::{self, ShadowRepo, SnapshotPhase, SnapshotRoot};
use super::store::{
    self, Generation, GenerationStatus, LockfileSnapshot, Snapshot, SnapshotInfo, Summary,
};
use crate::config::Settings;
use crate::dirs;
use crate::env;
use crate::file::{self, display_path};
use crate::lock_file::LockFile;

/// Set in the environment while a generation is open; a child mise that
/// sees it records nothing of its own.
pub(crate) const ENV_VAR: &str = "__MISE_BOOTSTRAP_GENERATION";

/// Bootstrap parts whose changes always leave journal entries, so a run
/// covering only these with an empty journal really did nothing.
const JOURNALED_PARTS: &[&str] = &["dotfiles", "mise-shell-activate"];

struct Writer {
    state_dir: PathBuf,
    shadow: Option<ShadowRepo>,
    roots: Vec<SnapshotRoot>,
    lockfile_path: PathBuf,
    generation: Generation,
}

type Shared = Arc<Mutex<Writer>>;

static CURRENT: Mutex<Option<Shared>> = Mutex::new(None);

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Appends an entry to the open generation, if any, and persists it.
/// Returns the entry's index in the journal.
pub(crate) fn record(entry: JournalEntry) -> Option<u32> {
    let shared = lock_unpoisoned(&CURRENT).clone();
    shared.map(|shared| lock_unpoisoned(&shared).record(entry))
}

/// Whether a generation is open in this process.
pub(crate) fn is_active() -> bool {
    lock_unpoisoned(&CURRENT).is_some()
}

/// RAII handle for the generation a command records into. Inactive scopes
/// (dry runs, recording disabled, nested commands, store failures) are
/// no-ops so callers never branch on them.
#[must_use = "finish the scope so the generation is marked complete"]
pub(crate) struct GenerationScope(Option<Shared>);

impl GenerationScope {
    /// Opens a generation for `command` unless nothing should be recorded.
    pub(crate) fn begin(command: &str, dry_run: bool) -> Self {
        if dry_run {
            return Self(None);
        }
        if !Settings::get().bootstrap.generations.enabled {
            debug!("bootstrap generations: disabled by settings");
            return Self(None);
        }
        if std::env::var_os(ENV_VAR).is_some() {
            debug!("bootstrap generations: attached to the parent mise generation");
            return Self(None);
        }
        if lock_unpoisoned(&CURRENT).is_some() {
            debug!("bootstrap generations: a generation is already open");
            return Self(None);
        }
        match Writer::begin(&dirs::STATE, command) {
            Ok(writer) => {
                let id = writer.generation.id;
                debug!("bootstrap generations: recording generation {id}");
                let shared = Arc::new(Mutex::new(writer));
                *lock_unpoisoned(&CURRENT) = Some(shared.clone());
                env::set_var(ENV_VAR, id.to_string());
                Self(Some(shared))
            }
            Err(err) => {
                warn!("bootstrap generations: not recording this run: {err:#}");
                Self(None)
            }
        }
    }

    /// Runs `f` inside a generation for `command`, a single-part command,
    /// and finishes it with the outcome.
    pub(crate) async fn wrap<T, F>(command: &str, part: &str, dry_run: bool, f: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let scope = Self::begin(command, dry_run);
        let result = f.await;
        let summary = Summary {
            parts: vec![part.to_string()],
            message: None,
        };
        scope.finish(
            result.as_ref().err().map(|err| format!("{err:#}")),
            Some(summary),
        );
        result
    }

    /// Takes the `after` snapshot and marks the generation completed, or
    /// failed when `error` is set.
    pub(crate) fn finish(mut self, error: Option<String>, summary: Option<Summary>) {
        let Some(shared) = self.0.take() else {
            return;
        };
        Self::clear_current();
        if let Err(err) = lock_unpoisoned(&shared).finish(error, summary) {
            warn!("bootstrap generations: could not finish the generation record: {err:#}");
        }
    }

    fn clear_current() {
        *lock_unpoisoned(&CURRENT) = None;
        env::remove_var(ENV_VAR);
    }
}

impl Drop for GenerationScope {
    fn drop(&mut self) {
        let Some(shared) = self.0.take() else {
            return;
        };
        Self::clear_current();
        if std::thread::panicking() {
            return;
        }
        if let Err(err) = lock_unpoisoned(&shared).abandon() {
            warn!("bootstrap generations: could not mark the generation failed: {err:#}");
        }
    }
}

impl Writer {
    fn begin(state_dir: &Path, command: &str) -> Result<Self> {
        store::ensure_store_dir_in(state_dir)?;
        let _lock = store_lock(state_dir)?;
        let id = store::next_id_in(state_dir)?;
        let (shadow, reason) = match ShadowRepo::open_or_init_in(state_dir) {
            Ok(Some(shadow)) => (Some(shadow), None),
            Ok(None) => (None, Some(shadow::unavailable_reason())),
            Err(err) => (None, Some(format!("{err:#}"))),
        };
        if let Some(reason) = &reason {
            warn!("bootstrap generations: no content snapshot for generation {id}: {reason}");
        }
        let argv: Vec<String> = env::ARGS.read().unwrap().iter().skip(1).cloned().collect();
        // What the user typed is the better label; the caller's name is the
        // fallback for in-process callers with no argv (tests).
        let command = if argv.is_empty() {
            command.to_string()
        } else {
            shell_words::join(&argv)
        };
        let generation = Generation {
            schema_version: store::SCHEMA_VERSION,
            id,
            status: GenerationStatus::Pending,
            created_at: store::now_rfc3339(),
            finished_at: None,
            command,
            argv,
            cwd: dirs::CWD.clone().unwrap_or_default(),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .ok(),
            mise_version: crate::cli::version::VERSION_PLAIN.clone(),
            pid: std::process::id(),
            parent: (id > 1).then(|| id - 1),
            rollback_of: None,
            snapshot: SnapshotInfo {
                available: shadow.is_some(),
                reason,
                repo: ShadowRepo::path_in(state_dir),
                before: None,
                after: None,
                unchanged: None,
            },
            lockfile: None,
            journal: vec![],
            summary: None,
            error: None,
        };
        let mut writer = Self {
            state_dir: state_dir.to_path_buf(),
            shadow,
            roots: snapshot_roots(),
            lockfile_path: global_lockfile_path(),
            generation,
        };
        writer.snapshot(SnapshotPhase::Before);
        writer.write()?;
        Ok(writer)
    }

    fn record(&mut self, entry: JournalEntry) -> u32 {
        let seq = self.generation.journal.len() as u32;
        self.generation.journal.push(entry);
        if let Err(err) = self.write() {
            warn!("bootstrap generations: could not persist a journal entry: {err:#}");
        }
        seq
    }

    fn finish(&mut self, error: Option<String>, summary: Option<Summary>) -> Result<()> {
        // Held across the snapshot: its objects are unreferenced until
        // update-ref, and another run's prune must not gc them meanwhile.
        let _lock = store_lock(&self.state_dir)?;
        self.snapshot(SnapshotPhase::After);
        self.generation.finished_at = Some(store::now_rfc3339());
        self.generation.status = if error.is_some() {
            GenerationStatus::Failed
        } else {
            GenerationStatus::Completed
        };
        self.generation.error = error;
        self.generation.summary = summary;
        let id = self.generation.id;
        // A run is a no-op only when every part it covered journals its
        // changes; a part that does not journal yet may have changed the
        // machine without leaving a trace here.
        let journaled_parts = self.generation.summary.as_ref().is_some_and(|summary| {
            !summary.parts.is_empty()
                && summary
                    .parts
                    .iter()
                    .all(|part| JOURNALED_PARTS.contains(&part.as_str()))
        });
        let noop = self.generation.status == GenerationStatus::Completed
            && journaled_parts
            && self.generation.journal.is_empty()
            && self.generation.snapshot.unchanged == Some(true);
        if noop {
            debug!("bootstrap generations: generation {id} changed nothing, not keeping it");
            store::remove_in(&self.state_dir, id)?;
            if let Some(shadow) = &self.shadow {
                shadow.delete_refs(id);
            }
            return Ok(());
        }
        self.write()?;
        let keep = Settings::get().bootstrap.generations.keep;
        let pruned = store::prune_in(&self.state_dir, self.shadow.as_ref(), keep, &[id])?;
        if !pruned.is_empty() {
            debug!("bootstrap generations: pruned {pruned:?}");
            if let Some(shadow) = &self.shadow
                && let Err(err) = shadow.gc()
            {
                warn!("bootstrap generations: gc failed: {err:#}");
            }
        }
        Ok(())
    }

    fn abandon(&mut self) -> Result<()> {
        self.generation.finished_at = Some(store::now_rfc3339());
        self.generation.status = GenerationStatus::Failed;
        self.generation.error = Some("the command exited before finishing".into());
        self.write()
    }

    fn write(&self) -> Result<()> {
        store::write_in(&self.state_dir, &self.generation)
    }

    /// Snapshots the roots and lockfile for `phase`. A failure is recorded
    /// on the generation rather than propagated: the run must go on.
    fn snapshot(&mut self, phase: SnapshotPhase) {
        let id = self.generation.id;
        let lockfile = match std::fs::read(&self.lockfile_path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                warn!(
                    "bootstrap generations: could not read {}: {err}",
                    display_path(&self.lockfile_path)
                );
                None
            }
        };
        let mut blob = None;
        if let Some(shadow) = &self.shadow {
            let phase_name = match phase {
                SnapshotPhase::Before => "before",
                SnapshotPhase::After => "after",
            };
            let message = format!("generation {id} {phase_name}: {}", self.generation.command);
            match shadow.snapshot(&self.roots, lockfile.as_deref(), id, phase, &message) {
                Ok(result) => {
                    for warning in &result.warnings {
                        warn!("bootstrap generations: {warning}");
                    }
                    blob = result.lockfile_blob;
                    let snapshot = Snapshot {
                        commit: result.commit,
                        tree: result.tree,
                        taken_at: store::now_rfc3339(),
                        roots: result.roots,
                        warnings: result.warnings,
                    };
                    let info = &mut self.generation.snapshot;
                    match phase {
                        SnapshotPhase::Before => info.before = Some(snapshot),
                        SnapshotPhase::After => {
                            info.unchanged = info
                                .before
                                .as_ref()
                                .map(|before| before.tree == snapshot.tree);
                            info.after = Some(snapshot);
                        }
                    }
                }
                Err(err) => {
                    warn!("bootstrap generations: snapshot failed: {err:#}");
                    self.generation.snapshot.available = false;
                    self.generation.snapshot.reason = Some(format!("{err:#}"));
                }
            }
        }
        self.generation.lockfile = lockfile.map(|bytes| {
            let sidecar = if self.shadow.is_none() {
                let path = store::sidecar_lockfile_path_in(&self.state_dir, id);
                match file::write_atomic(&path, &bytes) {
                    Ok(()) => Some(path),
                    Err(err) => {
                        warn!("bootstrap generations: could not copy the lockfile: {err:#}");
                        None
                    }
                }
            } else {
                None
            };
            LockfileSnapshot {
                path: self.lockfile_path.clone(),
                sha256: sha256_hex(&bytes),
                blob,
                file: sidecar,
            }
        });
    }
}

fn store_lock(state_dir: &Path) -> Result<fslock::LockFile> {
    LockFile::new(&store::records_dir_in(state_dir))
        .with_callback(|path| {
            debug!(
                "waiting for the bootstrap generation lock {}",
                display_path(path)
            );
        })
        .lock()
}

/// The global config directory (where `--from-git` checks out) and
/// `dotfiles.root`, in that order.
fn snapshot_roots() -> Vec<SnapshotRoot> {
    let config_dir = env::MISE_GLOBAL_CONFIG_FILE
        .as_deref()
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dirs::CONFIG.to_path_buf());
    vec![
        SnapshotRoot {
            label: "config".into(),
            path: config_dir,
        },
        SnapshotRoot {
            label: "dotfiles".into(),
            path: crate::system::files::dotfiles_root(),
        },
    ]
}

fn global_lockfile_path() -> PathBuf {
    crate::lockfile::lockfile_path_for_config(&crate::config::global_config_path(), None).0
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}
