use crate::config::settings::SETTINGS;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::Mutex;

#[cfg(test)]
#[macro_export]
macro_rules! miseprintln {
    () => {
        miseprint!("\n")?;
    };
    ($($arg:tt)*) => {{
        let mut stdout = $crate::output::tests::STDOUT.lock().unwrap();
        stdout.push(format!($($arg)*));
    }}
}

#[cfg(not(test))]
#[macro_export]
macro_rules! miseprintln {
    () => {
        calm_io::stdoutln!()?;
    };
    ($($arg:tt)*) => {{
        calm_io::stdoutln!($($arg)*)?;
    }}
}

#[cfg(test)]
#[macro_export]
macro_rules! miseprint {
    ($($arg:tt)*) => {{
        let mut stdout = $crate::output::tests::STDOUT.lock().unwrap();
        let cur = stdout.pop().unwrap_or_default();
        stdout.push(cur + &format!($($arg)*));
        std::io::Result::Ok(())
    }}
}

#[cfg(not(test))]
#[macro_export]
macro_rules! miseprint {
    ($($arg:tt)*) => {{
        calm_io::stdout!($($arg)*)
    }}
}

#[cfg(test)]
#[macro_export]
macro_rules! info {
        ($($arg:tt)*) => {{
            let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
            let mise = console::style("mise").dim().for_stderr();
            stderr.push(format!("{} {}", mise, format!($($arg)*)));
        }};
    }

#[cfg(test)]
#[macro_export]
macro_rules! warn {
        ($($arg:tt)*) => {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
                let mise = console::style("mise").yellow().for_stderr();
                stderr.push(format!("{} {}", mise, format!($($arg)*)));
            })
        }
    }

#[cfg(test)]
#[macro_export]
macro_rules! error {
        ($($arg:tt)*) => {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
                let mise = console::style("mise").red().for_stderr();
                stderr.push(format!("{} {}", mise, format!($($arg)*)));
            })
        }
    }

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        log::trace!($($arg)*);
    }};
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        log::debug!($($arg)*);
    }};
}

pub fn should_display_hint(id: &str) -> bool {
    if cfg!(test) {
        return false;
    }
    if SETTINGS
        .disable_hints
        .iter()
        .any(|hint| hint == id || hint == "*")
    {
        return false;
    }
    if !console::user_attended() {
        return false;
    }
    static DISPLAYED_HINTS: Lazy<Mutex<HashSet<String>>> = Lazy::new(Default::default);
    let displayed_hints = &mut DISPLAYED_HINTS.lock().unwrap();
    if displayed_hints.contains(id) {
        return false;
    }
    displayed_hints.insert(id.to_string());
    true
}

#[macro_export]
macro_rules! hint {
    ($id:expr, $message:expr, $example_cmd:expr) => {{
        if $crate::output::should_display_hint($id) {
            let prefix = console::style("mise ").dim().for_stderr().to_string();
            let prefix = prefix
                + console::style("hint")
                    .dim()
                    .yellow()
                    .for_stderr()
                    .to_string()
                    .as_str();
            let cmd = console::style($example_cmd).bold().for_stderr();
            let disable_single = console::style(format!("mise settings set disable_hints {}", $id))
                .bold()
                .for_stderr();
            let disable_all = console::style("mise settings set disable_hints \"*\"")
                .bold()
                .for_stderr();
            info_unprefix!("{} {} {}", prefix, format!($message), cmd);
            info_unprefix!(
                "{} disable this hint with {} or all with {}",
                prefix,
                disable_single,
                disable_all
            );
        }
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        let mise = console::style("mise").dim().for_stderr();
        info_unprefix!("{} {}", mise, format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! info_unprefix {
    ($($arg:tt)*) => {{
        if log::log_enabled!(log::Level::Debug) {
           log::info!($($arg)*);
        } else if log::log_enabled!(log::Level::Info) {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                eprintln!("{}", format!($($arg)*));
            });
        }
    }};
}

#[macro_export]
macro_rules! info_unprefix_trunc {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let msg = msg.lines().next().unwrap_or_default();
        let msg = console::truncate_str(&msg, *$crate::env::TERM_WIDTH, "…");
        info_unprefix!("{msg}");
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        if log::log_enabled!(log::Level::Debug) {
           log::warn!($($arg)*);
        } else if log::log_enabled!(log::Level::Warn) {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let mise = console::style("mise ").yellow().for_stderr();
                eprintln!("{}{}", mise, format!($($arg)*));
            });
        }
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        if log::log_enabled!(log::Level::Debug) {
           log::error!($($arg)*);
        } else if log::log_enabled!(log::Level::Error) {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let mise = console::style("mise ").red().for_stderr();
                eprintln!("{}{}", mise, format!($($arg)*));
            });
        }
    }};
}

#[cfg(test)]
pub mod tests {
    use std::sync::Mutex;

    pub static STDOUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pub static STDERR: Mutex<Vec<String>> = Mutex::new(Vec::new());
}
