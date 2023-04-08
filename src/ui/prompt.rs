use std::io;

use dialoguer::Confirm;

use crate::env;

pub fn confirm(message: &str) -> io::Result<bool> {
    match *env::RTX_CONFIRM {
        env::Confirm::Yes => return Ok(true),
        env::Confirm::No => return Ok(false),
        env::Confirm::Prompt => (),
    }
    if !console::user_attended_stderr() {
        return Ok(false);
    }
    Confirm::new().with_prompt(message).interact()
}
