use eyre::{Result, eyre};
use landlock::{
    ABI, Access, AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
    RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
};
use std::os::fd::AsFd;

use super::{SandboxConfig, SystemAccessProfile};

const PRODUCTION_ABI: ABI = ABI::V6;

fn production_access() -> BitFlags<AccessFs> {
    AccessFs::from_all(PRODUCTION_ABI)
}

fn production_scopes() -> BitFlags<Scope> {
    Scope::from_all(PRODUCTION_ABI)
}

fn formula_execution_writable_access() -> BitFlags<AccessFs> {
    AccessFs::from_read(PRODUCTION_ABI)
        | AccessFs::WriteFile
        | AccessFs::RemoveDir
        | AccessFs::RemoveFile
        | AccessFs::MakeDir
        | AccessFs::MakeReg
        | AccessFs::MakeFifo
        | AccessFs::MakeSym
        | AccessFs::Refer
        | AccessFs::Truncate
}

/// Verify that the running kernel can create a Landlock ruleset before the
/// command enters `pre_exec`. Errors returned from `pre_exec` lose their
/// structured message in Rust's spawn protocol and otherwise surface as a
/// synthetic `EINVAL`, hiding an unsupported confinement environment.
pub fn ensure_landlock_available() -> Result<()> {
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(production_access())
        .map_err(|e| eyre!("landlock is unavailable: {e}"))?
        .scope(production_scopes())
        .map_err(|e| eyre!("landlock is unavailable: {e}"))?
        .create()
        .map(|_| ())
        .map_err(|e| eyre!("landlock is unavailable: {e}"))
}

/// Historical system policy used by user-configured task and command
/// sandboxes. Formula execution uses a separate minimal profile below.
const COMPATIBILITY_SYSTEM_READ_PATHS: &[&str] = &[
    "/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/proc", "/sys", "/nix", "/snap",
];

/// System files that may resolve outside [`COMPATIBILITY_SYSTEM_READ_PATHS`].
///
/// Landlock rules apply to the resolved file hierarchy. On many Linux systems,
/// /etc/resolv.conf points into /run, which must not be made broadly readable
/// because it can contain runtime secrets.
const COMPATIBILITY_SYSTEM_READ_FILES: &[&str] = &["/etc/resolv.conf"];

/// Toolchain and runtime roots needed to execute formula code. Mutable
/// runtime trees and package-manager roots are intentionally absent.
const FORMULA_EXECUTION_SYSTEM_READ_PATHS: &[&str] = &[
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/lib64",
    "/usr/libexec",
    "/usr/include",
    "/usr/share",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
];

/// Exact host configuration leaves needed by the dynamic loader and basic
/// libc identity/locale lookups. Formula execution does not receive recursive /etc.
const FORMULA_EXECUTION_SYSTEM_READ_FILES: &[&str] = &[
    "/etc/ld.so.cache",
    "/etc/localtime",
    "/etc/locale.alias",
    "/etc/nsswitch.conf",
    "/etc/passwd",
    "/etc/group",
];

fn access_for_fd<F: AsFd>(fd: &F, access: BitFlags<AccessFs>) -> Result<BitFlags<AccessFs>> {
    let file = std::fs::File::from(
        fd.as_fd()
            .try_clone_to_owned()
            .map_err(|e| eyre!("failed to inspect landlock path descriptor: {e}"))?,
    );
    let metadata = file
        .metadata()
        .map_err(|e| eyre!("failed to inspect landlock path descriptor: {e}"))?;
    Ok(if metadata.is_dir() {
        access
    } else {
        access & AccessFs::from_file(PRODUCTION_ABI)
    })
}

fn add_bound_rule(
    ruleset: landlock::RulesetCreated,
    path: &super::BoundSandboxPath,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    let access = access_for_fd(path.fd.as_ref(), access)?;
    ruleset
        .add_rule(PathBeneath::new(path.fd.as_ref(), access))
        .map_err(|error| {
            eyre!(
                "landlock add_rule failed for bound {}: {error}",
                path.path.display()
            )
        })
}

fn add_rule(
    ruleset: landlock::RulesetCreated,
    fd: PathFd,
    path: &std::path::Path,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    let access = access_for_fd(&fd, access)?;
    ruleset
        .add_rule(PathBeneath::new(fd, access))
        .map_err(|e| eyre!("landlock add_rule failed for {}: {e}", path.display()))
}

fn add_read_rule(
    ruleset: landlock::RulesetCreated,
    path: &str,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    match PathFd::new(path) {
        Ok(fd) => add_rule(ruleset, fd, std::path::Path::new(path), access),
        Err(_) => Ok(ruleset), // Path doesn't exist, skip
    }
}

fn add_path_rule(
    ruleset: landlock::RulesetCreated,
    path: &std::path::Path,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    match PathFd::new(path) {
        Ok(fd) => add_rule(ruleset, fd, path, access),
        Err(_) => {
            // Path doesn't exist — on Linux, Landlock requires existing paths.
            // This affects cases like --allow-write=./dist where the dir doesn't exist yet.
            // We warn rather than silently skipping or granting broader ancestor access.
            eprintln!(
                "mise sandbox: path '{}' does not exist, sandbox rule may not apply as expected",
                path.display()
            );
            Ok(ruleset)
        }
    }
}

fn system_read_paths(profile: SystemAccessProfile) -> &'static [&'static str] {
    match profile {
        SystemAccessProfile::Compatibility => COMPATIBILITY_SYSTEM_READ_PATHS,
        SystemAccessProfile::FormulaExecution => FORMULA_EXECUTION_SYSTEM_READ_PATHS,
    }
}

fn system_read_files(profile: SystemAccessProfile) -> &'static [&'static str] {
    match profile {
        SystemAccessProfile::Compatibility => COMPATIBILITY_SYSTEM_READ_FILES,
        SystemAccessProfile::FormulaExecution => FORMULA_EXECUTION_SYSTEM_READ_FILES,
    }
}

fn add_system_read_rules(
    mut ruleset: landlock::RulesetCreated,
    profile: SystemAccessProfile,
    read_access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    for path in system_read_paths(profile) {
        ruleset = add_read_rule(ruleset, path, read_access)?;
    }
    for path in system_read_files(profile) {
        ruleset = add_read_rule(ruleset, path, read_access)?;
    }
    Ok(ruleset)
}

fn add_formula_execution_device_rules(
    mut ruleset: landlock::RulesetCreated,
) -> Result<landlock::RulesetCreated> {
    let read_file: BitFlags<AccessFs> = AccessFs::ReadFile.into();
    let null_access = read_file | AccessFs::WriteFile | AccessFs::Truncate;
    ruleset = add_read_rule(ruleset, "/dev/null", null_access)?;
    for path in ["/dev/zero", "/dev/random", "/dev/urandom"] {
        ruleset = add_read_rule(ruleset, path, read_file)?;
    }
    Ok(ruleset)
}

/// Apply Landlock filesystem restrictions.
pub fn apply_landlock(config: &SandboxConfig) -> Result<()> {
    let abi = PRODUCTION_ABI;

    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_write(abi);
    let full_access = read_access | write_access;
    let writable_access = if config.system_access_profile == SystemAccessProfile::FormulaExecution {
        formula_execution_writable_access()
    } else {
        full_access
    };

    let deny_read = config.effective_deny_read();
    let deny_write = config.effective_deny_write();

    if config.system_access_profile == SystemAccessProfile::FormulaExecution
        && (!deny_read
            || !deny_write
            || !config.deny_system_temp_write
            || !config.deny_mise_data_read
            || !config.require_full_filesystem_confinement)
    {
        eyre::bail!(
            "formula-execution system access requires strict read, write, temp, mise-data, and filesystem enforcement"
        );
    }
    if config.system_access_profile == SystemAccessProfile::FormulaExecution {
        config.require_bound_formula_execution_paths()?;
    }

    // Only handle the access types we're actually restricting.
    // If we handle_access(full_access) but only add read rules,
    // writes to un-ruled paths get blocked too (Landlock denies by default).
    let handled_access = match (deny_read, deny_write) {
        (true, true) => full_access,
        (true, false) => read_access,
        (false, true) => full_access, // need full to add read+write rules for allowed paths
        (false, false) => return Ok(()), // nothing to restrict
    };

    let compatibility = if config.require_full_filesystem_confinement {
        CompatLevel::HardRequirement
    } else {
        CompatLevel::BestEffort
    };
    let ruleset = Ruleset::default()
        .set_compatibility(compatibility)
        .handle_access(handled_access)
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?;
    let ruleset = if config.system_access_profile == SystemAccessProfile::FormulaExecution {
        ruleset
            .scope(production_scopes())
            .map_err(|e| eyre!("failed to scope formula-execution landlock ruleset: {e}"))?
    } else {
        ruleset
    };
    let mut ruleset = ruleset
        .create()
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?;

    if deny_read && deny_write {
        ruleset = add_system_read_rules(ruleset, config.system_access_profile, read_access)?;
        if config.system_access_profile == SystemAccessProfile::FormulaExecution {
            ruleset = add_formula_execution_device_rules(ruleset)?;
        } else {
            ruleset = add_read_rule(
                ruleset,
                "/tmp",
                if config.deny_system_temp_write {
                    read_access
                } else {
                    full_access
                },
            )?;
            ruleset = add_read_rule(ruleset, "/dev", full_access)?;
        }
        if !config.deny_mise_data_read {
            let installs_dir: &std::path::Path = &crate::dirs::INSTALLS;
            if installs_dir.exists() {
                ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
            }
            ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        }
        if config.system_access_profile == SystemAccessProfile::FormulaExecution {
            for path in &config.bound_allow_read {
                ruleset = add_bound_rule(ruleset, path, read_access)?;
            }
            for path in &config.bound_allow_write {
                ruleset = add_bound_rule(ruleset, path, writable_access)?;
            }
        } else {
            for path in &config.allow_read {
                ruleset = add_path_rule(ruleset, path, read_access)?;
            }
            for path in &config.allow_write {
                ruleset = add_path_rule(ruleset, path, writable_access)?;
            }
        }
    } else if deny_read {
        // Only reads restricted — only handle read access so writes are unaffected
        ruleset = add_system_read_rules(ruleset, config.system_access_profile, read_access)?;
        // /tmp and /dev need read access (not in SYSTEM_READ_PATHS, handled separately)
        ruleset = add_read_rule(ruleset, "/tmp", read_access)?;
        ruleset = add_read_rule(ruleset, "/dev", read_access)?;
        if !config.deny_mise_data_read {
            let installs_dir: &std::path::Path = &crate::dirs::INSTALLS;
            if installs_dir.exists() {
                ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
            }
            ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        }
        for path in &config.allow_read {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        // allow_write paths are implicitly readable
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
    } else if deny_write {
        // Only writes restricted — allow read everywhere, deny write except allowed paths
        ruleset = add_read_rule(ruleset, "/", read_access)?;
        ruleset = add_read_rule(
            ruleset,
            "/tmp",
            if config.deny_system_temp_write {
                read_access
            } else {
                full_access
            },
        )?;
        ruleset = add_read_rule(ruleset, "/dev", full_access)?;
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, full_access)?;
        }
    }

    let status = ruleset
        .restrict_self()
        .map_err(|e| eyre!("failed to apply landlock restrictions: {e}"))?;
    let unacceptable_ruleset = if config.require_full_filesystem_confinement {
        status.ruleset != RulesetStatus::FullyEnforced
    } else {
        status.ruleset == RulesetStatus::NotEnforced
    };
    if unacceptable_ruleset || !status.no_new_privs {
        eyre::bail!("failed to apply landlock restrictions: {status:?}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::libc;

    const FORMULA_PROFILE_CHILD: &str = "MISE_TEST_FORMULA_LANDLOCK_CHILD";
    const FORMULA_PROFILE_ROOT: &str = "MISE_TEST_FORMULA_LANDLOCK_ROOT";
    const FORMULA_PROFILE_SECRET: &str = "formula-parent-proc-secret-73d4f2";

    #[test]
    fn production_policy_requires_every_v6_filesystem_right_and_scope() {
        assert_eq!(production_access(), AccessFs::from_all(ABI::V6));
        assert_eq!(production_scopes(), Scope::from_all(ABI::V6));
        assert!(production_scopes().contains(Scope::Signal));
        assert!(production_scopes().contains(Scope::AbstractUnixSocket));
        assert!(production_access().contains(AccessFs::Refer));
        assert!(production_access().contains(AccessFs::Truncate));
        assert!(production_access().contains(AccessFs::IoctlDev));
    }

    #[test]
    fn formula_execution_writable_policy_excludes_device_and_socket_authority() {
        let access = formula_execution_writable_access();
        assert!(!access.contains(AccessFs::MakeChar));
        assert!(!access.contains(AccessFs::MakeBlock));
        assert!(!access.contains(AccessFs::MakeSock));
        assert!(!access.contains(AccessFs::IoctlDev));
        assert!(access.contains(AccessFs::MakeReg));
        assert!(access.contains(AccessFs::Truncate));
    }

    #[test]
    fn file_rules_drop_only_directory_inapplicable_rights() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let fd = PathFd::new(file.path()).unwrap();
        let access = access_for_fd(&fd, production_access()).unwrap();

        assert_eq!(access, AccessFs::from_file(PRODUCTION_ABI));
    }

    #[test]
    fn directory_rules_keep_every_requested_right() {
        let directory = tempfile::tempdir().unwrap();
        let fd = PathFd::new(directory.path()).unwrap();
        let access = access_for_fd(&fd, production_access()).unwrap();

        assert_eq!(access, production_access());
    }

    #[test]
    fn formula_execution_profile_has_no_recursive_runtime_or_package_manager_roots() {
        for path in [
            "/usr",
            "/usr/local",
            "/etc",
            "/proc",
            "/sys",
            "/tmp",
            "/dev",
            "/nix",
            "/snap",
        ] {
            assert!(!FORMULA_EXECUTION_SYSTEM_READ_PATHS.contains(&path));
        }
        assert!(COMPATIBILITY_SYSTEM_READ_PATHS.contains(&"/proc"));
    }

    #[test]
    fn formula_execution_profile_denies_host_secrets_and_device_ioctl() {
        let test_name = "sandbox::landlock::tests::formula_execution_profile_denies_host_secrets_and_device_ioctl";
        if std::env::var_os(FORMULA_PROFILE_CHILD).is_none() {
            if ensure_landlock_available().is_err() {
                return;
            }
            let root = tempfile::tempdir().unwrap();
            let allowed = root.path().join("allowed");
            let sibling = root.path().join("sibling-secret");
            std::fs::create_dir(&allowed).unwrap();
            std::fs::write(&sibling, b"must stay private").unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name])
                .env(FORMULA_PROFILE_CHILD, FORMULA_PROFILE_SECRET)
                .env(FORMULA_PROFILE_ROOT, root.path())
                .env(crate::test::INHERIT_TEST_PROCESS_ENV, "1")
                .status()
                .unwrap();
            assert!(status.success(), "formula Landlock child failed");
            return;
        }

        let root = std::path::PathBuf::from(std::env::var_os(FORMULA_PROFILE_ROOT).unwrap());
        let allowed = root.join("allowed");
        let sibling = root.join("sibling-secret");
        let initial_environ = std::fs::read("/proc/self/environ").unwrap();
        assert!(
            initial_environ
                .windows(FORMULA_PROFILE_SECRET.len())
                .any(|window| window == FORMULA_PROFILE_SECRET.as_bytes()),
            "re-executed parent secret is absent from procfs"
        );

        let mut config = SandboxConfig {
            deny_read: true,
            deny_write: true,
            allow_read: vec![allowed.clone()],
            allow_write: vec![allowed.clone()],
            deny_system_temp_write: true,
            deny_mise_data_read: true,
            require_full_filesystem_confinement: true,
            system_access_profile: SystemAccessProfile::FormulaExecution,
            ..Default::default()
        };
        config.resolve_paths();
        config.bind_formula_execution_paths().unwrap();
        apply_landlock(&config).unwrap();

        let parent_pid = unsafe { libc::getppid() };
        assert_eq!(unsafe { libc::kill(parent_pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::PermissionDenied,
            "strict formula execution signaled a process outside its Landlock domain"
        );
        let mut same_domain = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let same_domain_pid = same_domain.id() as libc::pid_t;
        assert_eq!(unsafe { libc::kill(same_domain_pid, 0) }, 0);
        assert_eq!(unsafe { libc::kill(same_domain_pid, libc::SIGTERM) }, 0);
        same_domain.wait().unwrap();

        std::fs::write(allowed.join("output"), b"allowed").unwrap();
        assert_eq!(std::fs::read(allowed.join("output")).unwrap(), b"allowed");
        if std::path::Path::new("/usr/bin/cc").is_file() {
            let source = allowed.join("conftest.c");
            let executable = allowed.join("conftest");
            std::fs::write(&source, b"int main(void) { return 0; }\n").unwrap();
            let output = std::process::Command::new("/usr/bin/cc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .env("TMPDIR", &allowed)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "confined compiler helper failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                std::process::Command::new(executable)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        for denied in [
            std::path::Path::new("/etc/hosts"),
            std::path::Path::new("/etc/shadow"),
            std::path::Path::new("/proc/self/environ"),
            sibling.as_path(),
        ] {
            assert_eq!(
                std::fs::read(denied).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied,
                "strict formula execution unexpectedly read {}",
                denied.display()
            );
        }
        if std::path::Path::new("/usr/local/bin").is_dir() {
            assert_eq!(
                std::fs::read_dir("/usr/local/bin").unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied,
                "strict formula execution enumerated unrelated local tools"
            );
        }

        let parent_proc = std::process::Command::new("/bin/sh")
            .args(["-c", "cat /proc/$PPID/environ >/dev/null 2>&1"])
            .status()
            .unwrap();
        assert!(
            !parent_proc.success(),
            "strict formula execution read its parent's procfs environment"
        );
        assert_eq!(
            std::fs::File::open("/dev/tty").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
            "strict formula execution opened /dev/tty"
        );

        use std::os::fd::AsRawFd;
        let null = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
            .unwrap();
        let mut available = 0;
        let ioctl = unsafe { libc::ioctl(null.as_raw_fd(), libc::FIONREAD, &mut available) };
        assert_eq!(ioctl, -1);
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::PermissionDenied,
            "strict formula execution retained device ioctl authority"
        );

        if unsafe { libc::geteuid() } == 0 {
            use std::ffi::CString;
            let device = allowed.join("host-device");
            let device = CString::new(device.as_os_str().as_encoded_bytes()).unwrap();
            let created =
                unsafe { libc::mknod(device.as_ptr(), libc::S_IFCHR | 0o600, libc::makedev(1, 3)) };
            assert_eq!(created, -1);
            assert_eq!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::PermissionDenied,
                "strict formula execution created a host device"
            );
        }
        use std::io::Read;
        let mut random = std::fs::File::open("/dev/urandom").unwrap();
        let mut byte = [0];
        random.read_exact(&mut byte).unwrap();
    }
}
