use console::{user_attended_stderr, Term};
pub use prompt::confirm;
use std::process::exit;
use std::sync::Once;

pub mod multi_progress_report;
pub mod progress_report;
pub mod prompt;
pub mod style;
pub mod table;

pub fn handle_ctrlc() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if user_attended_stderr() {
            let _ = ctrlc::set_handler(move || {
                let _ = Term::stderr().show_cursor();
                debug!("Ctrl-C pressed, exiting...");
                exit(1);
            });
        }
    });
}
