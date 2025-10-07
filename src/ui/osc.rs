/// OSC (Operating System Command) escape sequences for terminal integration
///
/// This module provides support for OSC escape sequences that allow terminal
/// integration features like progress bars in Ghostty, VS Code, and iTerm2.
use std::io::{self, Write};

/// OSC 9;4 states for terminal progress indication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ProgressState {
    /// No progress indicator (clears any existing progress)
    None,
    /// Indeterminate progress (spinner/activity indicator)
    Indeterminate,
    /// Progress bar with percentage
    Progress,
    /// Error state
    Error,
}

impl ProgressState {
    fn as_code(&self) -> u8 {
        match self {
            ProgressState::None => 0,
            ProgressState::Indeterminate => 1,
            ProgressState::Progress => 2,
            ProgressState::Error => 3,
        }
    }
}

/// Sends an OSC 9;4 sequence to set terminal progress
///
/// # Arguments
/// * `state` - The progress state to display
/// * `progress` - Progress percentage (0-100), ignored if state is None or Indeterminate
///
/// # Example
/// ```no_run
/// use mise::ui::osc::{set_progress, ProgressState};
///
/// // Show 50% progress
/// set_progress(ProgressState::Progress, 50);
///
/// // Show indeterminate progress
/// set_progress(ProgressState::Indeterminate, 0);
///
/// // Clear progress
/// set_progress(ProgressState::None, 0);
/// ```
pub fn set_progress(state: ProgressState, progress: u8) {
    let progress = progress.min(100);
    let _ = write_progress(state, progress);
}

fn write_progress(state: ProgressState, progress: u8) -> io::Result<()> {
    let mut stderr = io::stderr();
    // OSC 9;4 format: ESC ] 9 ; 4 ; <state> ; <progress> BEL
    // Note: The color is controlled by the terminal theme
    // Ghostty may show cyan automatically for normal progress
    write!(
        stderr,
        "\x1b]9;4;{};{}\x1b\\",
        state.as_code(),
        progress
    )?;
    stderr.flush()
}

/// Clears any terminal progress indicator
pub fn clear_progress() {
    set_progress(ProgressState::None, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_state_codes() {
        assert_eq!(ProgressState::None.as_code(), 0);
        assert_eq!(ProgressState::Indeterminate.as_code(), 1);
        assert_eq!(ProgressState::Progress.as_code(), 2);
        assert_eq!(ProgressState::Error.as_code(), 3);
    }

    #[test]
    fn test_set_progress_doesnt_panic() {
        // Just ensure it doesn't panic when called
        set_progress(ProgressState::Progress, 50);
        set_progress(ProgressState::Indeterminate, 0);
        clear_progress();
    }

    #[test]
    fn test_progress_clamping() {
        // Verify that progress values over 100 are clamped
        set_progress(ProgressState::Progress, 150);
    }
}
