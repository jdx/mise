pub(crate) use prompt::confirm;

#[cfg_attr(any(test, windows), path = "ctrlc_stub.rs")]
pub(crate) mod ctrlc;
pub(crate) mod info;
pub(crate) mod multi_progress_report;
pub(crate) mod progress_report;
pub(crate) mod prompt;
pub(crate) mod style;
pub(crate) mod table;
pub(crate) mod theme;
pub(crate) mod time;
pub(crate) mod tree;
