//! The history watcher: filesystem watches for the tracked set (`plan`),
//! change batching with a per-path quiet period (`batcher`), noise
//! reporting (`noise`), and the foreground process (`runtime`).

pub(crate) mod batcher;
pub(crate) mod noise;
pub(crate) mod plan;
pub(crate) mod runtime;
