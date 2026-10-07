use std::collections::HashSet;
use std::sync::LazyLock;
use std::sync::Mutex;

/// Like `eprintln!`, but ignores write failures instead of panicking when
/// stderr is unwritable (e.g. the process that spawned mise closed its end
/// of the pipe).
#[macro_export]
macro_rules! safe_eprintln {
    ($($arg:tt)*) => {{
        let _ = calm_io::stderrln!($($arg)*);
    }};
}

#[macro_export]
macro_rules! prefix_println {
    ($prefix:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let _ = calm_io::stdoutln!("{} {}", $prefix, msg);
    }};
}
#[macro_export]
macro_rules! prefix_eprintln {
    ($prefix:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::safe_eprintln!("{} {}", $prefix, msg);
    }};
}

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
        if $crate::env::should_truncate() {
            let msg = console::truncate_str(&msg, *$crate::env::TERM_WIDTH, "…");
            info!("{msg}");
        } else {
            info!("{msg}");
        }
    }};
}

#[cfg(not(test))]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
       log::warn!($($arg)*);
    }};
}

pub(crate) static WARNED_ONCE: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
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

pub static DEPRECATED: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Default::default);

#[macro_export]
macro_rules! deprecated {
    ($id:tt, $($arg:tt)*) => {{
        if $crate::output::DEPRECATED.lock().unwrap().insert($id) {
            warn!("deprecated [{}]: {}", $id, format!($($arg)*));
        }
    }};
}

/// Emits a deprecation warning when mise version >= warn_at, and fires a debug_assert
/// when version >= remove_at to remind developers to remove the deprecated code.
/// The removal version is automatically appended to the warning message.
///
/// # Example
/// ```ignore
/// deprecated_at!("2026.3.0", "2027.3.0", "legacy-syntax", "Use {{version}} instead of {version}");
/// ```
#[macro_export]
macro_rules! deprecated_at {
    ($warn_at:tt, $remove_at:tt, $id:tt, $($arg:tt)*) => {{
        use versions::Versioning;
        let warn_version = Versioning::new($warn_at).expect("invalid warn_at version in deprecated_at!");
        let remove_version = Versioning::new($remove_at).expect("invalid remove_at version in deprecated_at!");
        debug_assert!(
            *$crate::version::V < remove_version,
            "Deprecated code [{}] should have been removed in version {}. Please remove this deprecated functionality.",
            $id, $remove_at
        );
        if *$crate::version::V >= warn_version {
            if $crate::output::DEPRECATED.lock().unwrap().insert($id) {
                warn!("deprecated [{}]: {} This will be removed in mise {}.", $id, format!($($arg)*), $remove_at);
            }
        }
    }};
}

/// Fails the build once mise's own version reaches `remove_at`, for code that
/// only has to live for a while, such as a migration or a fallback that reads
/// what an older mise wrote. Unlike [`deprecated_at!`], it needs no code path
/// to run: the release that bumps the version cannot build until the code
/// carrying it is removed.
///
/// # Example
/// ```ignore
/// remove_by!("2027.1.0", "legacy-cache-markers");
/// ```
#[macro_export]
macro_rules! remove_by {
    ($remove_at:literal, $id:literal) => {
        const _: () = assert!(
            $crate::version::version_lt(env!("CARGO_PKG_VERSION"), $remove_at),
            concat!(
                "[",
                $id,
                "] should have been removed by mise ",
                $remove_at,
                ". Remove it, and every other remove_by! with this id."
            )
        );
    };
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    pub(crate) static STDOUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pub(crate) static STDERR: Mutex<Vec<String>> = Mutex::new(Vec::new());
}
