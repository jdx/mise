use std::sync::Mutex;

use demand::{Confirm, Dialog, DialogButton};

use crate::env;
use crate::ui::ctrlc;

static MUTEX: Mutex<()> = Mutex::new(());

static SKIP_PROMPT: Mutex<bool> = Mutex::new(false);

pub fn confirm<S: Into<String>>(message: S) -> eyre::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ctrlc::show_cursor_after_ctrl_c();

    if !console::user_attended_stderr() || env::__USAGE.is_some() {
        return Ok(false);
    }
    let result = Confirm::new(message).run()?;
    Ok(result)
}

pub fn confirm_with_all<S: Into<String>>(message: S) -> eyre::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ctrlc::show_cursor_after_ctrl_c();

    if !console::user_attended_stderr() || env::__USAGE.is_some() {
        return Ok(false);
    }

    let mut skip_prompt = SKIP_PROMPT.lock().unwrap();
    if *skip_prompt {
        return Ok(true);
    }

    let answer = Dialog::new(message)
        .buttons(vec![
            DialogButton::new("Yes"),
            DialogButton::new("No"),
            DialogButton::new("All"),
        ])
        .selected_button(1)
        .run()?;

    let result = match answer.as_str() {
        "Yes" => true,
        "No" => false,
        "All" => {
            *skip_prompt = true;
            true
        }
        _ => false,
    };
    Ok(result)
}
