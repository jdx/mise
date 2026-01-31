pub use prompt::confirm;

#[cfg_attr(any(test, windows), path = "ctrlc_stub.rs")]
pub mod ctrlc;
pub mod diagnostic_log;
pub(crate) mod info;
pub mod multi_progress_report;
pub mod osc;
pub mod progress_report;
pub mod prompt;
pub mod style;
pub mod table;
pub mod theme;
pub mod time;
pub mod tree;
