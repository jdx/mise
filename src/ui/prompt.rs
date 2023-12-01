use std::io;
use std::sync::Mutex;

use dialoguer::Confirm;

use crate::env;

static MUTEX: Mutex<()> = Mutex::new(());

pub fn confirm(message: &str) -> io::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once

    match *env::RTX_CONFIRM {
        env::Confirm::Yes => return Ok(true),
        env::Confirm::No => return Ok(false),
        env::Confirm::Prompt => (),
    }
    if !console::user_attended_stderr() {
        return Ok(false);
    }
    match Confirm::new().with_prompt(message).interact() {
        Ok(choice) => Ok(choice),
        Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
    }
}
