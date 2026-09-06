//! The per-generation journal: what a bootstrap run changed, recorded
//! before each mutation so a later rollback can invert it.
//!
//! The journal is write-ahead. For every path an apply is about to touch,
//! [`begin_changes`] captures the path's prior state as a [`PathSnapshot`]
//! and appends a `PathChanged` entry *before* the mutation; afterwards
//! [`commit_changes`] appends a `Committed` entry carrying the path's
//! resulting [`PathState`]. A `PathChanged` without a matching `Committed`
//! means the run died mid-mutation and the path's real state must be
//! inspected before anything is undone. Both entries are tiny except for
//! the prior content, which lives in a content-addressed [`Blob`].

use std::path::{Path, PathBuf};

use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::dirs;
use crate::file::{self, display_path};

/// Prior content at or below this size is embedded in the record itself.
pub(crate) const BLOB_INLINE_MAX: u64 = 64 * 1024;
/// Prior content above this size is not captured.
pub(crate) const BLOB_MAX: u64 = 8 * 1024 * 1024;
/// A replaced directory holding more than this is not captured.
pub(crate) const DIR_SNAPSHOT_MAX: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum JournalEntry {
    /// Context for someone reading `mise bootstrap dotfiles history show`.
    Note { message: String },
    /// Written before `path` is mutated on behalf of `item` (a dotfile
    /// target, an edit key) in bootstrap part `part`.
    PathChanged {
        part: String,
        item: String,
        path: PathBuf,
        prior: PathSnapshot,
    },
    /// The mutation recorded at index `seq` finished; `after` is what the
    /// path looks like now.
    Committed { seq: u32, after: PathState },
    /// Written by a newer mise than this one.
    #[serde(other)]
    Unknown,
}

impl JournalEntry {
    /// One-line rendering for tables and `show`.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Note { message } => message.clone(),
            Self::PathChanged {
                part,
                item,
                path,
                prior,
            } => format!(
                "{part}: {item}: {} was {}",
                display_path(path),
                prior.describe()
            ),
            Self::Committed { seq, after } => format!("#{seq} now {}", after.describe()),
            Self::Unknown => "recorded by a newer mise".to_string(),
        }
    }
}

/// Renders a journal for humans, folding each `Committed` into the
/// `PathChanged` it completes: `dotfiles: ~/.zshrc: missing -> symlink`.
pub(crate) fn render(entries: &[JournalEntry]) -> Vec<String> {
    let mut lines: Vec<(u32, String)> = vec![];
    for (index, entry) in entries.iter().enumerate() {
        match entry {
            JournalEntry::PathChanged {
                part,
                item,
                path,
                prior,
            } => lines.push((
                index as u32,
                format!(
                    "{part}: {item}: {} {} -> (not finished)",
                    display_path(path),
                    prior.describe()
                ),
            )),
            JournalEntry::Committed { seq, after } => {
                if let Some((_, line)) = lines.iter_mut().find(|(s, _)| s == seq) {
                    *line = line.replace("-> (not finished)", &format!("-> {}", after.describe()));
                }
            }
            other => lines.push((index as u32, other.describe())),
        }
    }
    lines.into_iter().map(|(_, line)| line).collect()
}

/// Content-addressed bytes. Small content is inlined into the record;
/// larger content is a git blob in the history repository, referenced from
/// the checkpoint's `blobs/<sha256>` tree entry (or, when no repository is
/// usable, a file under the index directory).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Blob {
    pub sha256: String,
    pub size: u64,
    /// base64 of the bytes when they fit [`BLOB_INLINE_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline: Option<String>,
    /// The git blob id when the content is in the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
}

/// Sidecar content for the case without a usable git.
pub(crate) fn blobs_dir_in(state_dir: &Path) -> PathBuf {
    super::store::index_dir_in(state_dir).join("blobs")
}

impl Blob {
    pub(crate) fn store_in(state_dir: &Path, bytes: &[u8]) -> Result<Self> {
        use base64::Engine;
        use sha2::Digest;

        let sha256 = hex::encode(sha2::Sha256::digest(bytes));
        let size = bytes.len() as u64;
        if size <= BLOB_INLINE_MAX {
            return Ok(Self {
                sha256,
                size,
                inline: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                git: None,
            });
        }
        if let Some(oid) = super::scope::store_blob(&sha256, bytes)? {
            return Ok(Self {
                sha256,
                size,
                inline: None,
                git: Some(oid),
            });
        }
        let dir = blobs_dir_in(state_dir);
        // preimages hold whatever the files held: private, like the store
        super::store::create_private_dir(&dir)?;
        let path = dir.join(&sha256);
        if !path.exists() {
            file::write_atomic(&path, bytes)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self {
            sha256,
            size,
            inline: None,
            git: None,
        })
    }
}

/// What was at a path before a mutation, captured completely enough to
/// put it back.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum PathSnapshot {
    Missing,
    File {
        content: Blob,
        mode: u32,
    },
    Symlink {
        dest: PathBuf,
    },
    /// A directory that is about to be replaced wholesale (`--force`),
    /// captured with its whole contents.
    Dir {
        files: Vec<DirFileSnapshot>,
        links: Vec<DirLinkSnapshot>,
        /// every subdirectory, empty ones included, with its mode
        #[serde(default)]
        dirs: Vec<DirDirSnapshot>,
        mode: u32,
    },
    /// A directory that stays a directory (something inside it changes, or
    /// it may be pruned once empty), so only its existence and mode are
    /// captured, never its contents.
    Directory {
        mode: u32,
    },
    /// Existed but could not be captured.
    Unrecorded {
        kind: String,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DirFileSnapshot {
    pub rel: PathBuf,
    pub content: Blob,
    pub mode: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DirLinkSnapshot {
    pub rel: PathBuf,
    pub dest: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DirDirSnapshot {
    pub rel: PathBuf,
    pub mode: u32,
}

/// How much of a path to capture before it is touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capture {
    /// Everything, including a directory's contents: the path is replaced.
    Full,
    /// A directory only by existence and mode: it stays a directory.
    Shallow,
}

impl PathSnapshot {
    /// Captures `path` without following symlinks. Never fails: anything
    /// that cannot be read becomes `Unrecorded` with the reason.
    pub(crate) fn capture_with(state_dir: &Path, path: &Path, capture: Capture) -> Self {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::Missing,
            Err(err) => {
                return Self::Unrecorded {
                    kind: "unknown".into(),
                    reason: err.to_string(),
                };
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return match std::fs::read_link(path) {
                Ok(dest) => Self::Symlink { dest },
                Err(err) => Self::Unrecorded {
                    kind: "symlink".into(),
                    reason: err.to_string(),
                },
            };
        }
        if file_type.is_file() {
            return match capture_file(state_dir, path, metadata.len()) {
                Ok(content) => Self::File {
                    content,
                    mode: mode_of(&metadata),
                },
                Err(reason) => Self::Unrecorded {
                    kind: "file".into(),
                    reason,
                },
            };
        }
        if file_type.is_dir() {
            if capture == Capture::Shallow {
                return Self::Directory {
                    mode: mode_of(&metadata),
                };
            }
            return match capture_dir(state_dir, path) {
                Ok((files, links, dirs)) => Self::Dir {
                    files,
                    links,
                    dirs,
                    mode: mode_of(&metadata),
                },
                Err(reason) => Self::Unrecorded {
                    kind: "directory".into(),
                    reason,
                },
            };
        }
        Self::Unrecorded {
            kind: "special file".into(),
            reason: "not a regular file, symlink, or directory".into(),
        }
    }

    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Missing => "missing".into(),
            Self::File { content, .. } => format!("a file ({} bytes)", content.size),
            Self::Symlink { dest } => format!("a symlink to {}", display_path(dest)),
            Self::Dir { files, links, .. } => {
                format!("a directory ({} files, {} links)", files.len(), links.len())
            }
            Self::Directory { .. } => "a directory".into(),
            Self::Unrecorded { kind, reason } => format!("{kind} (not captured: {reason})"),
        }
    }
}

fn capture_file(state_dir: &Path, path: &Path, size: u64) -> std::result::Result<Blob, String> {
    if size > BLOB_MAX {
        return Err(format!(
            "{} bytes is over the {} MiB limit",
            size,
            BLOB_MAX / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    Blob::store_in(state_dir, &bytes).map_err(|err| format!("{err:#}"))
}

type DirCapture = (
    Vec<DirFileSnapshot>,
    Vec<DirLinkSnapshot>,
    Vec<DirDirSnapshot>,
);

fn capture_dir(state_dir: &Path, dir: &Path) -> std::result::Result<DirCapture, String> {
    let mut files = vec![];
    let mut links = vec![];
    let mut dirs = vec![];
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|err| err.to_string())?;
        let rel = match entry.path().strip_prefix(dir) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel.to_path_buf(),
            _ => continue,
        };
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            let dest = std::fs::read_link(entry.path()).map_err(|err| err.to_string())?;
            links.push(DirLinkSnapshot { rel, dest });
        } else if file_type.is_file() {
            let metadata = entry.metadata().map_err(|err| err.to_string())?;
            total += metadata.len();
            if total > DIR_SNAPSHOT_MAX {
                return Err(format!(
                    "more than {} MiB",
                    DIR_SNAPSHOT_MAX / (1024 * 1024)
                ));
            }
            let content = capture_file(state_dir, entry.path(), metadata.len())
                .map_err(|reason| format!("{}: {reason}", display_path(entry.path())))?;
            files.push(DirFileSnapshot {
                rel,
                content,
                mode: mode_of(&metadata),
            });
        } else if file_type.is_dir() {
            let metadata = entry.metadata().map_err(|err| err.to_string())?;
            dirs.push(DirDirSnapshot {
                rel,
                mode: mode_of(&metadata),
            });
        } else {
            return Err(format!("{} is a special file", display_path(entry.path())));
        }
    }
    Ok((files, links, dirs))
}

/// Identity of a path after a mutation, enough to tell later whether it
/// is still in the state the run left it in.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum PathState {
    Missing,
    File {
        sha256: String,
        mode: u32,
    },
    Symlink {
        dest: PathBuf,
    },
    /// with how many entries it holds, so additions since are detectable
    Dir {
        entries: u64,
    },
    Other {
        kind: String,
    },
}

impl PathState {
    pub(crate) fn observe(path: &Path) -> Self {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::Missing,
            Err(err) => {
                return Self::Other {
                    kind: format!("unreadable: {err}"),
                };
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return match std::fs::read_link(path) {
                Ok(dest) => Self::Symlink { dest },
                Err(err) => Self::Other {
                    kind: format!("unreadable symlink: {err}"),
                },
            };
        }
        if file_type.is_file() {
            return match hash_file(path) {
                Ok(sha256) => Self::File {
                    sha256,
                    mode: mode_of(&metadata),
                },
                Err(err) => Self::Other {
                    kind: format!("unreadable file: {err}"),
                },
            };
        }
        if file_type.is_dir() {
            return match std::fs::read_dir(path) {
                Ok(entries) => Self::Dir {
                    entries: entries.count() as u64,
                },
                Err(err) => Self::Other {
                    kind: format!("unreadable directory: {err}"),
                },
            };
        }
        Self::Other {
            kind: "special file".into(),
        }
    }

    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Missing => "missing".into(),
            Self::File { sha256, .. } => {
                format!("a file ({})", sha256.get(..7).unwrap_or(sha256))
            }
            Self::Symlink { dest } => format!("a symlink to {}", display_path(dest)),
            Self::Dir { entries } => format!("a directory ({entries} entries)"),
            Self::Other { kind } => kind.clone(),
        }
    }
}

#[cfg(unix)]
/// The sha256 of a file, streamed so a large managed file is never read
/// into memory whole.
fn hash_file(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn mode_of(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(_metadata: &std::fs::Metadata) -> u32 {
    0
}

/// A `PathChanged` entry awaiting its `Committed`.
#[derive(Debug)]
pub(crate) struct PendingChange {
    seq: u32,
    path: PathBuf,
}

/// Captures every path's prior state and records a `PathChanged` for each,
/// before the caller mutates them. Returns nothing when no generation is
/// open, so inactive runs pay no capture cost.
pub(crate) fn begin_changes(
    part: &str,
    item: &str,
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<Vec<PendingChange>> {
    begin_changes_with(
        part,
        item,
        paths.into_iter().map(|path| (path, Capture::Full)),
    )
}

/// [`begin_changes`] with a capture depth per path. Fails when an entry
/// cannot be persisted: the mutation must not go ahead without its
/// write-ahead record.
pub(crate) fn begin_changes_with(
    part: &str,
    item: &str,
    paths: impl IntoIterator<Item = (PathBuf, Capture)>,
) -> Result<Vec<PendingChange>> {
    if !super::scope::is_active() {
        return Ok(vec![]);
    }
    let mut pending = vec![];
    for (path, capture) in paths {
        let prior = PathSnapshot::capture_with(&dirs::STATE, &path, capture);
        if let Some(seq) = super::scope::record(JournalEntry::PathChanged {
            part: part.to_string(),
            item: item.to_string(),
            path: path.clone(),
            prior,
        })? {
            pending.push(PendingChange { seq, path });
        }
    }
    Ok(pending)
}

/// Records the resulting state of each pending change.
pub(crate) fn commit_changes(pending: Vec<PendingChange>) {
    for change in pending {
        // the mutation already happened; a missing `committed` is handled by
        // inspecting the path later, so this is a warning
        if let Err(err) = super::scope::record(JournalEntry::Committed {
            seq: change.seq,
            after: PathState::observe(&change.path),
        }) {
            warn!("history: {err:#}");
        }
    }
}

/// Records a free-form note on the open generation.
pub(crate) fn note(message: impl Into<String>) {
    if let Err(err) = super::scope::record(JournalEntry::Note {
        message: message.into(),
    }) {
        warn!("history: {err:#}");
    }
}

/// Records that `part` changed `item` in a way rollback cannot undo.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_inline_small_and_store_large_content() {
        let tmp = tempfile::tempdir().unwrap();
        let small = Blob::store_in(tmp.path(), b"hello").unwrap();
        assert_eq!(small.size, 5);
        assert!(small.inline.is_some());
        let big = vec![7u8; BLOB_INLINE_MAX as usize + 1];
        let stored = Blob::store_in(tmp.path(), &big).unwrap();
        assert!(stored.inline.is_none());
        // no operation is open here, so the content lands in the sidecar dir
        let on_disk = blobs_dir_in(tmp.path()).join(&stored.sha256);
        assert_eq!(std::fs::read(&on_disk).unwrap(), big);
        // content-addressed: storing again is a no-op with the same id
        let again = Blob::store_in(tmp.path(), &big).unwrap();
        assert_eq!(again, stored);
    }

    #[test]
    fn snapshots_capture_each_kind_of_path() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let file = tmp.path().join("file");
        std::fs::write(&file, "content").unwrap();
        assert!(matches!(
            PathSnapshot::capture_with(&state, &tmp.path().join("nope"), Capture::Full),
            PathSnapshot::Missing
        ));
        match PathSnapshot::capture_with(&state, &file, Capture::Full) {
            PathSnapshot::File { content, .. } => assert_eq!(content.size, 7),
            other => panic!("{other:?}"),
        }
        #[cfg(unix)]
        {
            let link = tmp.path().join("link");
            std::os::unix::fs::symlink("file", &link).unwrap();
            assert!(matches!(
                PathSnapshot::capture_with(&state, &link, Capture::Full),
                PathSnapshot::Symlink { dest } if dest == Path::new("file")
            ));
            nix::unistd::mkfifo(&tmp.path().join("fifo"), nix::sys::stat::Mode::S_IRWXU).unwrap();
            assert!(matches!(
                PathSnapshot::capture_with(&state, &tmp.path().join("fifo"), Capture::Full),
                PathSnapshot::Unrecorded { .. }
            ));
        }
        let dir = tmp.path().join("dir/nested");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), "a").unwrap();
        std::fs::create_dir_all(tmp.path().join("dir/empty")).unwrap();
        match PathSnapshot::capture_with(&state, &tmp.path().join("dir"), Capture::Full) {
            PathSnapshot::Dir { files, dirs, .. } => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].rel, Path::new("nested/a"));
                let mut names: Vec<_> = dirs.iter().map(|d| d.rel.clone()).collect();
                names.sort();
                assert_eq!(names, vec![PathBuf::from("empty"), PathBuf::from("nested")]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            PathSnapshot::capture_with(&state, &tmp.path().join("dir"), Capture::Shallow),
            PathSnapshot::Directory { .. }
        ));
        let large = tmp.path().join("large");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(BLOB_MAX + 1)
            .unwrap();
        assert!(matches!(
            PathSnapshot::capture_with(&state, &large, Capture::Full),
            PathSnapshot::Unrecorded { kind, .. } if kind == "file"
        ));
    }

    #[test]
    fn path_state_tracks_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        assert_eq!(PathState::observe(&file), PathState::Missing);
        std::fs::write(&file, "one").unwrap();
        let first = PathState::observe(&file);
        std::fs::write(&file, "two").unwrap();
        assert_ne!(PathState::observe(&file), first);
        std::fs::write(&file, "one").unwrap();
        assert_eq!(PathState::observe(&file), first);
        assert!(matches!(
            PathState::observe(tmp.path()),
            PathState::Dir { entries: 1 }
        ));
        std::fs::write(tmp.path().join("second"), "").unwrap();
        assert!(matches!(
            PathState::observe(tmp.path()),
            PathState::Dir { entries: 2 }
        ));
    }

    #[test]
    fn render_folds_committed_into_its_change() {
        let entries = vec![
            JournalEntry::PathChanged {
                part: "dotfiles".into(),
                item: "~/.zshrc".into(),
                path: PathBuf::from("/home/u/.zshrc"),
                prior: PathSnapshot::Missing,
            },
            JournalEntry::Note {
                message: "hi".into(),
            },
            JournalEntry::Committed {
                seq: 0,
                after: PathState::Symlink {
                    dest: PathBuf::from("/home/u/.dotfiles/zshrc"),
                },
            },
        ];
        let lines = render(&entries);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("missing -> a symlink to"), "{}", lines[0]);
        assert_eq!(lines[1], "hi");
        let entry: JournalEntry = serde_json::from_str(
            r#"{"kind":"committed","seq":3,"after":{"state":"dir","entries":2}}"#,
        )
        .unwrap();
        assert!(matches!(entry, JournalEntry::Committed { seq: 3, .. }));
        // a record without `dirs` (written before they were captured) still loads
        let old: PathSnapshot =
            serde_json::from_str(r#"{"state":"dir","files":[],"links":[],"mode":493}"#).unwrap();
        assert!(matches!(old, PathSnapshot::Dir { .. }));
    }
}
