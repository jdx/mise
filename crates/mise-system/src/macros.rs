//! The logging and output macros the system modules use, as thin wrappers over `log` and
//! `calm_io`. mise's own versions capture output in mise's tests, which these modules never read.

macro_rules! debug {
    ($($arg:tt)*) => {{
        log::debug!($($arg)*);
    }};
}

macro_rules! trace {
    ($($arg:tt)*) => {{
        log::trace!($($arg)*);
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

/// Warn once per `id` that something is deprecated, from `warn_at` until it is removed in
/// `remove_at`.
macro_rules! deprecated_at {
    ($warn_at:tt, $remove_at:tt, $id:tt, $($arg:tt)*) => {{
        use versions::Versioning;
        let warn_version = Versioning::new($warn_at).expect("invalid warn_at version in deprecated_at!");
        let remove_version = Versioning::new($remove_at).expect("invalid remove_at version in deprecated_at!");
        let current = mise_util::deprecation::version();
        debug_assert!(
            *current < remove_version,
            "Deprecated code [{}] should have been removed in version {}. Please remove this deprecated functionality.",
            $id, $remove_at
        );
        if *current >= warn_version && mise_util::deprecation::DEPRECATED.lock().unwrap().insert($id) {
            log::warn!("deprecated [{}]: {} This will be removed in mise {}.", $id, format!($($arg)*), $remove_at);
        }
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

macro_rules! miseprint {
    ($($arg:tt)*) => {{
        calm_io::stdout!($($arg)*)
    }};
}
