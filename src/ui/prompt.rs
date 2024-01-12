use crate::ui;
use demand::Confirm;
use std::io;
use std::sync::Mutex;

static MUTEX: Mutex<()> = Mutex::new(());

pub fn confirm<S: Into<String>>(message: S) -> io::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ui::handle_ctrlc();

    if !console::user_attended_stderr() {
        return Ok(false);
    }
    Confirm::new(message).run()
}
