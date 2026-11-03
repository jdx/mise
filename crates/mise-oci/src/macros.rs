//! The logging macros the OCI modules use, as thin wrappers over `log`.

macro_rules! debug {
    ($($arg:tt)*) => {{
        log::debug!($($arg)*);
    }};
}

macro_rules! info {
    ($($arg:tt)*) => {{
        log::info!($($arg)*);
    }};
}

macro_rules! warn {
    ($($arg:tt)*) => {{
        log::warn!($($arg)*);
    }};
}
