//! The history watcher: filesystem watches for the tracked set (`plan`),
//! per-file adaptive save scheduling (`schedule`), the report of throttled
//! paths (`noise`), and the foreground process (`runtime`).

pub mod noise;
pub(crate) mod plan;
pub mod runtime;
pub(crate) mod schedule;
