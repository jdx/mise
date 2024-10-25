#[cfg(feature = "timings")]
use crate::ui::style;

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
        ($($arg:tt)*) => {{
            let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
            let mise = console::style("mise").yellow().for_stderr();
            stderr.push(format!("{} {}", mise, format!($($arg)*)));
        }}
    }

#[cfg(test)]
#[macro_export]
macro_rules! error {
        ($($arg:tt)*) => {
            let mut stderr = $crate::output::tests::STDERR.lock().unwrap();
            let mise = console::style("mise").red().for_stderr();
            stderr.push(format!("{} {}", mise, format!($($arg)*)));
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
macro_rules! info {
    ($($arg:tt)*) => {{
       log::info!($($arg)*);
    }};
}

#[cfg(feature = "timings")]
pub fn get_time_diff(module: &str, extra: &str) -> String {
    static PREV: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let now = std::time::Instant::now();
    if PREV.lock().unwrap().is_none() {
        *PREV.lock().unwrap() = Some(std::time::Instant::now());
    }
    let mut prev = PREV.lock().unwrap();
    let diff = now.duration_since(prev.unwrap());
    *prev = Some(now);
    let diff_str = crate::ui::time::format_duration(diff);
    let out = format!("[TIME] {module} {diff_str} {extra}")
        .trim()
        .to_string();
    if diff.as_micros() > 8000 {
        style::eblack(out).on_red().on_bright()
    } else if diff.as_micros() > 4000 {
        style::eblack(out).on_red()
    } else if diff.as_micros() > 2000 {
        style::ered(out).bright()
    } else if diff.as_micros() > 1000 {
        style::eyellow(out).bright()
    } else if diff.as_micros() > 500 {
        style::eyellow(out)
    } else if diff.as_micros() > 100 {
        style::ecyan(out)
    } else {
        style::edim(out)
    }
    .to_string()
}

#[cfg(not(feature = "timings"))]
pub fn get_time_diff(_module: &str, _extra: &str) -> String {
    "".to_string()
}

#[macro_export]
#[cfg(feature = "timings")]
macro_rules! time {
    () => {{
        if *$crate::env::MISE_TIMINGS {
            eprintln!("{}", $crate::output::get_time_diff(module_path!(), ""));
        }
    }};
    ($fn:expr) => {{
        if *$crate::env::MISE_TIMINGS {
            let module = format!("{}::{}", module_path!(), $fn);
            eprintln!("{}", $crate::output::get_time_diff(&module, ""));
        }
    }};
    ($fn:expr, $($arg:tt)+) => {{
        if *$crate::env::MISE_TIMINGS {
            let module = format!("{}::{}", module_path!(), $fn);
            let extra = format!($($arg)+);
            eprintln!("{}", $crate::output::get_time_diff(&module, &extra));
        }
    }};
}

#[macro_export]
#[cfg(not(feature = "timings"))]
macro_rules! time {
    () => {{}};
    ($fn:expr) => {{}};
    ($fn:expr, $($arg:tt)+) => {{}};
}

#[macro_export]
macro_rules! info_trunc {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let msg = msg.lines().next().unwrap_or_default();
        let msg = console::truncate_str(&msg, *$crate::env::TERM_WIDTH, "…");
        info!("{msg}");
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
       log::warn!($($arg)*);
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
       log::error!($($arg)*);
    }};
}

#[cfg(test)]
pub mod tests {
    use std::sync::Mutex;

    pub static STDOUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pub static STDERR: Mutex<Vec<String>> = Mutex::new(Vec::new());
}
