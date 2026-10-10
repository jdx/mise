//! Which saved stream versions this machine holds live.
//!
//! **A stream this machine never held is not its own to change.** The local
//! history carries every stream, including ones another machine wrote while
//! they were not selected here: a profile that was inactive when this
//! machine adopted the setup, or a variant it switched away from. Once such
//! a stream is selected, the saved version is the other machine's, not
//! this one's, so a missing or older live file is not an edit made here. A
//! capture that recorded it would publish a deletion or a revert, and a
//! pull that compared against it would find nothing to apply.
//!
//! A stream that stays selected is held: synchronization applies every
//! incoming version of it. One selected again is held when its saved
//! version is still the one it had when it was last selected here, when
//! nothing is saved for it, or when its live files already match. Until
//! then a capture carries it unchanged, and a pull compares it as a fresh
//! adoption, which applies it or asks which side to keep.
//!
//! Only incoming history can change a stream that is not selected, so
//! without an upstream every stream is held. The record is replaceable
//! bookkeeping: without it, every selected stream counts as held, as before
//! it existed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::shadow::HistoryRepo;
use super::sync::reconcile::Object;

/// Beside the checkpoint index, outside the repository it describes.
fn path(repo: &HistoryRepo) -> PathBuf {
    repo.dir()
        .parent()
        .map_or_else(|| repo.dir().join("index"), |store| store.join("index"))
        .join("held.json")
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    /// The streams selected and held at the latest capture.
    selected: BTreeSet<String>,
    /// For each stream that was held and is no longer selected, its saved
    /// version when it stopped being selected.
    #[serde(default)]
    frozen: BTreeMap<String, Option<Object>>,
}

#[derive(Debug, Default)]
pub(crate) struct Held {
    /// `None` when no record exists yet: every selected stream is held.
    record: Option<Record>,
    /// Whether any history arrived from elsewhere.
    upstream: bool,
}

impl Held {
    pub(crate) fn load(repo: &HistoryRepo) -> Result<Self> {
        let record = match std::fs::read(path(repo)) {
            // unreadable bookkeeping is replaced, never trusted in part
            Ok(bytes) => serde_json::from_slice(&bytes).ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            record,
            upstream: repo.ref_oid(super::sync::network::UPSTREAM_REF)?.is_some(),
        })
    }

    /// Whether a selected stream is held whatever its saved version: when
    /// no history arrived from elsewhere, or it stayed selected. Checked
    /// first, so the common case reads nothing from Git.
    pub(crate) fn always_holds(&self, stream: &str) -> bool {
        !self.upstream
            || self
                .record
                .as_ref()
                .is_none_or(|record| record.selected.contains(stream))
    }

    /// Whether this machine holds `saved`, a selected stream's version in
    /// the local history.
    pub(crate) fn holds(&self, stream: &str, saved: Option<&Object>) -> bool {
        let Some(saved) = saved else {
            return true;
        };
        if !self.upstream {
            return true;
        }
        let Some(record) = &self.record else {
            return true;
        };
        record.selected.contains(stream) || record.frozen.get(stream) == Some(&Some(saved.clone()))
    }

    /// The saved version a stream had when it stopped being selected here:
    /// the version this machine still has live, unless it changed it since.
    pub(crate) fn frozen(&self, stream: &str) -> Option<&Object> {
        self.record.as_ref()?.frozen.get(stream)?.as_ref()
    }

    /// Record a capture: `held` are the selected streams it held, and
    /// `saved` reads a stream's version in the history it started from.
    /// A stream held before and not selected now keeps that version.
    pub(crate) fn observe(
        &mut self,
        held: BTreeSet<String>,
        saved: impl Fn(&str) -> Result<Option<Object>>,
    ) -> Result<()> {
        let mut next = self.record.clone().unwrap_or_default();
        for stream in next.selected.difference(&held) {
            next.frozen.insert(stream.clone(), saved(stream)?);
        }
        next.frozen.retain(|stream, _| !held.contains(stream));
        next.selected = held;
        self.record = Some(next);
        Ok(())
    }

    /// Hold a stream as it is live here, so the next capture saves this
    /// machine's version over the saved one: keeping the local side.
    pub(crate) fn keep(&mut self, stream: &str) {
        // without a record every stream is held already
        let Some(record) = &mut self.record else {
            return;
        };
        record.selected.insert(stream.to_string());
        record.frozen.remove(stream);
    }

    pub(crate) fn save(&self, repo: &HistoryRepo) -> Result<()> {
        match &self.record {
            Some(record) => super::store::write_json(&path(repo), record),
            None => Ok(()),
        }
    }
}

/// The selected streams a capture holds, and the entries it carries
/// because this machine never held their saved version.
#[derive(Debug, Default)]
pub(crate) struct Streams {
    pub(crate) held: BTreeSet<String>,
    pub(crate) carry: Vec<usize>,
}

pub(crate) fn streams(
    repo: &HistoryRepo,
    held: &Held,
    parent: Option<&str>,
    walk: &super::tracked::Walk,
) -> Result<Streams> {
    let mut streams = Streams::default();
    for (index, entry) in walk.entries.iter().enumerate() {
        let stream = entry.tree_path(&entry.path)?;
        if held.always_holds(&stream) {
            streams.held.insert(stream);
            continue;
        }
        let saved = parent
            .map(|head| repo.object_at(head, &stream))
            .transpose()?
            .flatten();
        if held.holds(&stream, saved.as_ref())
            || live_matches_saved(repo, parent, walk, index, &stream)?
        {
            streams.held.insert(stream);
        } else {
            streams.carry.push(index);
        }
    }
    Ok(streams)
}

/// Whether an entry's live files are exactly its saved ones, compared as
/// plaintext so an encrypted file matches its saved ciphertext.
fn live_matches_saved(
    repo: &HistoryRepo,
    parent: Option<&str>,
    walk: &super::tracked::Walk,
    index: usize,
    stream: &str,
) -> Result<bool> {
    let Some(head) = parent else {
        return Ok(false);
    };
    let roots = super::sync::layout::Roots::current();
    let live: BTreeSet<&PathBuf> = walk
        .files
        .iter()
        .filter(|(_, (owner, _))| *owner == index)
        .map(|(path, _)| path)
        .collect();
    let mut saved = 0;
    for file in repo.ls_tree(head)? {
        if file.path != stream && !file.path.starts_with(&format!("{stream}/")) {
            continue;
        }
        saved += 1;
        let Some(local) = roots.locate(&file.path).path().map(Path::to_path_buf) else {
            return Ok(false);
        };
        if !live.contains(&local) {
            return Ok(false);
        }
        let Ok(object) = super::sync::apply::live_object(repo, &local) else {
            return Ok(false);
        };
        // a version this machine cannot decrypt is not one it holds
        if repo.restored_object_at(head, &file.path).ok().flatten() != object {
            return Ok(false);
        }
    }
    Ok(saved == live.len())
}

/// The selected entries a save would carry rather than capture, against
/// the history at `saved`, each as its stream and display path.
pub fn unheld_entries(
    repo: &HistoryRepo,
    saved: &str,
    walk: &super::tracked::Walk,
) -> Result<Vec<(String, String)>> {
    let held = Held::load(repo)?;
    streams(repo, &held, Some(saved), walk)?
        .carry
        .iter()
        .map(|index| {
            let entry = &walk.entries[*index];
            Ok((entry.tree_path(&entry.path)?, entry.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(oid: &str) -> Object {
        ("100644".into(), oid.into())
    }

    fn streams(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_stream_selected_again_is_held_at_the_version_it_left() -> Result<()> {
        let mut held = Held {
            record: None,
            upstream: true,
        };
        // no record yet: everything counts as held
        assert!(held.holds("home@work/.f", Some(&obj("a"))));
        held.observe(streams(&["home/.g", "home@work/.f"]), |_| Ok(None))?;
        assert!(held.holds("home@work/.f", Some(&obj("a"))));
        // deselected at version a
        held.observe(streams(&["home/.g"]), |_| Ok(Some(obj("a"))))?;
        assert!(held.holds("home@work/.f", Some(&obj("a"))));
        // another machine's newer version is not the one held here
        assert!(!held.holds("home@work/.f", Some(&obj("b"))));
        // nothing saved is nothing to lose
        assert!(held.holds("home@work/.f", None));
        // a stream never held here
        assert!(!held.holds("home@home/.h", Some(&obj("h"))));
        // held again
        held.observe(streams(&["home/.g", "home@work/.f"]), |_| Ok(None))?;
        assert!(held.holds("home@work/.f", Some(&obj("b"))));
        Ok(())
    }

    #[test]
    fn without_an_upstream_every_stream_is_held() {
        let held = Held {
            record: Some(Record::default()),
            upstream: false,
        };
        assert!(held.holds("home@work/.f", Some(&obj("a"))));
    }
}
