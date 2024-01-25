use std::sync::Mutex;

use demand::Confirm;

use crate::ui::ctrlc;

static MUTEX: Mutex<()> = Mutex::new(());

pub fn confirm<S: Into<String>>(message: S) -> eyre::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    let _ = ctrlc::handle_ctrlc()?;

    if !console::user_attended_stderr() {
        return Ok(false);
    }
    let result = Confirm::new(message).run()?;
    Ok(result)
}
