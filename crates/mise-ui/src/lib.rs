/// Like `eprintln!`, but ignores write failures instead of panicking when
/// stderr is unwritable (e.g. the process that spawned mise closed its end
/// of the pipe).
macro_rules! safe_eprintln {
    ($($arg:tt)*) => {{
        let _ = calm_io::stderrln!($($arg)*);
    }};
}

macro_rules! progress_trace {
    ($($arg:tt)+) => {{
        if *mise_util::env::MISE_PROGRESS_TRACE {
            eprintln!("[PROGRESS] {}", format!($($arg)+));
        }
    }};
}

pub use prompt::confirm;

#[cfg_attr(windows, path = "ctrlc_windows.rs")]
pub mod ctrlc;
pub mod install_progress;
pub mod multi_progress_report;
pub mod progress_report;
pub mod prompt;
pub mod resolve_progress;
pub mod style;
pub mod sudo;
pub mod table;
pub(crate) mod text_install_progress;
pub mod theme;
pub mod time;
pub(crate) mod tty_install_progress;

static INTERRUPT_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Register work to do on every Ctrl-C, such as cancelling in-flight HTTP requests.
pub fn set_interrupt_hook(hook: fn()) {
    let _ = INTERRUPT_HOOK.set(hook);
}

#[cfg(any(unix, windows))]
fn run_interrupt_hook() {
    if let Some(hook) = INTERRUPT_HOOK.get() {
        hook();
    }
}

/// Whether unit tests are running, this crate's own or mise's. `cfg(test)` does not reach a
/// dependency, so mise's harness flips `mise_util::testing` instead.
pub(crate) fn in_tests() -> bool {
    cfg!(test) || mise_util::testing::in_tests()
}

#[cfg(test)]
mod test_harness {
    mise_util::run_before_main!(init);

    /// This crate's own tests have no mise config system to load settings from.
    fn init() {
        mise_settings::set_loader(mise_settings::load_defaults);
        mise_util::deprecation::set_version(env!("CARGO_PKG_VERSION"));
    }
}
