pub use prompt::confirm;

#[cfg_attr(any(test, windows), path = "ctrlc_stub.rs")]
pub mod ctrlc;
pub(crate) mod info;
pub mod multi_progress_report;
pub mod progress_report;
pub mod prompt;
pub mod style;
pub mod table;
pub mod time;
pub mod tree;
