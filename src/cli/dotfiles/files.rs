use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use serde::Serialize;

use crate::file::display_path;
use crate::system::history::tracked::{ExclusionReasons, TrackedSet, Walk};
use crate::ui::table::MiseTable;

/// List a directory's files and whether each one is tracked
///
/// Shows each file and subdirectory with its tracking state: `tracked` (with
/// the entry that covers it), `excluded` (with the rule that leaves it out),
/// `omitted` (covered, but a capture skips it, such as a credential-like
/// file), or `untracked`. A subdirectory shows how many tracked files it
/// holds, and is `partial` when it is not tracked itself, or is excluded, but
/// holds tracked files. A path kept in local-only history is one `local` row,
/// listed in full by `mise dot --local files PATH`. `--tracked` and
/// `--untracked` split the rows: `partial` and `local` rows are listed by
/// `--tracked`. Relative paths are read from the current directory.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        "mise dot files",
        help = "Show what is tracked in the current directory"
    ),
    example(
        "mise dot files ~/.config/hypr",
        help = "Show what is tracked in ~/.config/hypr"
    ),
    example(
        "mise dot files -r --untracked ~/.config/nvim",
        help = "List every file under ~/.config/nvim that history does not save"
    )
)]
pub(crate) struct DotfilesFiles {
    /// The directory or file to list; defaults to the current directory
    #[usage(value_name = "PATH")]
    path: Option<PathBuf>,

    /// List files in subdirectories too, instead of one row per subdirectory
    #[usage(long, short)]
    recursive: bool,

    /// Only show what is tracked
    #[usage(long)]
    tracked: bool,

    /// Only show what holds nothing tracked
    #[usage(long)]
    untracked: bool,

    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Tracked,
    Partial,
    Local,
    Excluded,
    Omitted,
    Untracked,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tracked => "tracked",
            Self::Partial => "partial",
            Self::Local => "local",
            Self::Excluded => "excluded",
            Self::Omitted => "omitted",
            Self::Untracked => "untracked",
        }
    }
}

#[derive(Debug, Serialize)]
struct Row {
    path: String,
    #[serde(skip)]
    name: String,
    kind: &'static str,
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    autosave: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// For a directory, how many files below it are tracked.
    #[serde(skip_serializing_if = "Option::is_none")]
    tracked_files: Option<usize>,
    /// For a directory, whether local-only history keeps a path below it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    local_below: bool,
}

impl DotfilesFiles {
    pub(crate) async fn run(self) -> Result<()> {
        if self.tracked && self.untracked {
            bail!("--tracked and --untracked cannot be combined");
        }
        let target = self.target()?;
        // a symlink is listed as itself, as history tracks it, and a
        // dangling one is still something to list
        let meta = std::fs::symlink_metadata(&target)
            .map_err(|err| eyre::eyre!("{}: {err}", display_path(&target)))?;
        // a path kept in local-only history is listed from that history
        if super::route_local(std::slice::from_ref(&target)).await? {
            return Ok(());
        }
        let tracked = TrackedSet::effective().await?;
        // only the entries that overlap the listed path need walking
        let selected: Vec<usize> = tracked
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.path.starts_with(&target) || target.starts_with(&entry.path))
            .map(|(index, _)| index)
            .collect();
        let walk = tracked.walk_selected(&selected)?;
        walk.report_warnings();
        let lister = Lister {
            set: &tracked,
            reasons: tracked.exclusion_reasons()?,
            omitted: walk
                .omitted
                .iter()
                .map(|omitted| (omitted.path.clone(), omitted.reason.clone()))
                .collect(),
            walk: &walk,
        };

        let mut rows = vec![];
        if meta.is_dir() {
            lister.list_dir(&target, &target, self.recursive, &mut rows)?;
        } else {
            rows.push(lister.row(&target, display_path(&target), false));
        }
        rows.retain(|row| self.shows(row));

        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&rows)?);
            return Ok(());
        }
        if rows.is_empty() {
            info!("nothing to list in {}", display_path(&target));
            return Ok(());
        }
        let mut table = MiseTable::new(false, &["Status", "Path", "Details"]);
        for row in &rows {
            table.add_row(vec![
                row.status.as_str().to_string(),
                row.name.clone(),
                details(row),
            ]);
        }
        table.print()?;
        Ok(())
    }

    /// The listed path: `~` expanded, read from the current directory when
    /// relative, and resolved the way tracked entries are, which keeps a
    /// symlink at the end as the link rather than its destination.
    fn target(&self) -> Result<PathBuf> {
        let raw = match &self.path {
            Some(path) => crate::system::files::resolve_target_arg(&path.to_string_lossy()),
            None => PathBuf::from("."),
        };
        let absolute = if raw.is_relative() {
            std::env::current_dir()?.join(raw)
        } else {
            raw
        };
        Ok(crate::system::history::tracked::normalize_target(&absolute))
    }

    fn shows(&self, row: &Row) -> bool {
        if self.tracked {
            matches!(
                row.status,
                Status::Tracked | Status::Partial | Status::Local
            )
        } else if self.untracked {
            // the two filters split the rows: a directory holding tracked
            // files is `--tracked`'s, so this one finds what holds none
            !matches!(
                row.status,
                Status::Tracked | Status::Partial | Status::Local
            )
        } else {
            true
        }
    }
}

struct Lister<'a> {
    set: &'a TrackedSet,
    reasons: ExclusionReasons<'a>,
    walk: &'a Walk,
    /// The walk's omissions by displayed path.
    omitted: BTreeMap<String, String>,
}

impl Lister<'_> {
    fn list_dir(
        &self,
        root: &Path,
        dir: &Path,
        recursive: bool,
        rows: &mut Vec<Row>,
    ) -> Result<()> {
        let read =
            std::fs::read_dir(dir).and_then(|entries| entries.collect::<std::io::Result<Vec<_>>>());
        let mut children = match read {
            Ok(children) => children,
            // the directory asked for has to be readable; one below it
            // that is not is reported, and the rest is still listed
            Err(err) if dir != root => {
                warn!("{}: {err}", display_path(dir));
                return Ok(());
            }
            Err(err) => return Err(eyre::eyre!("{}: {err}", display_path(dir))),
        };
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let path = child.path();
            // a symlink is listed as itself, as history tracks it
            let is_dir = child.file_type().is_ok_and(|kind| kind.is_dir());
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let mut name = relative.to_string_lossy().into_owned();
            // a local-only path is one row: its files are that history's
            // to judge, which this listing cannot ask
            if is_dir && recursive && !self.is_local(&path) {
                // `.git` is never captured, and walking it is all cost
                if child.file_name() != ".git" {
                    self.list_dir(root, &path, recursive, rows)?;
                }
                continue;
            }
            if is_dir {
                name.push('/');
            }
            rows.push(self.row(&path, name, is_dir));
        }
        Ok(())
    }

    fn row(&self, path: &Path, name: String, is_dir: bool) -> Row {
        let mut row = Row {
            path: display_path(path),
            name,
            kind: if is_dir {
                "directory"
            } else if path.is_symlink() {
                "symlink"
            } else {
                "file"
            },
            status: Status::Untracked,
            entry: None,
            mode: None,
            autosave: None,
            reason: None,
            tracked_files: None,
            local_below: false,
        };
        // local-only history is not in this set, and its own rules decide
        // what in it is saved: say whose the path is, not what it holds
        if let Some(local) = self.set.local.iter().find(|local| path.starts_with(local)) {
            row.status = Status::Local;
            row.entry = Some(display_path(local));
            row.mode = Some("track-local".into());
            return row;
        }
        if is_dir {
            let count = self.tracked_below(path);
            row.tracked_files = Some(count);
            row.local_below = self.set.local.iter().any(|local| local.starts_with(path));
            // whether anything below is saved, by this history or the
            // local-only one: such a directory is partial whatever covers it
            let holds_tracked = count > 0 || row.local_below;
            // a covered directory the capture could not read, unless an
            // entry of its own below it is still saved
            if let Some(reason) = self.omitted.get(&row.path) {
                row.status = if holds_tracked {
                    Status::Partial
                } else {
                    Status::Omitted
                };
                row.reason = Some(reason.clone());
                return row;
            }
            match self.set.entry_for(path) {
                Some(owner) => match self.reasons.of(path) {
                    // an entry of its own below an excluded directory still
                    // saves something in it
                    Some(reason) => {
                        row.status = if holds_tracked {
                            Status::Partial
                        } else {
                            Status::Excluded
                        };
                        row.reason = Some(reason.to_string());
                    }
                    None => {
                        row.status = Status::Tracked;
                        row.entry = Some(owner.display());
                        row.mode = Some(owner.mode.clone());
                        row.autosave = Some(owner.policy.autosave);
                    }
                },
                None if holds_tracked => row.status = Status::Partial,
                None => {}
            }
            return row;
        }
        if let Some((owner, policy)) = self.walk.files.get(path) {
            row.status = Status::Tracked;
            if let Some(entry) = self.walk.entries.get(*owner) {
                row.entry = Some(entry.display());
                row.mode = Some(entry.mode.clone());
            }
            row.autosave = Some(policy.autosave);
            return row;
        }
        if let Some(reason) = self.omitted.get(&row.path) {
            row.status = Status::Omitted;
            row.reason = Some(reason.clone());
            return row;
        }
        if self.set.entry_for(path).is_some() {
            match self.reasons.of(path) {
                Some(reason) => {
                    row.status = Status::Excluded;
                    row.reason = Some(reason.to_string());
                }
                None => {
                    row.status = Status::Omitted;
                    row.reason = Some("not captured".to_string());
                }
            }
        }
        row
    }

    fn is_local(&self, path: &Path) -> bool {
        self.set.local.iter().any(|local| path.starts_with(local))
    }

    /// How many captured files lie below `dir`. Descendants sort right after
    /// their directory, so they are one contiguous range of the map.
    fn tracked_below(&self, dir: &Path) -> usize {
        self.walk
            .files
            .range(dir.to_path_buf()..)
            .take_while(|(path, _)| path.starts_with(dir))
            .filter(|(path, _)| path.as_path() != dir)
            .count()
    }
}

fn details(row: &Row) -> String {
    match row.status {
        Status::Tracked => {
            let entry = row.entry.clone().unwrap_or_default();
            let mut how = row.mode.clone().unwrap_or_else(|| "track".into());
            // local-only history's policy is that history's to report
            match row.autosave {
                Some(true) => how.push_str(", autosave"),
                Some(false) => how.push_str(", manual save"),
                None => {}
            }
            match row.tracked_files {
                Some(count) => format!("{} · {entry} ({how})", files(count)),
                None => format!("{entry} ({how})"),
            }
        }
        Status::Partial => {
            let count = row.tracked_files.unwrap_or(0);
            match (count, row.local_below) {
                (0, true) => "local-only history below".to_string(),
                (count, true) => format!("{} tracked below, and local-only history", files(count)),
                (count, false) => format!("{} tracked below", files(count)),
            }
        }
        Status::Local => format!(
            "{} (track-local); `mise dot --local files` lists it",
            row.entry.clone().unwrap_or_default()
        ),
        Status::Excluded | Status::Omitted => row.reason.clone().unwrap_or_default(),
        Status::Untracked => String::new(),
    }
}

fn files(count: usize) -> String {
    if count == 1 {
        "1 file".into()
    } else {
        format!(
            "{} files",
            crate::system::history::tracked::with_separators(count)
        )
    }
}
