use std::path::{Path, PathBuf};

use eyre::{Result, bail};

use crate::file::display_path;
use crate::system::history::checkpoint::{Draft, Outcome, Store};
use crate::system::history::store::{DescriptionSource, Entry, Trigger};
use crate::system::history::tracked::{display_to_tree_path, normalize_target};

/// Save a checkpoint of the tracked files now
///
/// Fails when history cannot save or a requested path is not tracked, so a
/// script or an agent gets a trustworthy result; a save that finds nothing
/// changed succeeds as a no-op. `--best-effort` turns save errors into a
/// warning for `set -e` update scripts.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesSave {
    /// Paths to save; every one must be tracked
    #[usage(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// A description for the checkpoint
    #[usage(long, short, value_name = "TEXT")]
    description: Option<String>,

    /// What is saving: save (the default), agent, or update
    #[usage(long, value_name = "TRIGGER", default = "save")]
    trigger: String,

    /// The task an agent is working on
    #[usage(long, value_name = "ID")]
    task: Option<String>,

    /// A label to find the checkpoint by later
    #[usage(long, value_name = "LABEL")]
    label: Vec<String>,

    /// Warn instead of failing when history cannot save
    #[usage(long)]
    best_effort: bool,
}

impl DotfilesSave {
    pub(crate) async fn run(self) -> Result<()> {
        self.save().await
    }

    fn save_checkpoint(
        &self,
        store: &Store,
        tracked: &crate::system::history::tracked::TrackedSet,
        draft: Draft,
    ) -> Result<()> {
        match self.capture(store, tracked, draft) {
            Ok(()) => Ok(()),
            Err(err) if self.best_effort => {
                warn!("history save: {err:#}");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    async fn save(self) -> Result<()> {
        let trigger = match self.trigger.as_str() {
            "save" => Trigger::Save,
            "agent" => Trigger::Agent,
            "update" => Trigger::Update,
            other => bail!("unknown trigger {other:?}; use save, agent, or update"),
        };
        if !crate::config::Settings::get().history.enabled {
            bail!("history is disabled (history.enabled = false)");
        }
        let (store, tracked, entries) = super::history::open().await?;
        if !crate::system::history::local::active()
            && tracked.invalid_local.iter().any(|invalid| {
                self.paths.is_empty()
                    || self.paths.iter().any(|path| {
                        let path = normalize_target(path);
                        path.starts_with(invalid) || invalid.starts_with(path)
                    })
            })
        {
            bail!(
                "invalid local-only declaration: correct it before saving the affected shared paths"
            );
        }
        // local-only paths are saved in this machine's own history, by the
        // same command in the local scope
        let mut shared = self.paths.clone();
        if !tracked.local.is_empty() {
            let (local, rest): (Vec<_>, Vec<_>) = self.paths.iter().cloned().partition(|path| {
                let path = normalize_target(path);
                tracked.local.iter().any(|local| path.starts_with(local))
            });
            shared = rest;
            if self.paths.is_empty() || !local.is_empty() {
                let result = crate::system::history::local::run(self.local_args(&local));
                match result {
                    Err(err) if self.best_effort => warn!("history save: {err:#}"),
                    result => result?,
                }
            }
            if !self.paths.is_empty() && shared.is_empty() {
                return Ok(());
            }
        }
        let mut this = self;
        this.paths = shared;
        this.save_shared(store, tracked, entries, trigger).await
    }

    /// `mise dot save` arguments that save `paths` (all, when empty) in
    /// the local-only history.
    fn local_args(&self, paths: &[PathBuf]) -> Vec<std::ffi::OsString> {
        let mut args: Vec<std::ffi::OsString> = vec![
            "dot".into(),
            "save".into(),
            "--trigger".into(),
            self.trigger.clone().into(),
        ];
        if let Some(description) = &self.description {
            args.extend(["--description".into(), description.into()]);
        }
        if let Some(task) = &self.task {
            args.extend(["--task".into(), task.into()]);
        }
        for label in &self.label {
            args.extend(["--label".into(), label.into()]);
        }
        if self.best_effort {
            args.push("--best-effort".into());
        }
        args.push("--".into());
        args.extend(paths.iter().map(|path| path.clone().into_os_string()));
        args
    }

    async fn save_shared(
        self,
        store: Store,
        tracked: crate::system::history::tracked::TrackedSet,
        entries: Vec<Entry>,
        trigger: Trigger,
    ) -> Result<()> {
        if !self.paths.is_empty() {
            let walk = tracked.walk()?;
            for path in &self.paths {
                let path = normalize_target(path);
                let captured = walk.files.keys().any(|file| file.starts_with(&path));
                if !captured {
                    // saving a deleted manual-save path saves the deletion,
                    // provided the path was captured before (a path that
                    // never existed is not a deletion)
                    let deletion = tracked
                        .entry_for(&path)
                        .is_some_and(|entry| !entry.policy.autosave)
                        && std::fs::symlink_metadata(&path)
                            .is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
                        && previously_captured(&store, &entries, &path)?;
                    if deletion {
                        continue;
                    }
                    let reason = if tracked.entry_for(&path).is_some() {
                        "it is excluded, missing, or omitted from capture"
                    } else {
                        "track it with `mise dot track`"
                    };
                    bail!("{} is not captured; {reason}", display_path(&path));
                }
            }
        }
        let mut draft = Draft::new(trigger);
        draft.explicit_paths = self
            .paths
            .iter()
            .map(|path| normalize_target(path))
            .collect();
        draft.description = self.description.clone();
        draft.description_source = Some(if trigger == Trigger::Agent {
            DescriptionSource::Agent
        } else {
            DescriptionSource::User
        });
        draft.task = self.task.clone();
        draft.labels = self.label.clone();
        tokio::task::spawn_blocking(move || self.save_checkpoint(&store, &tracked, draft)).await?
    }

    fn capture(
        &self,
        store: &Store,
        tracked: &crate::system::history::tracked::TrackedSet,
        draft: Draft,
    ) -> Result<()> {
        if let Some(reason) = store.unavailable() {
            bail!("cannot save: {reason}");
        }
        // a save is a write: it waits for no running operation, refuses to
        // interleave with one, and closes one that died
        let _operation = crate::system::history::scope::take_operation_lock(store, tracked)?;
        match store.attempt(tracked, draft)? {
            Outcome::Created(entry) => {
                let scope = if crate::system::history::local::active() {
                    " (local-only)"
                } else {
                    ""
                };
                info!(
                    "history{scope}: saved checkpoint {}: {}",
                    entry.id, entry.checkpoint.description
                );
                Ok(())
            }
            Outcome::Unchanged => {
                info!("history: nothing changed since the latest checkpoint");
                Ok(())
            }
            Outcome::Unavailable(reason) => bail!("cannot save: {reason}"),
        }
    }
}

/// Whether the newest checkpoint holds `path`.
fn previously_captured(store: &Store, entries: &[Entry], path: &Path) -> Result<bool> {
    let Some(repo) = store.repo() else {
        return Ok(false);
    };
    let Some(snapshot) = entries
        .iter()
        .rev()
        .find_map(|entry| entry.checkpoint.tree.snapshot.as_ref())
    else {
        return Ok(false);
    };
    let tree_path = display_to_tree_path(&display_path(path));
    Ok(repo.object_at(snapshot, &tree_path)?.is_some())
}
