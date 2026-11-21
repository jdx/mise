use std::fs;
use std::fs::{read_dir, remove_file};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use eyre::Result;

use crate::config::{Settings, SettingsExt};
use crate::dirs::{TRACKED_CONFIGS, TRACKED_STUBS};
use crate::file::{create_dir_all, make_symlink_or_file};
use crate::hash::hash_to_str;

/// How old an entry's timestamp may get before loading its file rewrites it.
/// The timestamp only needs to be accurate to well within
/// `prune.forget_unused_after`, and rewriting at most daily keeps a write off
/// the path every command takes.
const REFRESH_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// How long an entry outlives the file it records. A project can be gone for a
/// while (an unmounted volume, a branch switch, a re-clone) and come back, so
/// its entry is kept, though it protects nothing while the file is missing.
const MISSING_GRACE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub struct Tracker {}

impl Tracker {
    pub(crate) fn track(path: &Path) -> Result<()> {
        Self::track_in(&TRACKED_CONFIGS, path, SystemTime::now())
    }

    pub fn track_stub(path: &Path) -> Result<()> {
        Self::track_in(&TRACKED_STUBS, path, SystemTime::now())
    }

    /// Records that `path` was used at `now`. An entry's own modification time
    /// is when its file was last used, so an existing entry is rewritten once it
    /// is a day stale.
    fn track_in(dir: &Path, path: &Path, now: SystemTime) -> Result<()> {
        let tracking_path = dir.join(hash_to_str(&path));
        if !tracking_path.exists() {
            create_dir_all(dir)?;
            make_symlink_or_file(path, &tracking_path)?;
        } else if last_used(&tracking_path)
            .is_none_or(|used| now.duration_since(used).unwrap_or_default() >= REFRESH_AFTER)
        {
            // The entry's own time, not its file's: the config is not changed.
            crate::file::set_symlink_times(&tracking_path, None, Some(now))?;
        }
        Ok(())
    }

    /// The tracked configs, after forgetting those that have been gone for a
    /// week or unused for longer than `prune.forget_unused_after`.
    pub fn list_all() -> Result<Vec<PathBuf>> {
        Self::list_all_in(&TRACKED_CONFIGS, Self::max_age(), SystemTime::now())
    }

    pub(crate) fn list_all_stubs() -> Result<Vec<PathBuf>> {
        Self::list_all_in(&TRACKED_STUBS, Self::max_age(), SystemTime::now())
    }

    /// `prune.forget_unused_after`, but no less than `REFRESH_AFTER`: a use is
    /// only recorded once an entry is that stale, so a shorter age would forget
    /// a file that was just loaded.
    fn max_age() -> Option<Duration> {
        Settings::get()
            .prune_forget_unused_after_duration()
            .unwrap_or_else(|err| {
                warn!("invalid prune.forget_unused_after: {err:#}");
                None
            })
            .map(|max_age| max_age.max(REFRESH_AFTER))
    }

    fn list_all_in(dir: &Path, max_age: Option<Duration>, now: SystemTime) -> Result<Vec<PathBuf>> {
        let mut output = vec![];
        if !dir.exists() {
            return Ok(output);
        }
        // Entries written before mise expired them carry their creation time,
        // which says nothing about when they were last used, so their age counts
        // from when this mise first read the list.
        let since = expiry_start(dir, now);
        for entry in read_dir(dir)? {
            let entry = entry?.path();
            // An entry another mise is rewriting can vanish between the listing
            // and the read; it is not this reader's to fail on.
            let path = match Self::tracked_path(&entry) {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(err) => {
                    debug!("skipping tracking entry {}: {err}", entry.display());
                    continue;
                }
            };
            let unused_for = || {
                let used = last_used(&entry).map_or(since, |used| used.max(since));
                now.duration_since(used).unwrap_or_default()
            };
            // Only a regular file can be one of these. A tracked path that is
            // gone, a device or a directory protects nothing, since no reader
            // can load it (#12246), so it is not listed; its entry is kept for
            // a while in case the file comes back.
            let is_file = path.is_file();
            let expired = if is_file {
                max_age.is_some_and(|max_age| unused_for() > max_age)
            } else {
                unused_for() > MISSING_GRACE
            };
            if expired {
                if let Err(err) = remove_file(&entry) {
                    debug!("failed to forget {}: {err}", entry.display());
                }
                continue;
            }
            if is_file {
                output.push(path);
            }
        }
        Ok(output)
    }

    /// The path an entry records, or `None` when it is not one mise wrote.
    ///
    /// Entries are symlinks on unix and, since Windows symlinks need a
    /// privilege mise does not require, plain files holding the path there —
    /// see `file::make_symlink_or_file`. Both forms have to be resolved before
    /// anything can be decided about what they point at.
    fn tracked_path(entry: &Path) -> Result<Option<PathBuf>> {
        if entry.is_symlink() {
            Ok(Some(fs::read_link(entry)?))
        } else if cfg!(target_os = "windows") {
            Ok(Some(PathBuf::from(fs::read_to_string(entry)?.trim())))
        } else {
            Ok(None)
        }
    }

    pub fn clean() -> Result<()> {
        Self::clean_in(&TRACKED_CONFIGS)?;
        Self::clean_in(&TRACKED_STUBS)
    }

    fn clean_in(dir: &Path) -> Result<()> {
        if dir.is_dir() {
            for entry in read_dir(dir)? {
                let entry = entry?.path();
                // Resolve first. Asking whether the *entry* exists answers the
                // wrong question: a symlink to `/dev/null` exists, so it
                // survived every prune while every reader kept failing on it,
                // and on Windows the entry is a plain file that always exists
                // no matter what it records.
                let keep = match Self::tracked_path(&entry)? {
                    Some(path) => path.is_file(),
                    None => false,
                };
                if !keep {
                    remove_file(&entry)?;
                }
            }
        }
        Ok(())
    }
}

/// When the file an entry records was last used: the entry's own modification
/// time, read without following the link.
fn last_used(entry: &Path) -> Option<SystemTime> {
    fs::symlink_metadata(entry).ok()?.modified().ok()
}

remove_by!("2027.6.0", "tracked-entry-expiry-start");

/// When mise started expiring the entries in `dir`, recorded the first time it
/// reads them. mise 2026.11.12 and earlier never refreshed an entry, so an
/// entry's own time is no guide to its use until it has been loaded since.
fn expiry_start(dir: &Path, now: SystemTime) -> SystemTime {
    let marker = dir.with_extension("expiry-start");
    if let Some(start) = last_used(&marker) {
        return start;
    }
    if let Err(err) = fs::write(&marker, "") {
        debug!("failed to write {}: {err}", marker.display());
    }
    now
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    /// `track_in` writes a symlink on unix and a plain file on Windows, so going
    /// through it keeps these tests meaningful on both.
    fn track(dir: &Path, target: &Path) -> PathBuf {
        Tracker::track_in(dir, target, SystemTime::now()).unwrap();
        dir.join(hash_to_str(&target))
    }

    fn list(dir: &Path) -> Vec<PathBuf> {
        Tracker::list_all_in(dir, None, SystemTime::now()).unwrap()
    }

    /// Backdates an entry's own time, as if its file was last used `ago`.
    fn set_last_used(entry: &Path, ago: Duration) {
        crate::file::set_symlink_times(entry, None, Some(SystemTime::now() - ago)).unwrap();
    }

    fn config(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        let config = tmp.path().join(name);
        fs::write(&config, "[tools]\n").unwrap();
        config
    }

    #[test]
    fn keeps_an_entry_pointing_at_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = config(&tmp, "mise.toml");

        let entry = track(&dir, &config);
        Tracker::clean_in(&dir).unwrap();

        assert!(entry.exists(), "a real config file must survive a clean");
        assert_eq!(list(&dir), vec![config]);
    }

    #[test]
    fn removes_an_entry_pointing_at_something_that_is_not_a_file() {
        // Stands in for the `/dev/null` entries older versions left behind: the
        // target exists, so asking `entry.exists()` kept them forever while
        // every reader failed on them. A directory reproduces that portably.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let not_a_config = tmp.path().join("somewhere");
        fs::create_dir(&not_a_config).unwrap();

        let entry = track(&dir, &not_a_config);
        Tracker::clean_in(&dir).unwrap();
        assert!(
            fs::symlink_metadata(&entry).is_err(),
            "the tracking entry must be removed"
        );
    }

    #[test]
    fn removes_an_entry_whose_target_is_gone() {
        // The entry is checked with `symlink_metadata` rather than `exists`,
        // which follows the link: a dangling symlink reports `false` whether or
        // not the clean removed it, so `exists` would pass on unix regardless.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = config(&tmp, "mise.toml");

        let entry = track(&dir, &config);
        fs::remove_file(&config).unwrap();

        Tracker::clean_in(&dir).unwrap();
        assert!(
            fs::symlink_metadata(&entry).is_err(),
            "the tracking entry must be removed"
        );
    }

    /// Tracks a deleted file and a directory, each last used `ago`, with expiry
    /// on for longer than that.
    fn track_missing(tmp: &tempfile::TempDir, dir: &Path, ago: Duration) -> [PathBuf; 2] {
        let gone = config(tmp, "gone.toml");
        let not_a_config = tmp.path().join("somewhere");
        fs::create_dir(&not_a_config).unwrap();
        let entries = [track(dir, &gone), track(dir, &not_a_config)];
        fs::remove_file(&gone).unwrap();
        let start = dir.with_extension("expiry-start");
        fs::write(&start, "").unwrap();
        set_last_used(&start, 365 * DAY);
        for entry in &entries {
            set_last_used(entry, ago);
        }
        entries
    }

    #[test]
    fn listing_skips_but_keeps_a_recently_missing_entry() {
        // A missing file protects nothing, so it is not listed, but it may come
        // back, so its entry is kept for a week.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let kept = config(&tmp, "mise.toml");
        track(&dir, &kept);
        let missing = track_missing(&tmp, &dir, 6 * DAY);

        assert_eq!(list(&dir), vec![kept]);
        for entry in missing {
            assert!(
                fs::symlink_metadata(&entry).is_ok(),
                "{entry:?} was forgotten"
            );
        }
    }

    #[test]
    fn listing_forgets_an_entry_missing_for_a_week() {
        // Even with `prune.forget_unused_after` off.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let missing = track_missing(&tmp, &dir, 8 * DAY);

        assert!(list(&dir).is_empty());
        for entry in missing {
            assert!(fs::symlink_metadata(&entry).is_err(), "{entry:?} was kept");
        }
    }

    #[test]
    fn a_missing_entry_from_before_expiry_counts_from_its_start() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let gone = config(&tmp, "gone.toml");
        let entry = track(&dir, &gone);
        fs::remove_file(&gone).unwrap();
        set_last_used(&entry, 400 * DAY);
        let now = SystemTime::now();

        assert!(Tracker::list_all_in(&dir, None, now).unwrap().is_empty());
        assert!(fs::symlink_metadata(&entry).is_ok(), "the week starts now");
        Tracker::list_all_in(&dir, None, now + 8 * DAY).unwrap();
        assert!(fs::symlink_metadata(&entry).is_err());
    }

    #[test]
    fn listing_forgets_an_entry_unused_for_longer_than_the_max_age() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let stale = config(&tmp, "stale.toml");
        let recent = config(&tmp, "recent.toml");
        let stale_entry = track(&dir, &stale);
        let recent_entry = track(&dir, &recent);
        // Expiry has been on for longer than any of these ages.
        let start = dir.with_extension("expiry-start");
        fs::write(&start, "").unwrap();
        set_last_used(&start, 365 * DAY);
        set_last_used(&stale_entry, 91 * DAY);
        set_last_used(&recent_entry, 89 * DAY);

        let listed = Tracker::list_all_in(&dir, Some(90 * DAY), SystemTime::now()).unwrap();

        assert_eq!(listed, vec![recent]);
        assert!(fs::symlink_metadata(&stale_entry).is_err());
    }

    #[test]
    fn no_max_age_keeps_an_old_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = config(&tmp, "mise.toml");
        let entry = track(&dir, &config);
        set_last_used(&entry, 1000 * DAY);

        assert_eq!(list(&dir), vec![config]);
    }

    #[test]
    fn entries_from_before_expiry_count_from_its_start() {
        // An entry an older mise wrote carries its creation time, not its last
        // use, so the first listing starts its clock instead of expiring it.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = config(&tmp, "mise.toml");
        let entry = track(&dir, &config);
        set_last_used(&entry, 400 * DAY);
        let now = SystemTime::now();

        let first = Tracker::list_all_in(&dir, Some(90 * DAY), now).unwrap();
        assert_eq!(first, vec![config.clone()]);
        let later = Tracker::list_all_in(&dir, Some(90 * DAY), now + 91 * DAY).unwrap();
        assert!(later.is_empty(), "unused for 91 days since expiry started");
    }

    #[test]
    fn tracking_refreshes_an_entry_once_it_is_a_day_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("tracked");
        let config = config(&tmp, "mise.toml");
        let entry = track(&dir, &config);

        set_last_used(&entry, DAY / 2);
        let before = last_used(&entry).unwrap();
        Tracker::track_in(&dir, &config, SystemTime::now()).unwrap();
        assert_eq!(
            last_used(&entry).unwrap(),
            before,
            "fresh entries are not rewritten"
        );

        set_last_used(&entry, 30 * DAY);
        Tracker::track_in(&dir, &config, SystemTime::now()).unwrap();
        let age = SystemTime::now()
            .duration_since(last_used(&entry).unwrap())
            .unwrap_or_default();
        assert!(age < DAY, "a stale entry is refreshed, got {age:?}");
        assert_eq!(list(&dir), vec![config]);
    }
}
