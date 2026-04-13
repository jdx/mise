use std::io::IsTerminal;
use std::sync::Mutex;

use demand::{Confirm, Dialog, DialogButton};

use crate::env;
use crate::ui::ctrlc;
use crate::ui::theme::get_theme;

static MUTEX: Mutex<()> = Mutex::new(());

static SKIP_PROMPT: Mutex<bool> = Mutex::new(false);

/// Returns true if the current process can safely show an interactive prompt.
///
/// Requires stderr to be a TTY (so the prompt is visible) AND stdout to be a
/// TTY. When stdout is not a TTY the process is running inside a command
/// substitution (`$(…)`) or process substitution (`<(…)`), contexts where
/// stdin may report as a TTY yet be unable to deliver input — leading to an
/// unrecoverable hang. See https://github.com/jdx/mise/discussions/8940
fn can_prompt() -> bool {
    console::user_attended_stderr() && std::io::stdout().is_terminal() && env::__USAGE.is_none()
}

pub fn confirm<S: Into<String>>(message: S) -> eyre::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ctrlc::show_cursor_after_ctrl_c();

    if !can_prompt() {
        return Ok(false);
    }
    let theme = get_theme();
    let result = Confirm::new(message)
        .clear_screen(true)
        .theme(&theme)
        .run()?;
    Ok(result)
}

pub fn confirm_with_all<S: Into<String>>(message: S) -> eyre::Result<bool> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ctrlc::show_cursor_after_ctrl_c();

    if !can_prompt() {
        return Ok(false);
    }

    let mut skip_prompt = SKIP_PROMPT.lock().unwrap();
    if *skip_prompt {
        return Ok(true);
    }

    let theme = get_theme();
    let answer = Dialog::new(message)
        .buttons(vec![
            DialogButton::new("Yes"),
            DialogButton::new("No"),
            DialogButton::new("All"),
        ])
        .selected_button(1)
        .clear_screen(true)
        .theme(&theme)
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
