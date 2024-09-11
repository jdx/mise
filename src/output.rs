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
macro_rules! hint {
    ($arg1:expr, $arg2:expr, $arg3:expr) => {{
        let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
        if !$crate::config::Settings::get()
            .disable_hints
            .iter()
            .any(|hint| hint == $arg1 || hint == "*")
            || !console::user_attended()
        {
            let prefix = console::style("mise hint").dim().for_stderr();
            let cmd = console::style($arg3).bold().for_stderr();
            stderr.push(format!("{} {} {}", prefix, format!($arg2), cmd));
        }
    }};
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

#[cfg(not(test))]
#[macro_export]
macro_rules! hint {
    ($arg1:expr, $arg2:expr, $arg3:expr) => {{
        if !$crate::config::Settings::get()
            .disable_hints
            .iter()
            .any(|hint| hint == $arg1 || hint == "*")
            && console::user_attended()
        {
            let prefix = console::style("mise ").dim().for_stderr().to_string();
            let prefix = prefix
                + console::style("hint")
                    .dim()
                    .yellow()
                    .for_stderr()
                    .to_string()
                    .as_str();
            let cmd = console::style($arg3).bold().for_stderr();
            let disable_single =
                console::style(format!("mise settings set disable_hints {}", $arg1))
                    .bold()
                    .for_stderr();
            let disable_all = console::style("mise settings set disable_hints \"*\"")
                .bold()
                .for_stderr();
            info_unprefix!("{} {} {}", prefix, format!($arg2), cmd);
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
