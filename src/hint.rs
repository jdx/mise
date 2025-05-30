use crate::config::Settings;
use crate::dirs;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;

#[macro_export]
macro_rules! hint {
    ($id:expr, $message:expr, $example_cmd:expr) => {{
        if $crate::hint::should_display_hint($id) {
            let _ = $crate::file::touch_file(&$crate::hint::HINTS_DIR.join($id));
            let prefix = console::style("hint")
                .dim()
                .yellow()
                .for_stderr()
                .to_string();
            let message = format!($message);
            let cmd = console::style($example_cmd).bold().for_stderr();
            info!("{prefix} {message} {cmd}");
        }
    }};
}

pub static HINTS_DIR: Lazy<PathBuf> = Lazy::new(|| dirs::STATE.join("hints"));

pub static DISPLAYED_HINTS: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| {
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

pub fn should_display_hint(id: &str) -> bool {
    if cfg!(test) || !console::user_attended() || !console::user_attended_stderr() {
        return false;
    }
    if Settings::get()
        .disable_hints
        .iter()
        .any(|hint| hint == id || hint == "*")
    {
        return false;
    }
    let displayed_hints = &mut DISPLAYED_HINTS.lock().unwrap();
    if displayed_hints.contains(id) {
        return false;
    }
    displayed_hints.insert(id.to_string());
    true
}
