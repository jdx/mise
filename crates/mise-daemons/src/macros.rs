//! The logging macros the daemon modules use, as thin wrappers over `log`.

macro_rules! debug {
    ($($arg:tt)*) => {{
        log::debug!($($arg)*);
    }};
}

macro_rules! warn {
    ($($arg:tt)*) => {{
        log::warn!($($arg)*);
    }};
}

/// `warn!`, but only the first time a given message is logged.
macro_rules! warn_once {
    ($($arg:tt)*) => {{
        static WARNED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
            std::sync::LazyLock::new(Default::default);
        let msg = format!($($arg)*);
        if WARNED.lock().unwrap().insert(msg.clone()) {
            log::warn!("{}", msg);
        }
    }};
}
