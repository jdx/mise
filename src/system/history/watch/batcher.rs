//! Turns a stream of changed paths into capture moments. A path is quiet
//! once nothing touched it for the debounce interval; the batch flushes as
//! soon as any pending path is quiet, so a file that keeps changing never
//! delays the others, and after the maximum interval regardless.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct Batcher {
    debounce: Duration,
    max_interval: Duration,
    /// Every pending path with the last moment it changed.
    pending: BTreeMap<PathBuf, Instant>,
    /// When the oldest still-pending change arrived.
    oldest: Option<Instant>,
}

impl Batcher {
    pub(crate) fn new(debounce: Duration, max_interval: Duration) -> Self {
        Self {
            debounce,
            max_interval: max_interval.max(debounce),
            pending: BTreeMap::new(),
            oldest: None,
        }
    }

    pub(crate) fn note(&mut self, path: PathBuf, now: Instant) {
        self.pending.insert(path, now);
        self.oldest.get_or_insert(now);
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// When the next flush is due, if anything is pending.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        let quiet = self
            .pending
            .values()
            .map(|last| *last + self.debounce)
            .min()?;
        let forced = self.oldest.map(|oldest| oldest + self.max_interval);
        Some(match forced {
            Some(forced) => quiet.min(forced),
            None => quiet,
        })
    }

    /// The paths ready to capture: every quiet one, or everything once the
    /// maximum interval has passed. Still-changing paths stay pending.
    pub(crate) fn flush(&mut self, now: Instant) -> Vec<PathBuf> {
        let forced = self
            .oldest
            .is_some_and(|oldest| now.duration_since(oldest) >= self.max_interval);
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, last)| forced || now.duration_since(**last) >= self.debounce)
            .map(|(path, _)| path.clone())
            .collect();
        for path in &ready {
            self.pending.remove(path);
        }
        if self.pending.is_empty() {
            self.oldest = None;
        } else if forced {
            // the remaining paths start a new interval
            self.oldest = Some(now);
        }
        ready
    }

    /// Everything pending, for a final capture at shutdown.
    pub(crate) fn drain(&mut self) -> Vec<PathBuf> {
        self.oldest = None;
        std::mem::take(&mut self.pending).into_keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_quiet_path_flushes_after_the_debounce() {
        let start = Instant::now();
        let mut batcher = Batcher::new(secs(2), secs(30));
        batcher.note("a".into(), start);
        assert_eq!(batcher.deadline(), Some(start + secs(2)));
        assert!(batcher.flush(start + secs(1)).is_empty());
        assert_eq!(batcher.flush(start + secs(2)), vec![PathBuf::from("a")]);
        assert!(batcher.is_empty());
        assert_eq!(batcher.deadline(), None);
    }

    #[test]
    fn a_changing_path_never_delays_a_quiet_one() {
        let start = Instant::now();
        let mut batcher = Batcher::new(secs(2), secs(30));
        batcher.note("busy".into(), start);
        batcher.note("quiet".into(), start);
        batcher.note("busy".into(), start + secs(1));
        batcher.note("busy".into(), start + secs(2));
        assert_eq!(batcher.deadline(), Some(start + secs(2)));
        assert_eq!(batcher.flush(start + secs(2)), vec![PathBuf::from("quiet")]);
        assert_eq!(batcher.deadline(), Some(start + secs(4)));
    }

    #[test]
    fn the_maximum_interval_flushes_everything() {
        let start = Instant::now();
        let mut batcher = Batcher::new(secs(2), secs(30));
        batcher.note("busy".into(), start);
        for i in 1..=29 {
            batcher.note("busy".into(), start + secs(i));
            assert!(batcher.flush(start + secs(i)).is_empty());
        }
        assert_eq!(batcher.deadline(), Some(start + secs(30)));
        batcher.note("busy".into(), start + secs(30));
        assert_eq!(batcher.flush(start + secs(30)), vec![PathBuf::from("busy")]);
        assert!(batcher.is_empty());
    }

    #[test]
    fn drain_takes_everything() {
        let start = Instant::now();
        let mut batcher = Batcher::new(secs(2), secs(30));
        batcher.note("a".into(), start);
        batcher.note("b".into(), start);
        assert_eq!(batcher.drain().len(), 2);
        assert!(batcher.is_empty());
    }
}
