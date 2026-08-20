use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::Arc;

use crate::file::replace_path;

#[cfg(target_os = "linux")]
mod landlock;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod seccomp;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemAccessProfile {
    /// Preserve the historical broad system-path policy for user-configured
    /// task and command sandboxes.
    #[default]
    Compatibility,
    /// Minimal system access for untrusted Homebrew formula execution.
    FormulaExecution,
}

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
    /// Also deny local/Unix-domain sockets when network access is denied.
    /// This is stricter than the default network sandbox, which keeps local
    /// IPC available for compatibility.
    pub deny_local_sockets: bool,
    pub deny_env: bool,
    pub allow_read: Vec<PathBuf>,
    pub allow_write: Vec<PathBuf>,
    pub allow_net: Vec<String>,
    pub allow_env: Vec<String>,
    /// Environment patterns that survive an active env sandbox without enabling it themselves.
    pub pass_through_env: Vec<String>,
    /// Exact hashed environment names that survive an active env sandbox.
    pub cache_env: Vec<String>,
    /// Do not grant the sandbox's usual broad write access to system temp.
    /// Callers using this must add an explicit private temp path to `allow_write`.
    pub deny_system_temp_write: bool,
    /// Do not grant the sandbox's usual broad read access to mise's data and
    /// install roots. Security-sensitive callers must allow exact tool roots.
    pub deny_mise_data_read: bool,
    /// Require every vetted Landlock ABI V6 filesystem right and reject
    /// partial enforcement. Source formula execution enables this; ordinary
    /// task sandboxes retain their compatibility behavior.
    pub require_full_filesystem_confinement: bool,
    /// Select the built-in system-path policy independently from enforcement
    /// compatibility. Formula execution must not inherit the broad task policy.
    pub system_access_profile: SystemAccessProfile,
    /// Parent-opened allow nodes for strict formula execution. Compatibility
    /// sandboxes intentionally keep their historical pathname behavior.
    #[cfg(target_os = "linux")]
    pub(super) bound_allow_read: Vec<BoundSandboxPath>,
    #[cfg(target_os = "linux")]
    pub(super) bound_allow_write: Vec<BoundSandboxPath>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub(super) struct BoundSandboxPath {
    pub(super) path: PathBuf,
    pub(super) fd: Arc<std::os::fd::OwnedFd>,
    device: nix::libc::dev_t,
    inode: nix::libc::ino_t,
    is_directory: bool,
    validate_pathname: bool,
}

#[cfg(target_os = "linux")]
impl BoundSandboxPath {
    fn from_fd(path: &std::path::Path, fd: std::os::fd::OwnedFd) -> eyre::Result<Self> {
        use std::os::fd::AsRawFd;

        let retained_stat = nix::sys::stat::fstat(&fd)?;
        let kind = nix::sys::stat::SFlag::from_bits_truncate(retained_stat.st_mode);
        if kind.contains(nix::sys::stat::SFlag::S_IFLNK) {
            eyre::bail!(
                "formula-execution sandbox descriptor is a symlink: {}",
                path.display()
            )
        }
        let authority = nix::fcntl::open(
            format!("/proc/self/fd/{}", fd.as_raw_fd()).as_str(),
            nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )?;
        let authority_stat = nix::sys::stat::fstat(&authority)?;
        let authority_kind = nix::sys::stat::SFlag::from_bits_truncate(authority_stat.st_mode);
        if (authority_stat.st_dev, authority_stat.st_ino, authority_kind)
            != (retained_stat.st_dev, retained_stat.st_ino, kind)
        {
            eyre::bail!(
                "formula-execution sandbox authority changed while binding: {}",
                path.display()
            )
        }
        Ok(Self {
            path: path.to_path_buf(),
            fd: Arc::new(authority),
            device: retained_stat.st_dev,
            inode: retained_stat.st_ino,
            is_directory: kind.contains(nix::sys::stat::SFlag::S_IFDIR),
            validate_pathname: false,
        })
    }

    fn open(path: &std::path::Path) -> eyre::Result<Self> {
        use std::path::Component;

        if !path.is_absolute() {
            eyre::bail!(
                "formula-execution sandbox allow path is not absolute: {}",
                path.display()
            )
        }
        let mut fd = nix::fcntl::open(
            "/",
            nix::fcntl::OFlag::O_PATH
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )?;
        let components = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (index, name) in components.iter().enumerate() {
            let mut flags = nix::fcntl::OFlag::O_PATH
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC;
            if index + 1 != components.len() {
                flags |= nix::fcntl::OFlag::O_DIRECTORY;
            }
            fd = nix::fcntl::openat(&fd, *name, flags, nix::sys::stat::Mode::empty()).map_err(
                |error| {
                    eyre::eyre!(
                        "failed to bind formula-execution sandbox path {}: {error}",
                        path.display()
                    )
                },
            )?;
            let stat = nix::sys::stat::fstat(&fd)?;
            let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
            if kind.contains(nix::sys::stat::SFlag::S_IFLNK) {
                eyre::bail!(
                    "formula-execution sandbox path contains a symlink: {}",
                    path.display()
                )
            }
            if index + 1 != components.len() && !kind.contains(nix::sys::stat::SFlag::S_IFDIR) {
                eyre::bail!(
                    "formula-execution sandbox path ancestor is not a directory: {}",
                    path.display()
                )
            }
        }
        let mut bound = Self::from_fd(path, fd)?;
        bound.validate_pathname = true;
        Ok(bound)
    }

    fn validate_path(&self) -> eyre::Result<()> {
        if !self.validate_pathname {
            return Ok(());
        }
        let current = Self::open(&self.path)?;
        if current.device != self.device
            || current.inode != self.inode
            || current.is_directory != self.is_directory
        {
            eyre::bail!(
                "formula-execution sandbox path changed while it was being bound: {}",
                self.path.display()
            )
        }
        Ok(())
    }
}

/// Minimal env vars inherited when deny_env is active.
const DEFAULT_ENV_KEYS: &[&str] = &["PATH", "HOME", "USER", "SHELL", "TERM", "COLORTERM", "LANG"];

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
    pub fn from_settings_and_cli(
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
    pub fn is_active(&self) -> bool {
        self.deny_read
            || self.deny_write
            || self.deny_net
            || self.deny_local_sockets
            || self.deny_env
            || !self.allow_read.is_empty()
            || !self.allow_write.is_empty()
            || !self.allow_net.is_empty()
            || !self.allow_env.is_empty()
            || self.deny_system_temp_write
            || self.deny_mise_data_read
            || self.system_access_profile == SystemAccessProfile::FormulaExecution
    }

    pub(crate) fn validate_formula_execution_for_runner(
        &self,
        cleanup_process_group: bool,
        bound_current_dir: bool,
    ) -> eyre::Result<()> {
        if self.system_access_profile != SystemAccessProfile::FormulaExecution {
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (cleanup_process_group, bound_current_dir);
            eyre::bail!(
                "formula-execution sandbox is supported only on Linux with fully enforced Landlock ABI V6 confinement"
            );
        }
        #[cfg(target_os = "linux")]
        {
            if !self.deny_read
                || !self.deny_write
                || !self.deny_net
                || !self.deny_local_sockets
                || !self.deny_env
                || !self.deny_system_temp_write
                || !self.deny_mise_data_read
                || !self.require_full_filesystem_confinement
                || !cleanup_process_group
                || !bound_current_dir
            {
                eyre::bail!(
                    "formula-execution sandbox requires strict read, write, network, local-socket, environment, temp, mise-data, filesystem, process-group, and descriptor-bound working-directory confinement"
                );
            }
            Ok(())
        }
    }

    /// Resolve allow_* paths to absolute paths relative to cwd.
    pub fn resolve_paths(&mut self) {
        let cwd = std::env::current_dir().unwrap_or_default();
        let canonicalize = self.system_access_profile != SystemAccessProfile::FormulaExecution;
        let resolve = |paths: &mut Vec<PathBuf>| {
            paths.retain(|p| !p.as_os_str().is_empty());
            for p in paths.iter_mut() {
                *p = replace_path(&*p);
                if p.is_relative() {
                    *p = cwd.join(&*p);
                }
                // Canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
                if canonicalize && let Ok(canonical) = p.canonicalize() {
                    *p = canonical;
                }
            }
        };
        resolve(&mut self.allow_read);
        resolve(&mut self.allow_write);
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn bind_formula_execution_paths(&mut self) -> eyre::Result<()> {
        if self.system_access_profile != SystemAccessProfile::FormulaExecution {
            return Ok(());
        }
        for path in &self.allow_read {
            if !self
                .bound_allow_read
                .iter()
                .any(|bound| bound.path == *path)
            {
                self.bound_allow_read.push(BoundSandboxPath::open(path)?);
            }
        }
        for path in &self.allow_write {
            if !self
                .bound_allow_write
                .iter()
                .any(|bound| bound.path == *path)
            {
                self.bound_allow_write.push(BoundSandboxPath::open(path)?);
            }
        }
        for path in self.bound_allow_read.iter().chain(&self.bound_allow_write) {
            path.validate_path()?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn require_bound_formula_execution_paths(&self) -> eyre::Result<()> {
        if self.system_access_profile == SystemAccessProfile::FormulaExecution
            && (!bindings_cover_paths(&self.allow_read, &self.bound_allow_read)
                || !bindings_cover_paths(&self.allow_write, &self.bound_allow_write))
        {
            eyre::bail!(
                "formula-execution sandbox allow paths must be descriptor-bound before runner setup"
            )
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prebind_formula_execution_read(
        &mut self,
        path: &std::path::Path,
        fd: std::os::fd::OwnedFd,
    ) -> eyre::Result<()> {
        self.prebind_formula_execution_path(path, fd, false)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn prebind_formula_execution_write(
        &mut self,
        path: &std::path::Path,
        fd: std::os::fd::OwnedFd,
    ) -> eyre::Result<()> {
        self.prebind_formula_execution_path(path, fd, true)
    }

    #[cfg(target_os = "linux")]
    fn prebind_formula_execution_path(
        &mut self,
        path: &std::path::Path,
        fd: std::os::fd::OwnedFd,
        write: bool,
    ) -> eyre::Result<()> {
        if self.system_access_profile != SystemAccessProfile::FormulaExecution {
            eyre::bail!("only formula-execution sandboxes accept prebound paths")
        }
        let allowed = if write {
            &self.allow_write
        } else {
            &self.allow_read
        };
        if !allowed.iter().any(|allowed| allowed == path) {
            eyre::bail!(
                "prebound formula-execution path is not declared: {}",
                path.display()
            )
        }
        let bound = BoundSandboxPath::from_fd(path, fd)?;
        let bindings = if write {
            &mut self.bound_allow_write
        } else {
            &mut self.bound_allow_read
        };
        if bindings.iter().any(|existing| existing.path == path) {
            eyre::bail!(
                "formula-execution sandbox path was bound more than once: {}",
                path.display()
            )
        }
        bindings.push(bound);
        Ok(())
    }

    /// Compute effective deny flags, accounting for allow_* implying deny_*.
    #[cfg_attr(windows, allow(dead_code))]
    pub fn effective_deny_read(&self) -> bool {
        self.deny_read || !self.allow_read.is_empty()
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn effective_deny_write(&self) -> bool {
        self.deny_write || !self.allow_write.is_empty()
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn effective_deny_net(&self) -> bool {
        self.deny_net || self.deny_local_sockets || !self.allow_net.is_empty()
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
    pub async fn apply(
        &self,
        program: &str,
        args: &[String],
    ) -> eyre::Result<Option<SandboxedCommand>> {
        if !self.is_active() {
            return Ok(None);
        }
        if self.system_access_profile == SystemAccessProfile::FormulaExecution {
            eyre::bail!(
                "formula-execution sandbox requires CmdLineRunner process-group confinement"
            );
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
        let strict_formula_execution =
            self.system_access_profile == SystemAccessProfile::FormulaExecution;
        if self.effective_deny_net() || strict_formula_execution {
            if !self.allow_net.is_empty() {
                eyre::bail!(
                    "per-host network filtering (--allow-net=<host>) is not supported on Linux. \
                     Use --deny-net to block all network, or remove --allow-net."
                );
            }
            seccomp::apply_seccomp_net_filter(
                self.deny_local_sockets,
                false,
                strict_formula_execution,
            )?;
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

#[cfg(target_os = "linux")]
fn bindings_cover_paths(paths: &[PathBuf], bindings: &[BoundSandboxPath]) -> bool {
    paths
        .iter()
        .all(|path| bindings.iter().any(|binding| binding.path == *path))
        && bindings.iter().all(|binding| paths.contains(&binding.path))
}

/// A command rewritten to run through a sandbox wrapper (macOS sandbox-exec).
#[cfg(not(test))]
#[cfg_attr(windows, allow(dead_code))]
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

/// Fail before spawning when filesystem confinement is unavailable. This
/// keeps the real Landlock diagnostic instead of `pre_exec`'s synthetic EINVAL.
#[cfg(target_os = "linux")]
pub fn ensure_landlock_available() -> eyre::Result<()> {
    landlock::ensure_landlock_available()
}

/// Prove that untrusted formula code can run with the strict Linux sandbox.
/// Callers must invoke this before downloads, staging, receipt changes, or any
/// other host mutation that would be unsafe to leave behind on refusal.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn ensure_strict_formula_execution_available(context: &str) -> eyre::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").map_err(|error| {
            eyre::eyre!(
                "{context}: could not inspect retained Linux capabilities before formula execution: {error}"
            )
        })?;
        validate_linux_formula_execution_security(
            context,
            unsafe { nix::libc::geteuid() },
            &status,
        )?;
        probe_strict_formula_execution().map_err(|error| {
            eyre::eyre!("{context}: strict formula execution is unavailable: {error}")
        })?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    eyre::bail!(
        "{context}: strict formula execution is supported only on Linux with fully enforced Landlock ABI V6 confinement"
    )
}

#[cfg(target_os = "linux")]
fn probe_strict_formula_execution() -> eyre::Result<()> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let config = SandboxConfig {
        deny_read: true,
        deny_write: true,
        deny_net: true,
        deny_local_sockets: true,
        deny_env: true,
        deny_system_temp_write: true,
        deny_mise_data_read: true,
        require_full_filesystem_confinement: true,
        system_access_profile: SystemAccessProfile::FormulaExecution,
        ..Default::default()
    };
    let (stage_reader, stage_writer) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)
        .map_err(|error| eyre::eyre!("could not create strict sandbox probe pipe: {error}"))?;
    let stage_writer_fd = stage_writer.as_raw_fd();
    let report_stage = move |stage: &'static [u8]| unsafe {
        nix::libc::write(
            stage_writer_fd,
            stage.as_ptr().cast(),
            stage.len() as nix::libc::size_t,
        );
    };

    let mut command = Command::new("/bin/true");
    command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            if nix::libc::setpgid(0, 0) != 0 {
                report_stage(b"process-group");
                return Err(std::io::Error::last_os_error());
            }
            if let Err(error) = landlock::apply_landlock(&config) {
                report_stage(b"landlock");
                return Err(std::io::Error::other(error.to_string()));
            }
            if let Err(error) = seccomp::apply_seccomp_net_filter(true, true, true) {
                report_stage(b"seccomp");
                return Err(std::io::Error::other(error.to_string()));
            }
            Ok(())
        });
    }

    let child = command.spawn();
    drop(stage_writer);
    let mut stage = String::new();
    std::fs::File::from(stage_reader)
        .read_to_string(&mut stage)
        .map_err(|error| eyre::eyre!("could not read strict sandbox probe result: {error}"))?;
    let mut child = child.map_err(|error| strict_formula_probe_error(&stage, error))?;
    let status = child
        .wait()
        .map_err(|error| eyre::eyre!("strict sandbox probe could not wait for child: {error}"))?;
    if !status.success() {
        eyre::bail!("strict sandbox probe child exited unsuccessfully: {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn strict_formula_probe_error(stage: &str, error: impl std::fmt::Display) -> eyre::Report {
    let stage = if stage.is_empty() {
        "spawn/exec"
    } else {
        stage
    };
    eyre::eyre!("strict sandbox probe failed during {stage}: {error}")
}

#[cfg(any(target_os = "linux", test))]
fn validate_linux_formula_execution_security(
    context: &str,
    effective_uid: u32,
    proc_status: &str,
) -> eyre::Result<()> {
    if effective_uid == 0 {
        eyre::bail!(
            "{context}: strict formula execution refuses effective UID 0 because formula code must not retain host capabilities; run mise as an unprivileged user"
        );
    }
    for field in ["CapInh", "CapPrm", "CapEff", "CapAmb"] {
        let prefix = format!("{field}:");
        let values = proc_status
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix))
            .map(str::trim)
            .collect::<Vec<_>>();
        let [value] = values.as_slice() else {
            eyre::bail!(
                "{context}: cannot prove retained Linux capabilities are absent: expected exactly one {field} entry in /proc/self/status"
            );
        };
        let mask = u64::from_str_radix(value, 16).map_err(|_| {
            eyre::eyre!(
                "{context}: cannot prove retained Linux capabilities are absent: malformed {field} value in /proc/self/status"
            )
        })?;
        if mask != 0 {
            eyre::bail!(
                "{context}: strict formula execution refuses retained Linux capabilities ({field}={value}); run mise without inherited, permitted, effective, or ambient capabilities"
            );
        }
    }
    Ok(())
}

/// Apply seccomp network filter (Linux only).
#[cfg(target_os = "linux")]
pub fn seccomp_apply(
    deny_local_sockets: bool,
    deny_process_group_escape: bool,
    strict_formula_execution: bool,
) -> eyre::Result<()> {
    seccomp::apply_seccomp_net_filter(
        deny_local_sockets,
        deny_process_group_escape,
        strict_formula_execution,
    )
}

/// Generate a macOS Seatbelt profile string (macOS only).
#[cfg(target_os = "macos")]
pub async fn macos_generate_profile(config: &SandboxConfig) -> String {
    macos::generate_seatbelt_profile(config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::SettingsSandbox;
    use std::collections::BTreeMap;

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

    #[test]
    fn test_local_socket_denial_enables_strict_network_sandbox() {
        let config = SandboxConfig {
            deny_local_sockets: true,
            ..Default::default()
        };

        assert!(config.is_active());
        assert!(config.effective_deny_net());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn formula_execution_profile_requires_every_strict_boundary() {
        let strict = SandboxConfig {
            deny_read: true,
            deny_write: true,
            deny_net: true,
            deny_local_sockets: true,
            deny_env: true,
            deny_system_temp_write: true,
            deny_mise_data_read: true,
            require_full_filesystem_confinement: true,
            system_access_profile: SystemAccessProfile::FormulaExecution,
            ..Default::default()
        };

        assert!(strict.is_active());
        strict
            .validate_formula_execution_for_runner(true, true)
            .unwrap();
        assert!(
            strict
                .validate_formula_execution_for_runner(false, true)
                .unwrap_err()
                .to_string()
                .contains("process-group")
        );
        assert!(
            strict
                .validate_formula_execution_for_runner(true, false)
                .unwrap_err()
                .to_string()
                .contains("descriptor-bound working-directory confinement")
        );

        let mut incomplete = strict;
        incomplete.deny_local_sockets = false;
        assert!(
            incomplete
                .validate_formula_execution_for_runner(true, true)
                .unwrap_err()
                .to_string()
                .contains("formula-execution sandbox requires strict")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn formula_execution_binding_rejects_missing_and_replaced_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let readable = tmp.path().join("formula.rb");
        std::fs::write(&readable, "original").unwrap();
        let mut strict = SandboxConfig {
            system_access_profile: SystemAccessProfile::FormulaExecution,
            allow_read: vec![readable.clone()],
            ..Default::default()
        };
        strict.resolve_paths();
        strict.bind_formula_execution_paths().unwrap();
        std::fs::rename(&readable, tmp.path().join("original.rb")).unwrap();
        std::fs::write(&readable, "foreign").unwrap();
        assert!(strict.bound_allow_read[0].validate_path().is_err());

        let mut missing = SandboxConfig {
            system_access_profile: SystemAccessProfile::FormulaExecution,
            allow_read: vec![tmp.path().join("missing")],
            ..Default::default()
        };
        missing.resolve_paths();
        assert!(missing.bind_formula_execution_paths().is_err());

        let mut compatibility = SandboxConfig {
            system_access_profile: SystemAccessProfile::Compatibility,
            allow_read: vec![tmp.path().join("missing")],
            ..Default::default()
        };
        compatibility.resolve_paths();
        compatibility.bind_formula_execution_paths().unwrap();
    }

    #[test]
    fn default_system_access_profile_preserves_compatibility() {
        assert_eq!(
            SandboxConfig::default().system_access_profile,
            SystemAccessProfile::Compatibility
        );
    }

    #[test]
    fn formula_execution_preflight_rejects_root_and_retained_capabilities() {
        const ZERO_CAPABILITIES: &str = "\
CapInh:\t0000000000000000\n\
CapPrm:\t0000000000000000\n\
CapEff:\t0000000000000000\n\
CapBnd:\t000001ffffffffff\n\
CapAmb:\t0000000000000000\n";

        let error = validate_linux_formula_execution_security("test", 0, "").unwrap_err();
        assert!(error.to_string().contains("refuses effective UID 0"));
        validate_linux_formula_execution_security("test", 1000, ZERO_CAPABILITIES).unwrap();

        for field in ["CapInh", "CapPrm", "CapEff", "CapAmb"] {
            let status = ZERO_CAPABILITIES.replacen(
                &format!("{field}:\t0000000000000000"),
                &format!("{field}:\t0000000000000001"),
                1,
            );
            let error =
                validate_linux_formula_execution_security("test", 1000, &status).unwrap_err();
            assert!(error.to_string().contains(field));
            assert!(error.to_string().contains("retained Linux capabilities"));
        }

        let missing = ZERO_CAPABILITIES.replace("CapAmb:\t0000000000000000\n", "");
        assert!(
            validate_linux_formula_execution_security("test", 1000, &missing)
                .unwrap_err()
                .to_string()
                .contains("expected exactly one CapAmb")
        );
        let malformed = ZERO_CAPABILITIES.replace("CapEff:\t0000000000000000", "CapEff:\tnot-hex");
        assert!(
            validate_linux_formula_execution_security("test", 1000, &malformed)
                .unwrap_err()
                .to_string()
                .contains("malformed CapEff")
        );
        let duplicate = format!("{ZERO_CAPABILITIES}CapPrm:\t0000000000000000\n");
        assert!(
            validate_linux_formula_execution_security("test", 1000, &duplicate)
                .unwrap_err()
                .to_string()
                .contains("expected exactly one CapPrm")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn formula_execution_probe_reports_the_failing_enforcement_stage() {
        let error = strict_formula_probe_error("seccomp", "operation not supported");
        assert!(
            error
                .to_string()
                .contains("strict sandbox probe failed during seccomp")
        );
        let error = strict_formula_probe_error("", "invalid argument");
        assert!(error.to_string().contains("during spawn/exec"));
    }
}
