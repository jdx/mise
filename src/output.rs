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

pub static WARNED_ONCE: Lazy<Mutex<HashSet<String>>> = Lazy::new(Default::default);
macro_rules! warn_once {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        if $crate::output::WARNED_ONCE.lock().unwrap().insert(msg.clone()) {
            warn!("{}", msg);
        }
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
       log::error!($($arg)*);
    }};
}

pub static DEPRECATED: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(Default::default);

#[macro_export]
macro_rules! deprecated {
    ($id:tt, $($arg:tt)*) => {{
        if $crate::output::DEPRECATED.lock().unwrap().insert($id) {
            warn!("deprecated [{}]: {}", $id, format!($($arg)*));
        }
    }};
}

#[cfg(test)]
pub mod tests {
    use std::sync::Mutex;

    pub static STDOUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pub static STDERR: Mutex<Vec<String>> = Mutex::new(Vec::new());
}
