use std::sync::Mutex;

use demand::Confirm;
use miette::{IntoDiagnostic, Result};

use crate::ui;

static MUTEX: Mutex<()> = Mutex::new(());

pub fn confirm<S: Into<String>>(message: S) -> Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ui::handle_ctrlc();

    if !console::user_attended_stderr() {
        return Ok(false);
    }
    Confirm::new(message).run().into_diagnostic()
}
