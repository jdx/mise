//! The logging and output macros the package managers use, as thin wrappers over `log` and
//! `calm_io`. mise's own versions capture output in mise's tests, which these drivers never read.

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

/// Print a line to stdout, ignoring a closed pipe like `calm_io` does. Expands to `?`, so the
/// caller returns a `Result` whose error converts from `std::io::Error`.
macro_rules! miseprintln {
    () => {
        calm_io::stdoutln!()?;
    };
    ($($arg:tt)*) => {{
        calm_io::stdoutln!($($arg)*)?;
    }};
}
