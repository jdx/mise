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
    /// Streams whose live version `--keep-local` chose: the next capture
    /// saves them, while a pull still compares them as not held, so a
    /// later `--take-remote` can take the repository's version instead.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    kept: BTreeSet<String>,
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
        next.kept.retain(|stream| !held.contains(stream));
        next.selected = held;
        self.record = Some(next);
        Ok(())
    }

    /// Whether a capture saves this stream's live version although it is
    /// not held: `--keep-local` chose it.
    pub(crate) fn kept(&self, stream: &str) -> bool {
        self.record
            .as_ref()
            .is_some_and(|record| record.kept.contains(stream))
    }

    /// Choose a stream's live version, so the next capture saves it over
    /// the saved one: keeping the local side. Until then, a pull still
    /// treats the stream as not held.
    pub(crate) fn keep(&mut self, stream: &str) {
        // without a record every stream is held already
        if let Some(record) = &mut self.record {
            record.kept.insert(stream.to_string());
        }
    }

    /// Withdraw a `--keep-local` choice: `--take-remote` decided instead.
    pub(crate) fn unkeep(&mut self, stream: &str) {
        if let Some(record) = &mut self.record {
            record.kept.remove(stream);
        }
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

/// Determine which tracked entries are held at their saved version and which
/// must be carried into the next checkpoint.
pub(crate) fn streams(
    repo: &HistoryRepo,
    held: &Held,
    parent: Option<&str>,
    walk: &super::tracked::Walk,
) -> Result<Streams> {
    let mut streams = Streams::default();
    // loaded when the first stream needs a comparison, then shared; `Some(None)`
    // is a manifest that could not be read, which holds nothing and is not retried
    let mut encrypted: Option<Option<BTreeSet<String>>> = None;
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
        let unchanged = if held.holds(&stream, saved.as_ref()) || held.kept(&stream) {
            true
        } else {
            match encrypted
                .get_or_insert_with(|| super::sync::files::encrypted_prefixes(repo, parent).ok())
            {
                Some(paths) => {
                    live_matches_saved(repo, parent, walk, index, &stream, saved, paths)?
                }
                None => false,
            }
        };
        if unchanged {
            streams.held.insert(stream);
        } else {
            streams.carry.push(index);
        }
    }
    Ok(streams)
}

/// Whether every file an entry saved is live and unchanged, compared as
/// plaintext so an encrypted file matches its saved ciphertext.
///
/// A live file with no saved version is an addition when the history never
/// saved it, such as a file the entry now reaches because it replaced
/// narrower ones: capturing it publishes neither a deletion nor a revert.
/// One the history saved and no longer has was deleted by another machine,
/// and capturing it would undo that deletion.
fn live_matches_saved(
    repo: &HistoryRepo,
    parent: Option<&str>,
    walk: &super::tracked::Walk,
    index: usize,
    stream: &str,
    saved: Option<Object>,
    encrypted: &BTreeSet<String>,
) -> Result<bool> {
    let Some(head) = parent else {
        return Ok(false);
    };
    let roots = super::sync::layout::Roots::current();
    // the files this entry owns itself, by the same rule as the saved ones
    // below: a nested entry's files are that entry's to compare
    let live: BTreeSet<&PathBuf> = walk
        .files
        .keys()
        .filter(|path| super::tracked::owning_entry_index(&walk.entries, path) == Some(index))
        .collect();
    // only this stream's own files: a directory's subtree, or the one file
    let paths: Vec<(String, String, String)> = match saved {
        None => vec![],
        Some((mode, oid)) if mode == "040000" => repo
            .ls_tree(&oid)?
            .into_iter()
            .map(|file| (format!("{stream}/{}", file.path), file.mode, file.oid))
            .collect(),
        Some((mode, oid)) => vec![(stream.to_string(), mode, oid)],
    };
    let mut compared = BTreeSet::new();
    for (path, mode, oid) in paths {
        let Some(local) = roots.locate(&path).path().map(Path::to_path_buf) else {
            return Ok(false);
        };
        // a file a nested entry tracks is that entry's to compare
        if super::tracked::owning_entry_index(&walk.entries, &local) != Some(index) {
            continue;
        }
        if !live.contains(&local) {
            return Ok(false);
        }
        compared.insert(local.clone());
        let Ok(object) = super::sync::apply::live_object(repo, &local) else {
            return Ok(false);
        };
        let expected = if matches!(mode.as_str(), "040000" | "160000") {
            Some((mode, oid))
        } else if super::sync::files::is_encrypted(encrypted, &path) {
            // a version this machine cannot decrypt is not one it holds
            match super::sync::files::decrypt(
                repo,
                &path,
                &(mode, oid),
                console::user_attended_stderr(),
            ) {
                Ok(decrypted) => Some(decrypted),
                Err(_) => return Ok(false),
            }
        } else {
            Some((mode, oid))
        };
        if expected != object {
            return Ok(false);
        }
    }
    let entry = &walk.entries[index];
    let added = live
        .difference(&compared.iter().collect())
        .map(|local| entry.tree_path(local))
        .collect::<Result<Vec<_>>>()?;
    if added.is_empty() {
        return Ok(true);
    }
    // one history search for the whole stream, not one per added file
    let deleted = repo.deleted_paths_under(head, stream)?;
    Ok(!added.iter().any(|path| deleted.contains(path)))
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

    /// `live_matches_saved` against the version `commit` holds of the stream.
    #[cfg(unix)]
    fn check(
        repo: &HistoryRepo,
        commit: &str,
        walk: &super::super::tracked::Walk,
        index: usize,
        stream: &str,
        encrypted: &BTreeSet<String>,
    ) -> Result<bool> {
        let saved = repo.object_at(commit, stream)?;
        live_matches_saved(repo, Some(commit), walk, index, stream, saved, encrypted)
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
    fn a_kept_stream_is_saved_but_still_unheld_until_then() -> Result<()> {
        let mut held = Held {
            record: Some(Record::default()),
            upstream: true,
        };
        held.keep("home@work/.f");
        assert!(held.kept("home@work/.f"));
        // a pull still compares it as not held
        assert!(!held.holds("home@work/.f", Some(&obj("a"))));
        held.unkeep("home@work/.f");
        assert!(!held.kept("home@work/.f"));
        held.keep("home@work/.f");
        // the capture that saves it holds it
        held.observe(streams(&["home@work/.f"]), |_| Ok(None))?;
        assert!(!held.kept("home@work/.f"));
        assert!(held.holds("home@work/.f", Some(&obj("b"))));
        Ok(())
    }

    /// A directory enrolled in place of narrower entries covers files the
    /// narrower ones did not: those are additions, not a revert of what
    /// this machine saved, so the saved version stays this machine's. A
    /// file the history saved and another machine deleted is not one.
    #[cfg(unix)]
    #[test]
    fn live_files_never_saved_do_not_unhold_a_widened_entry() -> Result<()> {
        use crate::files::{FileMode, FilePolicy};
        use crate::history::checkpoint::test_checkpoint;
        use crate::history::shadow::Overlay;
        use crate::history::sync::layout::Roots;
        use crate::history::tracked::{TrackedEntry, Walk, normalize};

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let roots = Roots::current();
        let scratch = tempfile::Builder::new()
            .prefix(".history-held-widened-")
            .tempdir_in(&roots.home)?;
        let skills = normalize(scratch.path()).join("skills");
        let child = skills.join("child");
        std::fs::create_dir_all(&child)?;
        std::fs::write(child.join("SKILL.md"), "child")?;
        std::fs::write(child.join("new.md"), "new")?;
        std::fs::write(skills.join("loose.md"), "loose")?;

        let policy = FilePolicy::for_mode(FileMode::Track);
        let parent = TrackedEntry::new(skills.clone(), "track", policy);
        let nested = TrackedEntry::new(child.clone(), "track", policy);
        let stream = parent.tree_path(&skills)?;
        let overlay = |entry: &TrackedEntry, path: &Path, content: &[u8]| -> Result<Overlay> {
            Ok(Overlay {
                path: entry.tree_path(path)?,
                object: Some(("100644".into(), repo.hash_blob(content)?)),
            })
        };
        let save = |name: &str, overlays: &[Overlay]| -> Result<String> {
            let tree = repo.compose(&repo.empty_object("tree")?, overlays)?;
            repo.write_checkpoint(Some(&tree), &test_checkpoint(name, Some(&tree)))
        };
        // saved under the narrower entry: only the child's file
        let narrow = save(
            "narrow",
            &[overlay(&nested, &child.join("SKILL.md"), b"child")?],
        )?;
        let walk = |loose: bool| {
            let mut walk = Walk {
                entries: vec![parent.clone(), nested.clone()],
                ..Default::default()
            };
            walk.files.insert(child.join("SKILL.md"), (1, policy));
            walk.files.insert(child.join("new.md"), (1, policy));
            if loose {
                walk.files.insert(skills.join("loose.md"), (0, policy));
            }
            walk
        };
        let matches = |saved: &str, loose: bool| {
            check(&repo, saved, &walk(loose), 0, &stream, &BTreeSet::new())
        };
        // the nested entry's files are its own to compare; the loose one is
        // new, though a stream covering its directory was saved before
        assert!(matches(&narrow, true)?);
        assert!(matches(&narrow, false)?);

        // the child entry itself replaced by the parent, with a file added
        // inside the old child's directory
        let replaced = || {
            let mut walk = Walk {
                entries: vec![parent.clone()],
                ..Default::default()
            };
            for name in ["SKILL.md", "new.md"] {
                walk.files.insert(child.join(name), (0, policy));
            }
            walk
        };
        assert!(check(
            &repo,
            &narrow,
            &replaced(),
            0,
            &stream,
            &BTreeSet::new()
        )?);

        // a saved file the entry owns that changed or went missing is not held
        let owned = save(
            "owned",
            &[
                overlay(&nested, &child.join("SKILL.md"), b"child")?,
                overlay(&parent, &skills.join("loose.md"), b"loose")?,
            ],
        )?;
        assert!(matches(&owned, true)?);
        assert!(!matches(&owned, false)?);
        std::fs::write(skills.join("loose.md"), "edited elsewhere")?;
        assert!(!matches(&owned, true)?);
        std::fs::write(skills.join("loose.md"), "loose")?;

        // a file named like a glob over a saved one is a different file
        std::fs::write(skills.join("a.md"), "a")?;
        std::fs::write(skills.join("[ab].md"), "glob")?;
        let globbed = save("globbed", &[overlay(&parent, &skills.join("a.md"), b"a")?])?;
        let mut walk = Walk {
            entries: vec![parent.clone()],
            ..Default::default()
        };
        for name in ["a.md", "[ab].md"] {
            walk.files.insert(skills.join(name), (0, policy));
        }
        assert!(check(&repo, &globbed, &walk, 0, &stream, &BTreeSet::new())?);

        // a merge that dropped the loose file deleted it too
        let blob = |content: &[u8]| repo.hash_blob(content);
        let tree_of = |files: &[(&str, &[u8])]| -> Result<String> {
            let overlays = files
                .iter()
                .map(|(name, content)| {
                    Ok(Overlay {
                        path: parent.tree_path(&skills.join(name))?,
                        object: Some(("100644".into(), blob(content)?)),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            repo.compose(&repo.empty_object("tree")?, &overlays)
        };
        let base = repo.commit_tree(&tree_of(&[("loose.md", b"loose")])?, vec![], "base")?;
        let ours = repo.commit_tree(
            &tree_of(&[("loose.md", b"loose"), ("ours.md", b"ours")])?,
            vec![&base],
            "ours",
        )?;
        let theirs = repo.commit_tree(
            &tree_of(&[("loose.md", b"loose"), ("theirs.md", b"theirs")])?,
            vec![&base],
            "theirs",
        )?;
        let merged = repo.commit_tree(
            &tree_of(&[("ours.md", b"ours"), ("theirs.md", b"theirs")])?,
            vec![&ours, &theirs],
            "merge",
        )?;
        assert!(
            repo.deleted_paths_under(&merged, &stream)?
                .contains(&parent.tree_path(&skills.join("loose.md"))?)
        );

        // another machine deleted the loose file the history once saved
        let deleted = save(
            "deleted",
            &[overlay(&nested, &child.join("SKILL.md"), b"child")?],
        )?;
        assert!(!matches(&deleted, true)?);
        Ok(())
    }

    /// An encrypted file saved as ciphertext is held when the live file is
    /// the plaintext it decrypts to, and only when its path is known to be
    /// encrypted.
    #[cfg(unix)]
    #[test]
    fn an_encrypted_file_is_held_when_live_matches_its_plaintext() -> Result<()> {
        use crate::files::{FileMode, FilePolicy};
        use crate::history::checkpoint::test_checkpoint;
        use crate::history::shadow::Overlay;
        use crate::history::sync::layout::Roots;
        use crate::history::tracked::{TrackedEntry, Walk, normalize};
        use age::secrecy::ExposeSecret;

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let key = age::x25519::Identity::generate();
        let mut environment = mise_util::testing::EnvVarGuard::new();
        environment.set("MISE_AGE_KEY", key.to_string().expose_secret());
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(key.to_public())];

        let roots = Roots::current();
        let scratch = tempfile::Builder::new()
            .prefix(".history-held-encrypted-")
            .tempdir_in(&roots.home)?;
        let file = normalize(scratch.path()).join("secret.txt");
        std::fs::write(&file, "private contents")?;

        let policy = FilePolicy::for_mode(FileMode::Track);
        let entry = TrackedEntry::new(file.clone(), "track", policy);
        let stream = entry.tree_path(&file)?;
        let ciphertext = crate::history::sync::files::encode(
            &stream,
            "100644",
            b"private contents",
            "scheme",
            &recipients,
        )?;
        let overlay = Overlay {
            path: stream.clone(),
            object: Some(("100644".into(), repo.hash_blob(&ciphertext)?)),
        };
        let tree = repo.compose(&repo.empty_object("tree")?, &[overlay])?;
        let saved = repo.write_checkpoint(Some(&tree), &test_checkpoint("saved", Some(&tree)))?;

        let mut walk = Walk {
            entries: vec![entry],
            ..Default::default()
        };
        walk.files.insert(file.clone(), (0, policy));
        let encrypted = BTreeSet::from([stream.clone()]);
        let matches =
            |encrypted: &BTreeSet<String>| check(&repo, &saved, &walk, 0, &stream, encrypted);
        // the live plaintext is what the saved ciphertext decrypts to
        assert!(matches(&encrypted)?);
        // without knowing the path is encrypted, the ciphertext is compared as is
        assert!(!matches(&BTreeSet::new())?);
        // a live edit is not the saved version
        std::fs::write(&file, "edited")?;
        assert!(!matches(&encrypted)?);
        Ok(())
    }

    /// `streams` learns which paths are encrypted from the parent's
    /// manifest: a live file matching its saved plaintext is held only then.
    #[cfg(unix)]
    #[test]
    fn streams_reads_the_encrypted_enrollments_of_the_parent_manifest() -> Result<()> {
        use crate::files::{FileMode, FilePolicy};
        use crate::history::manifest::{Enrollment, Manifest};
        use crate::history::sync::layout::Roots;
        use crate::history::tracked::{TrackedEntry, Walk, normalize};
        use age::secrecy::ExposeSecret;

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let key = age::x25519::Identity::generate();
        let mut environment = mise_util::testing::EnvVarGuard::new();
        environment.set("MISE_AGE_KEY", key.to_string().expose_secret());
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(key.to_public())];

        let roots = Roots::current();
        let scratch = tempfile::Builder::new()
            .prefix(".history-held-enrolled-")
            .tempdir_in(&roots.home)?;
        let file = normalize(scratch.path()).join("secret.txt");
        std::fs::write(&file, "private contents")?;
        let policy = FilePolicy::for_mode(FileMode::Track);
        let entry = TrackedEntry::new(file.clone(), "track", policy);
        let stream = entry.tree_path(&file)?;
        let ciphertext = crate::history::sync::files::encode(
            &stream,
            "100644",
            b"private contents",
            "scheme",
            &recipients,
        )?;
        let tree = repo.write_tree(&[(
            "100644".into(),
            repo.hash_blob(&ciphertext)?,
            stream.clone(),
        )])?;
        let enrolled = Manifest {
            enrollment: vec![Enrollment {
                path: stream.clone(),
                autosave: true,
                encrypt: true,
                allow_plaintext: None,
                variants: vec![],
                exclude: None,
                include: None,
            }],
            ..Default::default()
        }
        .write(&repo, &tree)?;
        let unenrolled = repo.commit_tree(&tree, vec![], "no manifest")?;

        let mut walk = Walk {
            entries: vec![entry],
            ..Default::default()
        };
        walk.files.insert(file, (0, policy));
        let held = Held {
            record: Some(Record::default()),
            upstream: true,
        };
        let result = super::streams(&repo, &held, Some(&enrolled), &walk)?;
        assert_eq!(result.held, BTreeSet::from([stream]));
        assert!(result.carry.is_empty());
        // the same ciphertext without the enrollment is not the live plaintext
        let result = super::streams(&repo, &held, Some(&unenrolled), &walk)?;
        assert!(result.held.is_empty());
        assert_eq!(result.carry, vec![0]);
        Ok(())
    }

    /// A saved file this machine cannot decrypt is not held, even when its
    /// live file is gone: both sides must not read as "nothing".
    #[cfg(unix)]
    #[test]
    fn an_undecryptable_saved_file_with_no_live_file_is_not_held() -> Result<()> {
        use crate::files::{FileMode, FilePolicy};
        use crate::history::checkpoint::test_checkpoint;
        use crate::history::shadow::Overlay;
        use crate::history::sync::layout::Roots;
        use crate::history::tracked::{TrackedEntry, Walk, normalize};
        use age::secrecy::ExposeSecret;

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let key = age::x25519::Identity::generate();
        let mut environment = mise_util::testing::EnvVarGuard::new();
        environment.set("MISE_AGE_KEY", key.to_string().expose_secret());
        // encrypted for someone else, so this machine's key cannot open it
        let other = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn age::Recipient + Send>> = vec![Box::new(other.to_public())];

        let roots = Roots::current();
        let scratch = tempfile::Builder::new()
            .prefix(".history-held-undecryptable-")
            .tempdir_in(&roots.home)?;
        // listed as live in the walk, never created on disk
        let file = normalize(scratch.path()).join("secret.txt");
        let policy = FilePolicy::for_mode(FileMode::Track);
        let entry = TrackedEntry::new(file.clone(), "track", policy);
        let stream = entry.tree_path(&file)?;
        let ciphertext = crate::history::sync::files::encode(
            &stream,
            "100644",
            b"private contents",
            "scheme",
            &recipients,
        )?;
        let overlay = Overlay {
            path: stream.clone(),
            object: Some(("100644".into(), repo.hash_blob(&ciphertext)?)),
        };
        let tree = repo.compose(&repo.empty_object("tree")?, &[overlay])?;
        let saved = repo.write_checkpoint(Some(&tree), &test_checkpoint("saved", Some(&tree)))?;
        let mut walk = Walk {
            entries: vec![entry],
            ..Default::default()
        };
        walk.files.insert(file, (0, policy));
        let encrypted = BTreeSet::from([stream.clone()]);
        assert!(!check(&repo, &saved, &walk, 0, &stream, &encrypted)?);
        Ok(())
    }

    /// A parent whose enrollment manifest cannot be read holds nothing: the
    /// stream is carried and the save goes on, as before the encrypted
    /// paths were loaded once per save.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_manifest_carries_the_stream_instead_of_failing() -> Result<()> {
        use crate::files::{FileMode, FilePolicy};
        use crate::history::checkpoint::test_checkpoint;
        use crate::history::shadow::Overlay;
        use crate::history::sync::layout::Roots;
        use crate::history::tracked::{TrackedEntry, Walk, normalize};

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let roots = Roots::current();
        let scratch = tempfile::Builder::new()
            .prefix(".history-held-manifest-")
            .tempdir_in(&roots.home)?;
        let policy = FilePolicy::for_mode(FileMode::Track);
        // two streams, so the unreadable manifest is met more than once
        let files = ["one.txt", "two.txt"].map(|name| normalize(scratch.path()).join(name));
        let entries = files
            .clone()
            .map(|file| TrackedEntry::new(file, "track", policy));
        let streams = entries
            .iter()
            .zip(&files)
            .map(|(entry, file)| entry.tree_path(file))
            .collect::<Result<Vec<_>>>()?;
        let mut walk = Walk {
            entries: entries.to_vec(),
            ..Default::default()
        };
        for (index, file) in files.iter().enumerate() {
            std::fs::write(file, "plain")?;
            walk.files.insert(file.clone(), (index, policy));
        }
        let held = Held {
            record: Some(Record::default()),
            upstream: true,
        };
        let save = |manifest: Option<&[u8]>| -> Result<String> {
            let mut overlays = streams
                .iter()
                .map(|stream| {
                    Ok(Overlay {
                        path: stream.clone(),
                        object: Some(("100644".into(), repo.hash_blob(b"plain")?)),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if let Some(bytes) = manifest {
                overlays.push(Overlay {
                    path: crate::history::manifest::PATH.into(),
                    object: Some(("100644".into(), repo.hash_blob(bytes)?)),
                });
            }
            let tree = repo.compose(&repo.empty_object("tree")?, &overlays)?;
            repo.write_checkpoint(Some(&tree), &test_checkpoint("saved", Some(&tree)))
        };

        // no manifest: nothing is encrypted and the unchanged file is held
        let plain = save(None)?;
        let result = super::streams(&repo, &held, Some(&plain), &walk)?;
        assert_eq!(result.held, streams.iter().cloned().collect());
        assert!(result.carry.is_empty());

        // a manifest that cannot be parsed is carried, not an error
        let broken = save(Some(b"not a manifest"))?;
        let result = super::streams(&repo, &held, Some(&broken), &walk)?;
        assert!(result.held.is_empty());
        assert_eq!(result.carry, vec![0, 1]);
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
