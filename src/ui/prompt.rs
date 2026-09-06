use std::io::{BufRead, IsTerminal};
use std::sync::Mutex;

use demand::{Confirm, Dialog, DialogButton};

use crate::env;
use crate::ui::ctrlc;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::theme::get_theme;

static MUTEX: Mutex<()> = Mutex::new(());

static SKIP_PROMPT: Mutex<bool> = Mutex::new(false);

/// The outcome of a confirmation prompt.
///
/// A plain `bool` cannot say the difference between a user who declined and a
/// prompt that was never answerable, so every caller that acted on "no" had to
/// re-derive the difference for itself -- and one that forgot silently attributed
/// a decision to someone who never made one. Carrying it in the return type is
/// what keeps that from being expressible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIs)]
pub(crate) enum Confirmation {
    Yes,
    No,
    /// The question never reached anyone: nothing was drawable on stderr, or
    /// mise is generating usage/completions. A prompt that *was* displayed and
    /// then went unanswered is [`Confirmation::No`], not this.
    Unavailable,
}

/// Whether a [`Dialog`] can actually be answered.
///
/// `demand` defines interactive as stdin *and* stderr ([`demand::tty::is_tty`]),
/// and [`Confirm`] honors that -- but [`Dialog`] has no non-tty branch, so it
/// drives `Term::stderr().read_key()` unconditionally. When stderr is a terminal
/// and stdin is not (`docker run -t` without `-i`, a `script`-wrapped CI step,
/// `mise env < /dev/null`), `console` falls back to `/dev/tty` and either fails
/// with `ENXIO` -- burying the caller's real error under an errno -- or blocks
/// forever on a keypress nobody is there to type. Checking both ends keeps a
/// dialog from being drawn when nothing can answer it.
///
/// Deliberately private: callers learn that nobody could be asked from
/// [`Confirmation::Unavailable`], never by re-deriving it here.
fn can_prompt_dialog() -> bool {
    dialog_prompt_allowed(
        console::user_attended_stderr(),
        std::io::stdin().is_terminal(),
        env::__USAGE.is_some(),
    )
}

/// The decision behind [`can_prompt_dialog`], split out so it can be tested
/// without manipulating the process's file descriptors.
fn dialog_prompt_allowed(stderr_tty: bool, stdin_tty: bool, usage: bool) -> bool {
    stderr_tty && stdin_tty && !usage
}

/// [`Dialog`] and [`Confirm`] hide the cursor while they render and restore it on
/// submit and on Escape, but not when the read itself fails -- which leaves the
/// terminal with an invisible cursor. Failures here are ignored: the error that
/// got us here is the one worth reporting.
fn restore_cursor() {
    let _ = console::Term::stderr().show_cursor();
}

pub(crate) fn confirm<S: Into<String>>(message: S) -> eyre::Result<Confirmation> {
    confirm_with_default(message, true)
}

pub(crate) fn confirm_with_default<S: Into<String>>(
    message: S,
    default_yes: bool,
) -> eyre::Result<Confirmation> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ctrlc::show_cursor_after_ctrl_c();

    if !console::user_attended_stderr() || env::__USAGE.is_some() {
        return Ok(Confirmation::Unavailable);
    }
    let message = message.into();
    // Held across both branches below: a prompt written to stderr is just as
    // easily overwritten by progress redraws when it is read from a pipe as when
    // it is read from the terminal. The guard is a no-op when no report is active.
    let _progress_pause = MultiProgressReport::try_get().map(|report| report.pause_progress());
    // Deliberately not `can_prompt_dialog()`: `Confirm` does have a non-tty
    // branch, so `echo y | mise ...` is a supported way to answer. What it cannot
    // do is tell EOF from a bare newline -- it discards the 0-byte return of
    // `read_line` and applies the default -- so `mise implode < /dev/null` would
    // answer "yes" to every deletion. Read the line here instead, so that no
    // answer means no.
    if !std::io::stdin().is_terminal() {
        return read_confirm_from_stdin(&message, default_yes);
    }
    let theme = get_theme();
    let result = Confirm::new(message)
        .selected(default_yes)
        .theme(&theme)
        .run()
        .inspect_err(|_| restore_cursor())?;
    Ok(if result {
        Confirmation::Yes
    } else {
        Confirmation::No
    })
}

/// Prints `message` to stderr and reads a single answer from a non-terminal
/// stdin, which is what makes `echo y | mise ...` work without letting an
/// unanswered prompt (EOF) count as consent.
fn read_confirm_from_stdin(message: &str, default_yes: bool) -> eyre::Result<Confirmation> {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    safe_eprintln!("{message} {hint}");
    let mut line = String::new();
    let read = std::io::stdin().lock().read_line(&mut line)?;
    // A 0-byte read is EOF, which is the case `demand` cannot distinguish.
    let answer = (read > 0).then_some(line.as_str());
    parse_confirm_answer(answer, default_yes)
}

/// Interprets one typed answer, matching what `demand`'s own non-tty branch
/// accepted: any prefix of "yes" or "no", an empty line for the default, and an
/// error for anything else -- silently rereading what someone typed as "no" would
/// be worse than telling them it was not understood.
///
/// `None` means stdin reached EOF without an answer -- the one case `demand`
/// gets wrong (it applies the default) and the reason this exists.
fn parse_confirm_answer(line: Option<&str>, default_yes: bool) -> eyre::Result<Confirmation> {
    let Some(line) = line else {
        // A decline, not `Unavailable`: the question *was* put on stderr and
        // stdin ended without an answer, so nothing was consented to.
        // `Unavailable` is reserved for the question never reaching anyone.
        // Keeping it a decline is also what every caller already assumed, and
        // leaves silence safe to read as "no" for any that come later.
        return Ok(Confirmation::No);
    };
    let answer = line.trim().to_lowercase();
    if answer.is_empty() {
        return Ok(default_answer(default_yes));
    }
    if "yes".starts_with(&answer) {
        Ok(Confirmation::Yes)
    } else if "no".starts_with(&answer) {
        Ok(Confirmation::No)
    } else {
        eyre::bail!("expected y/yes or n/no, got {:?}", line.trim())
    }
}

fn default_answer(default_yes: bool) -> Confirmation {
    if default_yes {
        Confirmation::Yes
    } else {
        Confirmation::No
    }
}

pub(crate) fn confirm_with_all<S: Into<String>>(message: S) -> eyre::Result<Confirmation> {
    let _lock = MUTEX.lock().unwrap(); // Prevent multiple prompts at once
    ctrlc::show_cursor_after_ctrl_c();

    if !can_prompt_dialog() {
        return Ok(Confirmation::Unavailable);
    }

    let mut skip_prompt = SKIP_PROMPT.lock().unwrap();
    if *skip_prompt {
        return Ok(Confirmation::Yes);
    }

    let _progress_pause = MultiProgressReport::try_get().map(|report| report.pause_progress());
    let theme = get_theme();
    let answer = Dialog::new(message)
        .buttons(vec![
            DialogButton::new("Yes"),
            DialogButton::new("No"),
            DialogButton::new("All"),
        ])
        .selected_button(1)
        .theme(&theme)
        .run()
        .inspect_err(|_| restore_cursor())?;

    if answer == "All" {
        *skip_prompt = true;
    }
    Ok(dialog_answer(&answer))
}

/// Maps a [`Dialog`] button label to an answer. "All" is a "yes" that also
/// latches [`SKIP_PROMPT`]; the latching is the caller's job so this stays pure.
fn dialog_answer(answer: &str) -> Confirmation {
    match answer {
        "Yes" | "All" => Confirmation::Yes,
        _ => Confirmation::No,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_needs_both_ends_of_the_terminal() {
        // A dialog is drawn on stderr and read from the terminal, so anything
        // less than both ends means nothing can answer it.
        assert!(dialog_prompt_allowed(true, true, false));
        assert!(!dialog_prompt_allowed(false, true, false));
        assert!(!dialog_prompt_allowed(true, false, false));
        assert!(!dialog_prompt_allowed(false, false, false));
    }

    #[test]
    fn dialog_never_shown_while_generating_usage() {
        // Completion/usage generation must never block on a prompt, even from a
        // terminal.
        assert!(!dialog_prompt_allowed(true, true, true));
        assert!(!dialog_prompt_allowed(false, true, true));
        assert!(!dialog_prompt_allowed(true, false, true));
        assert!(!dialog_prompt_allowed(false, false, true));
    }

    #[test]
    fn eof_is_not_consent() {
        // The prompt was displayed and stdin ended without an answer. That has
        // to read as a decline, whichever way the default points -- callers
        // that act only on an explicit "no" must not take it as permission.
        assert_eq!(parse_confirm_answer(None, true).unwrap(), Confirmation::No);
        assert_eq!(parse_confirm_answer(None, false).unwrap(), Confirmation::No);
    }

    #[test]
    fn empty_line_takes_the_default() {
        for line in ["", "\n", "  \n"] {
            assert_eq!(
                parse_confirm_answer(Some(line), true).unwrap(),
                Confirmation::Yes
            );
            assert_eq!(
                parse_confirm_answer(Some(line), false).unwrap(),
                Confirmation::No
            );
        }
    }

    #[test]
    fn explicit_answers_override_the_default() {
        // Any prefix of the label answers, as `demand`'s own non-tty branch did.
        for line in ["y\n", "Y", "ye", "yes", "YES\n"] {
            assert_eq!(
                parse_confirm_answer(Some(line), false).unwrap(),
                Confirmation::Yes
            );
        }
        for line in ["n\n", "N", "no", "No\n"] {
            assert_eq!(
                parse_confirm_answer(Some(line), true).unwrap(),
                Confirmation::No
            );
        }
    }

    #[test]
    fn all_answers_yes() {
        assert_eq!(dialog_answer("Yes"), Confirmation::Yes);
        assert_eq!(dialog_answer("All"), Confirmation::Yes);
        assert_eq!(dialog_answer("No"), Confirmation::No);
        // Escape and anything unrecognized decline rather than proceed.
        assert_eq!(dialog_answer(""), Confirmation::No);
    }

    #[test]
    fn unrecognized_answers_are_an_error() {
        // Not silently a "no": the user typed something, and guessing at it is
        // how a decline gets attributed to someone who never made one.
        for line in ["maybe", "1", "yep", "sure\n"] {
            assert!(parse_confirm_answer(Some(line), true).is_err());
            assert!(parse_confirm_answer(Some(line), false).is_err());
        }
    }
}
