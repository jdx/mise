use std::path::PathBuf;

use eyre::{Result, bail};

use crate::file::display_path;
use crate::system::history::checkpoint::{Draft, Outcome};
use crate::system::history::store::{DescriptionSource, Trigger};
use crate::system::history::tracked::normalize_target;

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
        match self.save().await {
            Ok(()) => Ok(()),
            Err(err) if self.best_effort => {
                warn!("history save: {err:#}");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    async fn save(&self) -> Result<()> {
        let trigger = match self.trigger.as_str() {
            "save" => Trigger::Save,
            "agent" => Trigger::Agent,
            "update" => Trigger::Update,
            other => bail!("unknown trigger {other:?}; use save, agent, or update"),
        };
        if !crate::config::Settings::get().history.enabled {
            bail!("history is disabled (history.enabled = false)");
        }
        let (store, tracked, _entries) = super::history::open().await?;
        if let Some(reason) = store.unavailable() {
            bail!("cannot save: {reason}");
        }
        if !self.paths.is_empty() {
            let walk = tracked.walk()?;
            for path in &self.paths {
                let path = normalize_target(path);
                let captured = walk.files.keys().any(|file| file.starts_with(&path));
                if !captured {
                    // saving a deleted manual-save path saves the deletion
                    let deletion = tracked
                        .entry_for(&path)
                        .is_some_and(|entry| !entry.policy.autosave)
                        && std::fs::symlink_metadata(&path).is_err();
                    if deletion {
                        continue;
                    }
                    let reason = if tracked.entry_for(&path).is_some() {
                        "it is excluded, missing, or omitted from capture"
                    } else {
                        "track it with `mise bootstrap dotfiles track`"
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
        // a save is a write: it waits for no running operation, refuses to
        // interleave with one, and closes one that died
        let _operation = crate::system::history::scope::take_operation_lock(&store, &tracked)?;
        match store.attempt(&tracked, draft)? {
            Outcome::Created(entry) => {
                info!(
                    "history: saved checkpoint {}: {}",
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
