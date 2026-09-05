//! Notices paths that change constantly. Noise is never dropped from
//! protection and never excluded automatically: it is reported once per
//! path per hour with the exclusion the user can choose, and remembered in
//! `noisy.json` for `mise history paths --noisy`.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub(crate) const WINDOW: Duration = Duration::from_secs(10 * 60);
pub(crate) const THRESHOLD: usize = 60;
const WARN_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug)]
pub(crate) struct NoiseMonitor {
    events: BTreeMap<PathBuf, VecDeque<Instant>>,
    warned: BTreeMap<PathBuf, Instant>,
}

impl NoiseMonitor {
    pub(crate) fn new() -> Self {
        Self {
            events: BTreeMap::new(),
            warned: BTreeMap::new(),
        }
    }

    /// Records a change; returns the count in the window when a warning is
    /// due for this path.
    pub(crate) fn record(&mut self, path: &Path, now: Instant) -> Option<usize> {
        let events = self.events.entry(path.to_path_buf()).or_default();
        events.push_back(now);
        while events
            .front()
            .is_some_and(|first| now.duration_since(*first) > WINDOW)
        {
            events.pop_front();
        }
        let count = events.len();
        if count < THRESHOLD {
            return None;
        }
        let due = self
            .warned
            .get(path)
            .is_none_or(|last| now.duration_since(*last) >= WARN_INTERVAL);
        if !due {
            return None;
        }
        self.warned.insert(path.to_path_buf(), now);
        Some(count)
    }

    /// Every path over the threshold right now, with its count.
    pub(crate) fn noisy(&self, now: Instant) -> Vec<(PathBuf, usize)> {
        self.events
            .iter()
            .map(|(path, events)| {
                let count = events
                    .iter()
                    .filter(|at| now.duration_since(**at) <= WINDOW)
                    .count();
                (path.clone(), count)
            })
            .filter(|(_, count)| *count >= THRESHOLD)
            .collect()
    }

    /// Forgets paths with no change in the window, so the map stays small.
    pub(crate) fn prune(&mut self, now: Instant) {
        self.events.retain(|_, events| {
            events
                .back()
                .is_some_and(|last| now.duration_since(*last) <= WINDOW)
        });
    }
}

/// What the watcher persists for `mise history paths --noisy`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NoisyRecord {
    #[serde(default)]
    pub paths: BTreeMap<String, NoisyPath>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NoisyPath {
    pub changes_per_10m: usize,
    pub last_seen: String,
}

pub(crate) fn read(path: &Path) -> NoisyRecord {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn write(path: &Path, record: &NoisyRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(record)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_per_hour_above_the_threshold() {
        let start = Instant::now();
        let mut monitor = NoiseMonitor::new();
        let path = Path::new("/home/u/.config/app/state.json");
        for i in 0..THRESHOLD - 1 {
            assert_eq!(
                monitor.record(path, start + Duration::from_secs(i as u64)),
                None
            );
        }
        assert_eq!(
            monitor.record(path, start + Duration::from_secs(60)),
            Some(THRESHOLD)
        );
        assert_eq!(monitor.record(path, start + Duration::from_secs(61)), None);
        assert_eq!(monitor.noisy(start + Duration::from_secs(61)).len(), 1);
        // an hour later the path is still noisy: warned again, once
        let later = start + Duration::from_secs(3700);
        for i in 0..THRESHOLD - 1 {
            assert_eq!(
                monitor.record(path, later + Duration::from_secs(i as u64)),
                None
            );
        }
        assert_eq!(
            monitor.record(path, later + Duration::from_secs(60)),
            Some(THRESHOLD)
        );
        assert_eq!(monitor.record(path, later + Duration::from_secs(61)), None);
    }

    #[test]
    fn old_changes_leave_the_window() {
        let start = Instant::now();
        let mut monitor = NoiseMonitor::new();
        let path = Path::new("/x");
        for i in 0..THRESHOLD {
            monitor.record(path, start + Duration::from_secs(i as u64));
        }
        let later = start + WINDOW + Duration::from_secs(120);
        assert!(monitor.noisy(later).is_empty());
        monitor.prune(later);
        assert!(monitor.events.is_empty());
    }
}
