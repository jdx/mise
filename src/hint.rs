use crate::config::Settings;
use crate::dirs;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;

/// Prints a hint, by default only the first time it would be shown. The marker
/// file in `HINTS_DIR` records that, and `disable_hints` silences it for good.
///
/// `hint!(always, id, message, cmd)` skips the marker and prints on every call
/// until the user disables it via `disable_hints`. Use it sparingly: a hint that
/// can't be dismissed by acting on it nags forever.
#[macro_export]
macro_rules! hint {
    (always, $id:expr, $message:expr, $example_cmd:expr) => {{
        if $crate::hint::hint_enabled($id) {
            $crate::hint!(@print $message, $example_cmd);
        }
    }};
    ($id:expr, $message:expr, $example_cmd:expr) => {{
        if $crate::hint::should_display_hint($id) {
            let _ = $crate::file::touch_file(&$crate::hint::HINTS_DIR.join($id));
            $crate::hint!(@print $message, $example_cmd);
        }
    }};
    (@print $message:expr, $example_cmd:expr) => {{
        let prefix = console::style("hint")
            .dim()
            .yellow()
            .for_stderr()
            .to_string();
        let message = format!($message);
        let cmd = console::style($example_cmd).bold().for_stderr();
        info!("{prefix} {message} {cmd}");
    }};
}

pub static HINTS_DIR: Lazy<PathBuf> = Lazy::new(|| dirs::STATE.join("hints"));

pub(crate) static DISPLAYED_HINTS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| {
    let mut hints = HashSet::new();

    for file in xx::file::ls(&*HINTS_DIR).unwrap_or_default() {
        if let Some(file_name) = file.file_name().map(|f| f.to_string_lossy()) {
            if file_name.starts_with(".") {
                continue;
            }
            hints.insert(file_name.to_string());
        }
    }

    Mutex::new(hints)
});

/// Would `hint!` display this id? Unlike [`should_display_hint`], this does
/// not mark the hint as displayed — use it to skip expensive work whose only
/// purpose is feeding a hint.
pub fn hint_would_display(id: &str) -> bool {
    if mise_util::testing::in_tests()
        || !console::user_attended()
        || !console::user_attended_stderr()
    {
        return false;
    }
    if Settings::get()
        .disable_hints
        .iter()
        .any(|hint| hint == id || hint == "*")
    {
        return false;
    }
    !DISPLAYED_HINTS.lock().unwrap().contains(id)
}

/// Is this hint allowed on this terminal and not disabled via `disable_hints`?
/// Does not consult or record whether it was shown before, so callers can use it
/// for hints that should appear on every run until the user disables them.
pub fn hint_enabled(id: &str) -> bool {
    if mise_util::testing::in_tests()
        || !console::user_attended()
        || !console::user_attended_stderr()
    {
        return false;
    }
    !Settings::get()
        .disable_hints
        .iter()
        .any(|hint| hint == id || hint == "*")
}

pub fn should_display_hint(id: &str) -> bool {
    if !hint_enabled(id) {
        return false;
    }
    let displayed_hints = &mut DISPLAYED_HINTS.lock().unwrap();
    if displayed_hints.contains(id) {
        return false;
    }
    displayed_hints.insert(id.to_string());
    true
}
