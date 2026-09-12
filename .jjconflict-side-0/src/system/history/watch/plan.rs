//! Which directories the watcher installs watches on, computed from the
//! tracked set without touching the watcher: a pure function of the
//! entries and what exists on disk.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The directory and everything below it.
    Recursive,
    /// The directory's own entries only (a tracked file's parent, or the
    /// nearest existing ancestor of a path that does not exist yet).
    Flat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    pub path: PathBuf,
    pub mode: Mode,
}

/// What a tracked path is right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathKind {
    Directory,
    File,
    Missing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WatchPlan {
    pub anchors: Vec<Anchor>,
    /// Tracked paths that do not exist yet; their creation is seen through
    /// a flat anchor on the nearest existing ancestor, after which the plan
    /// is rebuilt.
    pub pending: Vec<PathBuf>,
}

impl WatchPlan {
    /// `nearest_existing` answers, for a missing path, which ancestor
    /// exists (injected so the plan stays testable without a filesystem).
    pub(crate) fn build(
        paths: impl IntoIterator<Item = (PathBuf, PathKind)>,
        nearest_existing: impl Fn(&Path) -> Option<PathBuf>,
    ) -> Self {
        let mut anchors: Vec<Anchor> = vec![];
        let mut pending = vec![];
        for (path, kind) in paths {
            match kind {
                PathKind::Directory => anchors.push(Anchor {
                    path,
                    mode: Mode::Recursive,
                }),
                PathKind::File => {
                    if let Some(parent) = path.parent() {
                        anchors.push(Anchor {
                            path: parent.to_path_buf(),
                            mode: Mode::Flat,
                        });
                    }
                }
                PathKind::Missing => {
                    if let Some(ancestor) = nearest_existing(&path) {
                        anchors.push(Anchor {
                            path: ancestor,
                            mode: Mode::Flat,
                        });
                    }
                    pending.push(path);
                }
            }
        }
        anchors.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.mode.cmp(&b.mode)));
        // a recursive anchor covers every anchor below it; on the same path
        // recursive wins over flat
        let mut merged: Vec<Anchor> = vec![];
        for anchor in anchors {
            if merged.iter().any(|kept| {
                kept.mode == Mode::Recursive
                    && (anchor.path == kept.path || anchor.path.starts_with(&kept.path))
            }) {
                continue;
            }
            if let Some(existing) = merged.iter_mut().find(|kept| kept.path == anchor.path) {
                if anchor.mode == Mode::Recursive {
                    existing.mode = Mode::Recursive;
                    // anchors below a newly recursive one are now covered
                    let path = existing.path.clone();
                    merged.retain(|kept| kept.path == path || !kept.path.starts_with(&path));
                }
                continue;
            }
            merged.push(anchor);
        }
        pending.sort();
        pending.dedup();
        Self {
            anchors: merged,
            pending,
        }
    }
}

impl PartialOrd for Mode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Mode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // recursive sorts first so it is kept when paths tie
        let rank = |mode: &Mode| match mode {
            Mode::Recursive => 0,
            Mode::Flat => 1,
        };
        rank(self).cmp(&rank(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn directories_are_recursive_and_files_watch_their_parent() {
        let plan = WatchPlan::build(
            [
                (p("/home/u/.config/hypr"), PathKind::Directory),
                (p("/home/u/.zshrc"), PathKind::File),
                (p("/home/u/.gitconfig"), PathKind::File),
            ],
            |_| None,
        );
        assert_eq!(
            plan.anchors,
            vec![
                Anchor {
                    path: p("/home/u"),
                    mode: Mode::Flat
                },
                Anchor {
                    path: p("/home/u/.config/hypr"),
                    mode: Mode::Recursive
                },
            ]
        );
        assert!(plan.pending.is_empty());
    }

    #[test]
    fn a_recursive_anchor_absorbs_anchors_below_it() {
        let plan = WatchPlan::build(
            [
                (p("/home/u/.config/hypr/bindings.lua"), PathKind::File),
                (p("/home/u/.config"), PathKind::Directory),
                (p("/home/u/.config/mise"), PathKind::Directory),
            ],
            |_| None,
        );
        assert_eq!(
            plan.anchors,
            vec![Anchor {
                path: p("/home/u/.config"),
                mode: Mode::Recursive
            }]
        );
    }

    #[test]
    fn missing_paths_are_pending_and_watched_through_an_ancestor() {
        let plan = WatchPlan::build(
            [(p("/home/u/.config/later/file"), PathKind::Missing)],
            |path| {
                assert_eq!(path, Path::new("/home/u/.config/later/file"));
                Some(p("/home/u/.config"))
            },
        );
        assert_eq!(
            plan.anchors,
            vec![Anchor {
                path: p("/home/u/.config"),
                mode: Mode::Flat
            }]
        );
        assert_eq!(plan.pending, vec![p("/home/u/.config/later/file")]);
    }

    #[test]
    fn recursive_wins_over_flat_on_the_same_path() {
        let plan = WatchPlan::build(
            [
                (p("/home/u/.config/a"), PathKind::File),
                (p("/home/u/.config"), PathKind::Directory),
            ],
            |_| None,
        );
        assert_eq!(plan.anchors.len(), 1);
        assert_eq!(plan.anchors[0].mode, Mode::Recursive);
    }
}
