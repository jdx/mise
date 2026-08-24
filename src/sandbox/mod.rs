use std::path::PathBuf;

use crate::file::replace_path;

#[cfg(target_os = "linux")]
mod landlock;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod seccomp;

/// Configuration for process sandboxing.
///
/// Any `deny_*` or `allow_*` field being set implicitly enables sandboxing.
/// `allow_*` fields imply their corresponding `deny_*` (e.g., `allow_write` implies `deny_write`
/// for everything not in the allow list).
#[derive(Debug, Clone, Default)]
pub(crate) struct SandboxConfig {
    pub deny_read: bool,
    pub deny_write: bool,
    pub deny_net: bool,
    pub deny_env: bool,
    pub allow_read: Vec<PathBuf>,
    pub allow_write: Vec<PathBuf>,
    pub allow_net: Vec<String>,
    pub allow_env: Vec<String>,
    /// Environment patterns that survive an active env sandbox without enabling it themselves.
    pub pass_through_env: Vec<String>,
    /// Exact hashed environment names that survive an active env sandbox.
    pub cache_env: Vec<String>,
}

/// Minimal env vars inherited when deny_env is active.
const DEFAULT_ENV_KEYS: &[&str] = &["PATH", "HOME", "USER", "SHELL", "TERM", "COLORTERM", "LANG"];

/// The closest ancestor that exists, and so the only one a Landlock rule can
/// name.
///
/// `parent()` is not enough: for `a/b/c` where `a/b` is missing too, allowing
/// `a/b` would be dropped for exactly the same reason.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn nearest_existing_ancestor(path: &std::path::Path) -> Option<&std::path::Path> {
    // Confirmed to exist, not merely "no error saying otherwise": pointing the
    // user at a directory we could not stat would be the same unchecked claim.
    path.ancestors()
        .skip(1)
        .find(|ancestor| matches!(ancestor.try_exists(), Ok(true)))
}

/// Fold `.` and `..` so two spellings of one missing path are reported once.
///
/// Only ever a deduplication key, never displayed. A path that does not exist
/// cannot be canonicalized, so this is lexical: if a `..` crosses a symlink the
/// fold is not what the kernel would resolve. The cost of being wrong is one
/// merged warning, so lexical is the right trade here.
fn dedup_key(path: &std::path::Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Check if an env var name matches an allow_env pattern.
/// Patterns can contain `*` as a wildcard (e.g., `MYAPP_*` matches `MYAPP_FOO`).
/// Patterns without `*` require an exact match.
fn env_pattern_matches(pattern: &str, key: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == key;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 2 {
        // Common case: single wildcard (prefix*, *suffix, or prefix*suffix)
        let (prefix, suffix) = (parts[0], parts[1]);
        return prefix.len() + suffix.len() <= key.len()
            && key.starts_with(prefix)
            && key.ends_with(suffix);
    }
    // Multiple wildcards: use globset
    globset::Glob::new(pattern)
        .map(|g| g.compile_matcher().is_match(key))
        .unwrap_or(false)
}

impl SandboxConfig {
    /// Build sandbox configuration by combining persistent deny settings with CLI options.
    pub(crate) fn from_settings_and_cli(
        settings: &crate::config::settings::SettingsSandbox,
        cli_deny_all: bool,
        mut cli: Self,
    ) -> Self {
        cli.deny_read |= settings.deny_all || settings.deny_read || cli_deny_all;
        cli.deny_write |= settings.deny_all || settings.deny_write || cli_deny_all;
        cli.deny_net |= settings.deny_all || settings.deny_net || cli_deny_all;
        cli.deny_env |= settings.deny_all || settings.deny_env || cli_deny_all;
        cli
    }

    /// Returns true if any sandbox restriction is configured.
    pub(crate) fn is_active(&self) -> bool {
        self.deny_read
            || self.deny_write
            || self.deny_net
            || self.deny_env
            || !self.allow_read.is_empty()
            || !self.allow_write.is_empty()
            || !self.allow_net.is_empty()
            || !self.allow_env.is_empty()
    }

    /// Resolve allow_* paths to absolute paths relative to cwd.
    pub(crate) fn resolve_paths(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_default();
        let resolve = |paths: &mut Vec<PathBuf>| {
            paths.retain(|p| !p.as_os_str().is_empty());
            for p in paths.iter_mut() {
                *p = replace_path(&*p);
                if p.is_relative() {
                    *p = cwd.join(&*p);
                }
                // Canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
                if let Ok(canonical) = p.canonicalize() {
                    *p = canonical;
                }
            }
        };
        resolve(&mut self.allow_read);
        resolve(&mut self.allow_write);
    }

    /// Compute effective deny flags, accounting for allow_* implying deny_*.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn effective_deny_read(&self) -> bool {
        self.deny_read || !self.allow_read.is_empty()
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn effective_deny_write(&self) -> bool {
        self.deny_write || !self.allow_write.is_empty()
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn effective_deny_net(&self) -> bool {
        self.deny_net || !self.allow_net.is_empty()
    }

    pub(crate) fn effective_deny_env(&self) -> bool {
        self.deny_env || !self.allow_env.is_empty()
    }

    /// Allow-list paths that do not exist, in declaration order and listed once
    /// even when named by both `allow_read` and `allow_write`.
    ///
    /// Landlock binds a rule to an open descriptor, so it cannot name a path
    /// that is not there yet and the rule is dropped — see
    /// <https://github.com/jdx/mise/discussions/10556>. Not gated on Linux:
    /// this is only a set of existence checks, and keeping it portable keeps it
    /// testable on any host.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn missing_allow_paths(&self) -> Vec<&std::path::Path> {
        let mut seen = std::collections::HashSet::new();
        self.allow_read
            .iter()
            .chain(self.allow_write.iter())
            // Only paths confirmed absent. `exists()` would fold a metadata
            // error — no listing permission on a parent, say — into "missing"
            // and report a cause nothing checked, which is the habit this whole
            // change is removing. Those land in `add_path_rule` instead.
            .filter(|path| matches!(path.try_exists(), Ok(false)))
            .filter(|path| seen.insert(dedup_key(path)))
            .map(|path| path.as_path())
            .collect()
    }

    /// Report dropped rules before the sandbox starts denying things.
    ///
    /// Reported from the parent rather than from `add_path_rule`, which runs
    /// inside `pre_exec` — after fork, where the logger is not available and
    /// the same path warns once per allow-list it appears in.
    #[cfg(target_os = "linux")]
    pub(crate) fn warn_missing_allow_paths(&self) {
        for path in self.missing_allow_paths() {
            let workaround = match nearest_existing_ancestor(path) {
                Some(dir) => format!(
                    "To let the task create it, allow a directory that does exist — the closest is {}.",
                    crate::file::display_path(dir)
                ),
                None => {
                    "To let the task create it, allow a directory that does exist and contains it."
                        .to_string()
                }
            };
            warn!(
                "sandbox: {} does not exist, so its rule was dropped.\n\
                 Landlock can only bind rules to paths that already exist. {workaround}",
                crate::file::display_path(path)
            );
        }
    }

    /// Filter environment variables based on sandbox config.
    ///
    /// When deny_env is active, starts with the mise-computed env (tool paths etc.),
    /// keeps only essential vars + allow_env entries, and also pulls in allow_env
    /// vars from the parent process environment if not already present.
    pub(crate) fn filter_env(
        &self,
        env: &std::collections::BTreeMap<String, String>,
    ) -> std::collections::BTreeMap<String, String> {
        if !self.effective_deny_env() {
            return env.clone();
        }
        let env_patterns = self.allow_env.iter().chain(&self.pass_through_env);
        let env_matches = |k: &str| {
            self.cache_env.iter().any(|name| name == k)
                || env_patterns
                    .clone()
                    .any(|pattern| env_pattern_matches(pattern, k))
        };
        let mut filtered: std::collections::BTreeMap<String, String> = env
            .iter()
            .filter(|(k, _)| DEFAULT_ENV_KEYS.contains(&k.as_str()) || env_matches(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Pull in allowed vars from parent env that might not be in mise's env map.
        // For wildcard patterns, check all parent env vars; for exact names, check directly.
        for pattern in self.allow_env.iter().chain(&self.pass_through_env) {
            if pattern.contains('*') {
                for (key, val) in crate::env::vars_safe() {
                    if !filtered.contains_key(&key) && env_pattern_matches(pattern, &key) {
                        filtered.insert(key, val);
                    }
                }
            } else if !filtered.contains_key(pattern)
                && let Ok(val) = std::env::var(pattern)
            {
                filtered.insert(pattern.clone(), val);
            }
        }
        for key in &self.cache_env {
            if !filtered.contains_key(key)
                && let Ok(val) = std::env::var(key)
            {
                filtered.insert(key.clone(), val);
            }
        }
        // Also ensure essential vars from parent env are present
        for key in DEFAULT_ENV_KEYS {
            let k = key.to_string();
            if !filtered.contains_key(&k)
                && let Ok(val) = std::env::var(key)
            {
                filtered.insert(k, val);
            }
        }
        filtered
    }

    /// Apply filesystem and network sandboxing before exec (for `mise x`).
    ///
    /// On Linux: applies Landlock rules and seccomp filters in-process (inherited across exec).
    /// On macOS: returns a modified command that wraps through sandbox-exec.
    #[cfg(not(test))]
    #[cfg_attr(windows, allow(dead_code))]
    #[allow(unused_variables)]
    pub(crate) async fn apply(
        &self,
        program: &str,
        args: &[String],
    ) -> eyre::Result<Option<SandboxedCommand>> {
        if !self.is_active() {
            return Ok(None);
        }

        #[cfg(target_os = "linux")]
        {
            self.warn_missing_allow_paths();
            self.apply_linux()?;
            Ok(None)
        }

        #[cfg(target_os = "macos")]
        {
            return self.apply_macos(program, args).await;
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            warn!("sandbox is not supported on this platform, running unsandboxed");
            Ok(None)
        }
    }

    #[cfg(all(not(test), target_os = "linux"))]
    fn apply_linux(&self) -> eyre::Result<()> {
        if self.effective_deny_read() || self.effective_deny_write() {
            landlock::apply_landlock(self)?;
        }
        if self.effective_deny_net() {
            if !self.allow_net.is_empty() {
                eyre::bail!(
                    "per-host network filtering (--allow-net=<host>) is not supported on Linux. \
                     Use --deny-net to block all network, or remove --allow-net."
                );
            }
            seccomp::apply_seccomp_net_filter()?;
        }
        Ok(())
    }

    #[cfg(all(not(test), target_os = "macos"))]
    async fn apply_macos(
        &self,
        program: &str,
        args: &[String],
    ) -> eyre::Result<Option<SandboxedCommand>> {
        let profile = macos::generate_seatbelt_profile(self).await;
        let mut sandbox_args = vec![
            "-p".to_string(),
            profile,
            "--".to_string(),
            program.to_string(),
        ];
        sandbox_args.extend(args.iter().cloned());
        Ok(Some(SandboxedCommand {
            program: "sandbox-exec".to_string(),
            args: sandbox_args,
        }))
    }
}

/// A command rewritten to run through a sandbox wrapper (macOS sandbox-exec).
#[cfg(not(test))]
#[cfg_attr(windows, allow(dead_code))]
#[derive(Debug)]
pub(crate) struct SandboxedCommand {
    pub program: String,
    pub args: Vec<String>,
}

// Public functions for use by cmd.rs (which can't access private submodules)

/// Apply Landlock filesystem restrictions (Linux only).
#[cfg(target_os = "linux")]
pub(crate) fn landlock_apply(config: &SandboxConfig) -> eyre::Result<()> {
    landlock::apply_landlock(config)
}

/// Apply seccomp network filter (Linux only).
#[cfg(target_os = "linux")]
pub(crate) fn seccomp_apply() -> eyre::Result<()> {
    seccomp::apply_seccomp_net_filter()
}

/// Generate a macOS Seatbelt profile string (macOS only).
#[cfg(target_os = "macos")]
pub(crate) async fn macos_generate_profile(config: &SandboxConfig) -> String {
    macos::generate_seatbelt_profile(config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::SettingsSandbox;
    use std::collections::BTreeMap;

    /// Fixture paths that cannot collide with a previous run or a concurrent one.
    fn missing_fixture(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mise-10556-{}-{name}", std::process::id()))
    }

    #[test]
    fn test_missing_allow_paths_lists_each_absent_path_once() {
        // Landlock drops a rule naming a path that is not there yet, and the
        // same path commonly appears in both allow-lists — the report in
        // discussion #10556 shows the warning printed twice for one path.
        let existing = std::env::temp_dir();
        let missing_a = missing_fixture("a");
        let missing_b = missing_fixture("b");
        assert!(existing.exists(), "temp_dir should exist");
        assert!(!missing_a.exists(), "fixture path should not exist");
        assert!(!missing_b.exists(), "fixture path should not exist");

        let config = SandboxConfig {
            allow_read: vec![existing.clone(), missing_a.clone(), missing_b.clone()],
            allow_write: vec![missing_a.clone(), existing],
            ..Default::default()
        };

        assert_eq!(
            config.missing_allow_paths(),
            vec![missing_a.as_path(), missing_b.as_path()]
        );
    }

    #[test]
    fn test_missing_allow_paths_folds_equivalent_spellings() {
        // A missing path cannot be canonicalized, so `a/../b` and `b` stay
        // distinct to `Path` and would each warn.
        let missing = missing_fixture("dotdot");
        let detoured = std::env::temp_dir()
            .join("nested")
            .join("..")
            .join(missing.file_name().unwrap());
        assert_ne!(missing.as_path(), detoured.as_path());

        let config = SandboxConfig {
            allow_read: vec![missing.clone()],
            allow_write: vec![detoured],
            ..Default::default()
        };

        assert_eq!(config.missing_allow_paths(), vec![missing.as_path()]);
    }

    #[test]
    fn test_nearest_existing_ancestor_skips_missing_parents() {
        let existing = std::env::temp_dir();
        // The immediate parent is missing too, so naming it in the warning
        // would send the user to a rule that is dropped for the same reason.
        let nested = missing_fixture("outer").join("inner").join("file");
        assert_eq!(nearest_existing_ancestor(&nested), Some(existing.as_path()));
    }

    #[test]
    fn test_missing_allow_paths_is_empty_when_everything_exists() {
        let config = SandboxConfig {
            allow_write: vec![std::env::temp_dir()],
            ..Default::default()
        };
        assert!(config.missing_allow_paths().is_empty());
    }

    #[test]
    fn test_env_pattern_matches_exact() {
        assert!(env_pattern_matches("FOO", "FOO"));
        assert!(!env_pattern_matches("FOO", "FOOBAR"));
        assert!(!env_pattern_matches("FOO", "BAR"));
    }

    #[test]
    fn test_env_pattern_matches_prefix_wildcard() {
        assert!(env_pattern_matches("MYAPP_*", "MYAPP_FOO"));
        assert!(env_pattern_matches("MYAPP_*", "MYAPP_"));
        assert!(!env_pattern_matches("MYAPP_*", "MYAPP"));
        assert!(!env_pattern_matches("MYAPP_*", "OTHER_FOO"));
    }

    #[test]
    fn test_env_pattern_matches_suffix_wildcard() {
        assert!(env_pattern_matches("*_SECRET", "MY_SECRET"));
        assert!(env_pattern_matches("*_SECRET", "_SECRET"));
        assert!(!env_pattern_matches("*_SECRET", "SECRET"));
    }

    #[test]
    fn test_env_pattern_matches_infix_wildcard() {
        assert!(env_pattern_matches("MY_*_SECRET", "MY_APP_SECRET"));
        assert!(env_pattern_matches("MY_*_SECRET", "MY__SECRET"));
        // key too short for both prefix and suffix without overlap
        assert!(!env_pattern_matches("MY_*_SECRET", "MY_SECRET"));
        assert!(!env_pattern_matches("AB*B", "AB"));
    }

    #[test]
    fn test_env_pattern_matches_star_only() {
        assert!(env_pattern_matches("*", "ANYTHING"));
        assert!(env_pattern_matches("*", ""));
    }

    #[test]
    fn test_filter_env_with_wildcard() {
        let config = SandboxConfig {
            allow_env: vec!["MYAPP_*".to_string()],
            ..Default::default()
        };
        let mut env = BTreeMap::new();
        env.insert("MYAPP_FOO".to_string(), "val1".to_string());
        env.insert("MYAPP_BAR".to_string(), "val2".to_string());
        env.insert("OTHER_VAR".to_string(), "val3".to_string());
        env.insert("PATH".to_string(), "/usr/bin".to_string());

        let filtered = config.filter_env(&env);
        assert!(filtered.contains_key("MYAPP_FOO"));
        assert!(filtered.contains_key("MYAPP_BAR"));
        assert!(!filtered.contains_key("OTHER_VAR"));
        assert!(filtered.contains_key("PATH")); // default key
    }

    #[test]
    fn test_pass_through_env_only_filters_when_env_is_denied() {
        let mut env = BTreeMap::new();
        env.insert("PASSED_SECRET".to_string(), "secret".to_string());
        env.insert("OTHER_VAR".to_string(), "other".to_string());
        let loose = SandboxConfig {
            pass_through_env: vec!["PASSED_*".to_string()],
            ..Default::default()
        };

        assert!(!loose.effective_deny_env());
        assert_eq!(loose.filter_env(&env), env);

        let strict = SandboxConfig {
            deny_env: true,
            ..loose
        };
        let filtered = strict.filter_env(&env);
        assert_eq!(
            filtered.get("PASSED_SECRET").map(String::as_str),
            Some("secret")
        );
        assert!(!filtered.contains_key("OTHER_VAR"));
    }

    #[test]
    fn test_cache_env_uses_exact_names() {
        let config = SandboxConfig {
            deny_env: true,
            cache_env: vec!["HASHED_*".to_string(), "EXACT".to_string()],
            ..Default::default()
        };
        let env = BTreeMap::from([
            ("HASHED_VALUE".to_string(), "not-selected".to_string()),
            ("EXACT".to_string(), "selected".to_string()),
        ]);

        let filtered = config.filter_env(&env);
        assert!(!filtered.contains_key("HASHED_VALUE"));
        assert_eq!(filtered.get("EXACT").map(String::as_str), Some("selected"));
    }

    /// filter_env() walks the parent environment for wildcard allow_env
    /// patterns; a non-UTF-8 parent var must be skipped, not panic (#5370).
    #[cfg(unix)]
    #[test]
    fn test_filter_env_wildcard_skips_invalid_utf8_parent_var() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let key = "MISE_TEST_SANDBOX_INVALID";
        let config = SandboxConfig {
            allow_env: vec!["MISE_TEST_SANDBOX_*".to_string()],
            ..Default::default()
        };
        // restores the environment on drop, even if the assertion below fails
        let mut guard = crate::test::EnvVarGuard::new();
        guard.set(key, OsString::from_vec(vec![0xff]));

        let filtered = config.filter_env(&BTreeMap::new());

        assert!(!filtered.contains_key(key));
    }

    #[test]
    fn test_resolve_paths_drops_empty_paths() {
        let mut config = SandboxConfig {
            allow_read: vec![PathBuf::new()],
            allow_write: vec![PathBuf::from("")],
            ..Default::default()
        };

        config.resolve_paths();

        assert!(config.allow_read.is_empty());
        assert!(config.allow_write.is_empty());
    }

    #[test]
    fn test_from_settings_and_cli_combines_denies() {
        let settings = SettingsSandbox {
            deny_read: true,
            deny_net: true,
            ..Default::default()
        };
        let config = SandboxConfig::from_settings_and_cli(
            &settings,
            false,
            SandboxConfig {
                deny_write: true,
                allow_env: vec!["ALLOWED".to_string()],
                ..Default::default()
            },
        );

        assert!(config.deny_read);
        assert!(config.deny_write);
        assert!(config.deny_net);
        assert!(!config.deny_env);
        assert_eq!(config.allow_env, ["ALLOWED"]);
    }

    #[test]
    fn test_from_settings_and_cli_expands_deny_all() {
        let settings = SettingsSandbox {
            deny_all: true,
            ..Default::default()
        };
        let config =
            SandboxConfig::from_settings_and_cli(&settings, false, SandboxConfig::default());

        assert!(config.deny_read);
        assert!(config.deny_write);
        assert!(config.deny_net);
        assert!(config.deny_env);
    }
}
