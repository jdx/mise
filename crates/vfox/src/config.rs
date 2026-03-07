use std::env::consts::{ARCH, OS};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone)]
pub struct Config {
    pub plugin_dir: PathBuf,
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

impl Config {
    pub fn get() -> Self {
        Self::_get().as_ref().unwrap().clone()
    }

    fn _get() -> MutexGuard<'static, Option<Config>> {
        let mut config = CONFIG.lock().unwrap();
        if config.is_none() {
            let home = homedir::my_home()
                .ok()
                .flatten()
                .unwrap_or_else(|| PathBuf::from("/"));
            *config = Some(Config {
                plugin_dir: home.join(".version-fox/plugin"),
            });
        }
        config
    }
}

pub fn os() -> String {
    match OS {
        "macos" => "darwin".to_string(),
        os => os.to_string(),
    }
}

pub fn arch() -> String {
    match ARCH {
        "aarch64" => "arm64".to_string(),
        "x86_64" => "amd64".to_string(),
        arch => arch.to_string(),
    }
}

/// Detect the libc environment type at runtime.
/// Returns `Some("gnu")` on glibc Linux, `Some("musl")` on musl Linux, `None` elsewhere.
// NOTE: This logic mirrors is_musl_system() in src/platform.rs. Keep in sync.
#[cfg(target_os = "linux")]
pub(crate) fn env_type() -> Option<String> {
    use once_cell::sync::Lazy;
    static ENV_TYPE: Lazy<Option<String>> = Lazy::new(|| {
        // If glibc's dynamic linker exists, this is a glibc system
        for dir in ["/lib", "/lib64"] {
            if has_file_prefix(dir, "ld-linux-") {
                return Some("gnu".to_string());
            }
        }
        // No glibc linker found — check for musl's
        if has_file_prefix("/lib", "ld-musl-") {
            return Some("musl".to_string());
        }
        None
    });
    ENV_TYPE.clone()
}

#[cfg(target_os = "linux")]
fn has_file_prefix(dir: &str, prefix: &str) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with(prefix))
        })
        .unwrap_or(false)
}

/// On non-Linux platforms, libc variant is not applicable.
#[cfg(not(target_os = "linux"))]
pub(crate) fn env_type() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os() {
        let os = os();
        assert!(!os.is_empty());
    }

    #[test]
    fn test_arch() {
        let arch = arch();
        assert!(!arch.is_empty());
    }

    #[test]
    fn test_env_type() {
        let et = env_type();
        match et.as_deref() {
            Some("gnu") | Some("musl") | None => {}
            other => panic!("unexpected env_type: {other:?}"),
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_env_type_non_linux_returns_none() {
        assert_eq!(env_type(), None);
    }
}
