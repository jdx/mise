#[cfg(test)]
#[macro_export]
macro_rules! rtxprintln {
    () => {
        rtxprint!("\n")
    };
    ($($arg:tt)*) => {{
        let mut stdout = $crate::output::tests::STDOUT.lock().unwrap();
        stdout.push(format!($($arg)*));
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxprintln {
    () => {
        rtxprint!("\n")
    };
    ($($arg:tt)*) => {{
        println!($($arg)*);
    }};
}

#[cfg(test)]
#[macro_export]
macro_rules! rtxprint {
    ($($arg:tt)*) => {{
        let mut stdout = $crate::output::tests::STDOUT.lock().unwrap();
        let cur = stdout.pop().unwrap_or_default();
        stdout.push(cur + &format!($($arg)*));
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxprint {
    ($($arg:tt)*) => {{
        print!($($arg)*);
    }};
}

#[cfg(test)]
#[macro_export]
macro_rules! rtxstatusln {
        ($($arg:tt)*) => {{
            let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
            let rtx = console::style("rtx ").dim().for_stderr();
            stderr.push(format!("{}{}", rtx, format!($($arg)*)));
        }};
    }

#[cfg(test)]
#[macro_export]
macro_rules! rtxwarn {
        ($($arg:tt)*) => {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
                let rtx = console::style("rtx ").yellow().for_stderr();
                stderr.push(format!("{}{}", rtx, format!($($arg)*)));
            })
        }
    }

#[cfg(test)]
#[macro_export]
macro_rules! rtxerror {
        ($($arg:tt)*) => {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
                let rtx = console::style("rtx ").red().for_stderr();
                stderr.push(format!("{}{}", rtx, format!($($arg)*)));
            })
        }
    }

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxstatusln {
    ($($arg:tt)*) => {{
        if log_enabled!(log::Level::Info) {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let rtx = console::style("rtx ").dim().for_stderr();
                eprintln!("{}{}", rtx, format!($($arg)*));
            });
        }
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxwarn {
    ($($arg:tt)*) => {{
        if log_enabled!(log::Level::Warn) {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let rtx = console::style("rtx ").yellow().for_stderr();
                eprintln!("{}{}", rtx, format!($($arg)*));
            });
        }
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! rtxerror {
    ($($arg:tt)*) => {{
        if log_enabled!(log::Level::Error) {
            $crate::ui::multi_progress_report::MultiProgressReport::suspend_if_active(|| {
                let rtx = console::style("rtx ").red().for_stderr();
                eprintln!("{}{}", rtx, format!($($arg)*));
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
