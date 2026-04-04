use std::path::PathBuf;

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
pub struct SandboxConfig {
    pub deny_read: bool,
    pub deny_write: bool,
    pub deny_net: bool,
    pub deny_env: bool,
    pub allow_read: Vec<PathBuf>,
    pub allow_write: Vec<PathBuf>,
    pub allow_net: Vec<String>,
    pub allow_env: Vec<String>,
}

/// Minimal env vars inherited when deny_env is active.
const DEFAULT_ENV_KEYS: &[&str] = &["PATH", "HOME", "USER", "SHELL", "TERM", "LANG"];

impl SandboxConfig {
    /// Returns true if any sandbox restriction is configured.
    pub fn is_active(&self) -> bool {
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
    pub fn resolve_paths(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_default();
        let resolve = |paths: &mut Vec<PathBuf>| {
            for p in paths.iter_mut() {
                if p.is_relative() {
                    *p = cwd.join(&p);
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
    pub fn effective_deny_read(&self) -> bool {
        self.deny_read || !self.allow_read.is_empty()
    }

    pub fn effective_deny_write(&self) -> bool {
        self.deny_write || !self.allow_write.is_empty()
    }

    pub fn effective_deny_net(&self) -> bool {
        self.deny_net || !self.allow_net.is_empty()
    }

    pub fn effective_deny_env(&self) -> bool {
        self.deny_env || !self.allow_env.is_empty()
    }

    /// Filter environment variables based on sandbox config.
    ///
    /// When deny_env is active, starts with the mise-computed env (tool paths etc.),
    /// keeps only essential vars + allow_env entries, and also pulls in allow_env
    /// vars from the parent process environment if not already present.
    pub fn filter_env(
        &self,
        env: &std::collections::BTreeMap<String, String>,
    ) -> std::collections::BTreeMap<String, String> {
        if !self.effective_deny_env() {
            return env.clone();
        }
        let mut filtered: std::collections::BTreeMap<String, String> = env
            .iter()
            .filter(|(k, _)| DEFAULT_ENV_KEYS.contains(&k.as_str()) || self.allow_env.contains(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Pull in allowed vars from parent env that might not be in mise's env map
        for key in &self.allow_env {
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
    #[allow(unused_variables)]
    pub async fn apply(
        &self,
        program: &str,
        args: &[String],
    ) -> eyre::Result<Option<SandboxedCommand>> {
        if !self.is_active() {
            return Ok(None);
        }

        #[cfg(target_os = "linux")]
        {
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
#[derive(Debug)]
pub struct SandboxedCommand {
    pub program: String,
    pub args: Vec<String>,
}

// Public functions for use by cmd.rs (which can't access private submodules)

/// Apply Landlock filesystem restrictions (Linux only).
#[cfg(target_os = "linux")]
pub fn landlock_apply(config: &SandboxConfig) -> eyre::Result<()> {
    landlock::apply_landlock(config)
}

/// Apply seccomp network filter (Linux only).
#[cfg(target_os = "linux")]
pub fn seccomp_apply() -> eyre::Result<()> {
    seccomp::apply_seccomp_net_filter()
}

/// Generate a macOS Seatbelt profile string (macOS only).
#[cfg(target_os = "macos")]
pub async fn macos_generate_profile(config: &SandboxConfig) -> String {
    macos::generate_seatbelt_profile(config).await
}
