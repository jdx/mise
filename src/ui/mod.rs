pub use prompt::confirm;

#[cfg_attr(test, path = "ctrlc_stub.rs")]
#[cfg_attr(all(windows, not(test)), path = "ctrlc_windows.rs")]
pub mod ctrlc;
pub mod info;
pub mod install_progress;
pub mod multi_progress_report;
pub mod progress_report;
pub mod prompt;
pub mod resolve_progress;
pub mod style;
pub mod table;
pub(crate) mod text_install_progress;
pub mod theme;
pub mod time;
pub mod tree;
pub(crate) mod tty_install_progress;
