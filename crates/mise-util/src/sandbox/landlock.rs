use eyre::{Result, eyre};
use landlock::{
    ABI, AccessFs, AccessNet, BitFlags, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, Scope,
};

use super::SandboxConfig;

/// System paths that are always readable on Linux.
/// Note: /tmp and /dev are handled separately with full (read+write) access.
const SYSTEM_READ_PATHS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib64",
    "/bin",
    "/sbin",
    "/etc",
    "/proc",
    "/sys",
    "/nix",
    "/snap",
    "/home/linuxbrew",
];

/// System files that may resolve outside [`SYSTEM_READ_PATHS`].
///
/// Landlock rules apply to the resolved file hierarchy. On many Linux systems,
/// /etc/resolv.conf points into /run, which must not be made broadly readable
/// because it can contain runtime secrets.
const SYSTEM_READ_FILES: &[&str] = &["/etc/resolv.conf"];

fn add_read_rule(
    ruleset: landlock::RulesetCreated,
    path: &str,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    match PathFd::new(path) {
        Ok(fd) => ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| eyre!("landlock add_rule failed for {path}: {e}")),
        Err(_) => Ok(ruleset), // Path doesn't exist, skip
    }
}

fn add_path_rule(
    ruleset: landlock::RulesetCreated,
    path: &std::path::Path,
    access: BitFlags<AccessFs>,
) -> Result<landlock::RulesetCreated> {
    match PathFd::new(path) {
        Ok(fd) => ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| eyre!("landlock add_rule failed for {}: {e}", path.display())),
        Err(_) => {
            // Landlock requires existing paths, so a rule naming one that is not
            // there yet — --allow-write=./dist before dist is created — has to be
            // dropped. Report it from the parent, not here:
            // SandboxConfig::warn_missing_allow_paths names every path confirmed
            // missing once, through the logger, with a directory that can
            // actually be allowed.
            //
            // Deliberately silent even for the cases the parent cannot predict —
            // a path that was there when it looked but cannot be opened now.
            // Probing openability in the parent is not the answer either: an
            // ordinary open blocks on a FIFO until a writer appears, and the
            // O_PATH open that avoids that only repeats what PathFd does here a
            // moment later, while still missing the window between the two.
            // This runs through pre_exec, after fork, where only async-signal-safe
            // work is sound; `eprintln!` takes the stderr lock, which another
            // thread may have held at fork time, and would hang the child before
            // exec. A missed diagnostic is the cheaper failure.
            Ok(ruleset)
        }
    }
}

/// Let `allow_list_dirs` be listed: the names in those directories, and
/// nothing of the files' contents.
fn add_list_rules(
    mut ruleset: landlock::RulesetCreated,
    config: &SandboxConfig,
) -> Result<landlock::RulesetCreated> {
    for path in &config.allow_list_dirs {
        ruleset = add_path_rule(ruleset, path, AccessFs::ReadDir.into())?;
    }
    Ok(ruleset)
}

/// The access rights a configuration restricts.
///
/// Only handle the access types we're actually restricting.
/// If we handle_access(full_access) but only add read rules,
/// writes to un-ruled paths get blocked too (Landlock denies by default).
fn handled_access(config: &SandboxConfig) -> BitFlags<AccessFs> {
    let abi = ABI::V5;
    let mut read_access = AccessFs::from_read(abi);
    if config.deny_process {
        read_access.remove(AccessFs::Execute);
    }
    let full_access = read_access | AccessFs::from_write(abi);
    let mut handled_access = match (config.effective_deny_read(), config.effective_deny_write()) {
        (true, true) => full_access,
        (true, false) => read_access,
        (false, true) => full_access, // need full to add read+write rules for allowed paths
        (false, false) => BitFlags::empty(),
    };
    if config.deny_process {
        handled_access |= AccessFs::Execute;
    }
    // Pathname sockets are refused unless an `allow_unix` rule covers them.
    // Abstract sockets have no path; `create_ruleset` scopes them separately.
    if config.effective_deny_unix() {
        handled_access |= AccessFs::ResolveUnix;
    }
    handled_access
}

/// Rights an older kernel may lack without weakening the sandbox. Before
/// Refer existed (Linux 5.19), a Landlock domain refused every rename or link
/// across directories; IoctlDev (Linux 6.10) covers ioctl on device files,
/// which live under /dev and stay writable under every restriction anyway.
fn optional_access() -> BitFlags<AccessFs> {
    AccessFs::Refer | AccessFs::IoctlDev
}

/// The rights the kernel must support for `handled_access` to mean what it
/// says; the sandbox is refused rather than applied without them.
fn required_access(handled_access: BitFlags<AccessFs>) -> BitFlags<AccessFs> {
    handled_access & !optional_access()
}

/// Create a ruleset handling `handled_access`, or fail if the running kernel
/// cannot enforce it.
///
/// A right the ruleset does not handle is not restricted at all, so asking
/// for every right best-effort silently weakens the sandbox on an older
/// kernel: without Truncate (Linux 6.2) a command under `--deny-write` can
/// still empty any file with truncate(2). Only [`optional_access`] stays best
/// effort, and so does TCP under `deny_tcp`: the kernel handles it from Linux
/// 6.7, and seccomp refuses network sockets on every kernel.
fn create_ruleset(
    handled_access: BitFlags<AccessFs>,
    scope_abstract_sockets: bool,
    deny_tcp: bool,
) -> Result<landlock::RulesetCreated> {
    let required = required_access(handled_access);
    let mut ruleset = Ruleset::default()
        .set_compatibility(landlock::CompatLevel::HardRequirement)
        .handle_access(required)
        .map_err(|e| unsupported_error(required, e))?;
    if scope_abstract_sockets {
        ruleset = ruleset
            .scope(Scope::AbstractUnixSocket)
            .map_err(|e| unsupported_error(required, e))?;
    }
    let mut ruleset = ruleset
        .set_compatibility(landlock::CompatLevel::BestEffort)
        .handle_access(handled_access)
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?;
    if deny_tcp {
        ruleset = ruleset
            .handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
            .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))?;
    }
    ruleset
        .create()
        .map_err(|e| eyre!("failed to create landlock ruleset: {e}"))
}

/// Explain why the kernel cannot enforce `required`, naming the right that a
/// still-common kernel lacks when that is the only one missing.
fn unsupported_error(required: BitFlags<AccessFs>, err: impl std::fmt::Display) -> eyre::Report {
    let only_truncate_missing = required.contains(AccessFs::Truncate)
        && Ruleset::default()
            .set_compatibility(landlock::CompatLevel::HardRequirement)
            .handle_access(required & !AccessFs::Truncate)
            .is_ok();
    if required.contains(AccessFs::ResolveUnix) {
        return eyre!(
            "sandbox: Landlock on this kernel cannot restrict Unix sockets, \
             so mise will not run the command with --deny-unix or --allow-unix ({err}). \
             Restricting Unix sockets needs Linux 7.1 or later."
        );
    }
    if only_truncate_missing {
        eyre!(
            "sandbox: Landlock on this kernel cannot stop a command from truncating files, \
             so mise will not run it with a write restriction ({err}). \
             Restricting writes needs Linux 6.2 or later."
        )
    } else {
        eyre!("sandbox: Landlock on this kernel cannot enforce the requested restrictions ({err})")
    }
}

/// Check that the running kernel can enforce `config` before the child is
/// forked.
///
/// `apply_landlock` refuses the same kernels, but under `mise run` it runs in
/// `pre_exec`, after fork, where an error reaches the parent as a bare errno
/// and the task would fail with no word about the kernel.
pub(super) fn check_kernel_support(config: &SandboxConfig) -> Result<()> {
    let handled_access = handled_access(config);
    if !handled_access.is_empty() {
        create_ruleset(
            handled_access,
            config.effective_deny_unix(),
            config.effective_deny_net(),
        )?;
    }
    Ok(())
}

/// Apply Landlock filesystem and executable restrictions.
pub(super) fn apply_landlock(
    config: &SandboxConfig,
    initial_program: Option<&std::path::Path>,
) -> Result<()> {
    let abi = ABI::V5;

    let deny_read = config.effective_deny_read();
    let deny_write = config.effective_deny_write();
    let execute_access: BitFlags<AccessFs> = AccessFs::Execute.into();
    let mut read_access = AccessFs::from_read(abi);
    if config.deny_process {
        read_access.remove(execute_access);
    }
    let write_access = AccessFs::from_write(abi);
    let full_access = read_access | write_access;

    let handled_access = handled_access(config);
    if handled_access.is_empty() {
        if config.effective_deny_net() {
            return apply_network_domain();
        }
        return Ok(());
    }

    let mut ruleset = create_ruleset(
        handled_access,
        config.effective_deny_unix(),
        config.effective_deny_net(),
    )?;

    if deny_read && deny_write {
        // Both restricted: add read rules for system paths, full for /tmp and /dev
        for path in SYSTEM_READ_PATHS {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        for path in SYSTEM_READ_FILES {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        if !config.deny_temp_read {
            ruleset = add_read_rule(
                ruleset,
                "/tmp",
                if config.deny_temp_write {
                    read_access
                } else {
                    full_access
                },
            )?;
        }
        ruleset = add_read_rule(ruleset, "/dev", full_access)?;
        for installs_dir in [*crate::dirs::INSTALLS, *crate::dirs::INSTALL_STORE] {
            if installs_dir.exists() {
                ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
            }
        }
        ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        for path in &config.allow_read {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, full_access)?;
        }
        ruleset = add_list_rules(ruleset, config)?;
    } else if deny_read {
        // Only reads restricted — only handle read access so writes are unaffected
        for path in SYSTEM_READ_PATHS {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        for path in SYSTEM_READ_FILES {
            ruleset = add_read_rule(ruleset, path, read_access)?;
        }
        // /tmp and /dev need read access (not in SYSTEM_READ_PATHS, handled separately)
        if !config.deny_temp_read {
            ruleset = add_read_rule(ruleset, "/tmp", read_access)?;
        }
        ruleset = add_read_rule(ruleset, "/dev", read_access)?;
        for installs_dir in [*crate::dirs::INSTALLS, *crate::dirs::INSTALL_STORE] {
            if installs_dir.exists() {
                ruleset = add_path_rule(ruleset, installs_dir, read_access)?;
            }
        }
        ruleset = add_path_rule(ruleset, &crate::env::MISE_DATA_DIR, read_access)?;
        for path in &config.allow_read {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        // allow_write paths are implicitly readable
        for path in &config.allow_write {
            ruleset = add_path_rule(ruleset, path, read_access)?;
        }
        ruleset = add_list_rules(ruleset, config)?;
    } else if deny_write {
        // Only writes restricted — allow read everywhere, deny write except allowed paths
        ruleset = add_read_rule(ruleset, "/", read_access)?;
        ruleset = add_read_rule(
            ruleset,
            "/tmp",
            if config.deny_temp_write {
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

    for path in &config.allow_unix {
        ruleset = add_path_rule(ruleset, path, AccessFs::ResolveUnix.into())?;
    }

    // The ruleset is installed before the target's initial exec. Permit that
    // exact executable while denying later execve/execveat calls for every
    // other file (including Kernel.exec from evaluated tap Ruby).
    if config.deny_process
        && let Some(program) = initial_program
    {
        ruleset = add_path_rule(ruleset, program, execute_access)?;
    }

    let status = ruleset
        .restrict_self()
        .map_err(|e| eyre!("failed to apply landlock restrictions: {e}"))?;
    if status.ruleset == RulesetStatus::NotEnforced || !status.no_new_privs {
        eyre::bail!("failed to apply landlock restrictions: {status:?}");
    }

    Ok(())
}

/// Put a network-only sandbox in a Landlock domain that refuses TCP bind and
/// connect and refuses nothing on the filesystem.
///
/// seccomp keeps the command from creating a network socket, but not from
/// borrowing one: a process outside any domain lets a same-user process
/// ptrace it or open its `/proc/<pid>/mem`, and so create a socket in it and
/// copy the descriptor out. A process in a domain can do neither to a process
/// outside it. TCP is what seccomp already refuses, so the domain adds no
/// restriction a command could notice besides that scope (and `mount`, which
/// every domain refuses).
///
/// Every Landlock layer refuses a rename or hard link into another directory
/// (`Refer`) unless one of its rules grants it, even a layer that handles no
/// filesystem access. The domain grants it below `/`, so a command run inside
/// another Landlock sandbox can still reparent what that sandbox allows.
///
/// Landlock handles network access from Linux 6.7. On an older kernel the
/// domain is not created and the command runs under seccomp alone.
fn apply_network_domain() -> Result<()> {
    let Ok(ruleset) = Ruleset::default()
        .set_compatibility(landlock::CompatLevel::HardRequirement)
        .handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
    else {
        return Ok(());
    };
    let refer: BitFlags<AccessFs> = AccessFs::Refer.into();
    let root = PathFd::new("/").map_err(|e| eyre!("failed to open / for landlock: {e}"))?;
    let status = ruleset
        .handle_access(refer)
        .and_then(|r| r.create())
        .and_then(|r| r.add_rule(PathBeneath::new(root, refer)))
        .and_then(|r| r.restrict_self())
        .map_err(|e| eyre!("failed to apply landlock restrictions: {e}"))?;
    if !status.no_new_privs {
        eyre::bail!("failed to apply landlock restrictions: {status:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use landlock::Access;

    /// A command under a network restriction alone cannot reach into a
    /// process outside its sandbox, which it could otherwise make create a
    /// socket for it.
    #[test]
    fn test_network_only_sandbox_cannot_reach_other_processes() {
        if !Ruleset::default()
            .set_compatibility(landlock::CompatLevel::HardRequirement)
            .handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
            .is_ok_and(|r| r.create().is_ok())
        {
            eprintln!("skipping: Landlock on this kernel does not handle network access");
            return;
        }
        // started outside the sandbox, by this process, so ptrace rules
        // (Yama's included) allow reading its memory before the domain exists
        let mut outside = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mem = format!("/proc/{}/mem", outside.id());
        if std::fs::File::open(&mem).is_err() {
            // Yama's `ptrace_scope` 2 and 3 refuse this even to the parent
            outside.kill().unwrap();
            outside.wait().unwrap();
            eprintln!("skipping: this kernel refuses ptrace access to a child process");
            return;
        }
        let result = std::thread::spawn(move || {
            // the domain binds to the calling thread
            apply_landlock(
                &SandboxConfig {
                    deny_net: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
            std::fs::File::open(&mem).map(drop)
        })
        .join()
        .unwrap();
        outside.kill().unwrap();
        outside.wait().unwrap();
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    /// Under `--deny-net` with a file restriction too, the one Landlock domain
    /// refuses TCP, as the network-only domain does: seccomp refuses creating a
    /// network socket, not connecting one the command was handed.
    #[test]
    fn test_file_and_network_sandbox_refuses_tcp() {
        if !Ruleset::default()
            .set_compatibility(landlock::CompatLevel::HardRequirement)
            .handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
            .is_ok_and(|r| r.create().is_ok())
        {
            eprintln!("skipping: Landlock on this kernel does not handle network access");
            return;
        }
        let config = SandboxConfig {
            deny_write: true,
            deny_net: true,
            ..Default::default()
        };
        if check_kernel_support(&config).is_err() {
            eprintln!("skipping: Landlock on this kernel cannot restrict writes");
            return;
        }
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let result = std::thread::spawn(move || {
            apply_landlock(&config, None).unwrap();
            std::net::TcpStream::connect(addr).map(drop)
        })
        .join()
        .unwrap();
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    /// A command run inside another Landlock sandbox keeps what that sandbox
    /// allows: the network domain does not refuse a rename or hard link into
    /// another directory, which a Landlock layer refuses unless it grants it.
    #[test]
    fn test_network_only_sandbox_keeps_reparenting_an_outer_sandbox_allows() {
        if !Ruleset::default()
            .set_compatibility(landlock::CompatLevel::HardRequirement)
            .handle_access(AccessNet::BindTcp | AccessNet::ConnectTcp)
            .is_ok_and(|r| r.create().is_ok())
        {
            eprintln!("skipping: Landlock on this kernel does not handle network access");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::write(a.join("f"), "").unwrap();
        let result = std::thread::spawn(move || {
            // the outer sandbox: a domain that allows everything, reparenting included
            let all = AccessFs::from_all(ABI::V5);
            Ruleset::default()
                .handle_access(all)
                .unwrap()
                .create()
                .unwrap()
                .add_rule(PathBeneath::new(PathFd::new("/").unwrap(), all))
                .unwrap()
                .restrict_self()
                .unwrap();
            std::fs::rename(a.join("f"), b.join("f")).unwrap();
            apply_landlock(
                &SandboxConfig {
                    deny_net: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
            std::fs::rename(b.join("f"), a.join("f"))
                .and_then(|()| std::fs::hard_link(a.join("f"), b.join("g")))
        })
        .join()
        .unwrap();
        result.unwrap();
    }

    /// Before Linux 6.2 the kernel has no Truncate right, and best effort
    /// dropped it silently: `--deny-write` then left truncate(2) open. A write
    /// restriction must require it; only rights whose absence weakens nothing
    /// may be dropped.
    #[test]
    fn test_write_restriction_requires_truncate() {
        let write = handled_access(&SandboxConfig {
            deny_write: true,
            ..Default::default()
        });
        assert!(required_access(write).contains(AccessFs::Truncate));
        assert_eq!(write & !required_access(write), optional_access());

        // Reads alone need nothing newer than the first Landlock ABI.
        let read = handled_access(&SandboxConfig {
            deny_read: true,
            deny_process: true,
            ..Default::default()
        });
        assert!(AccessFs::from_all(ABI::V1).contains(required_access(read)));
    }

    /// A write restriction either stops truncate(2) or is refused.
    #[test]
    fn test_deny_write_blocks_truncate_or_is_refused() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "data").unwrap();
        let path = std::ffi::CString::new(file.path().as_os_str().as_encoded_bytes()).unwrap();
        let config = SandboxConfig {
            deny_write: true,
            deny_temp_write: true,
            ..Default::default()
        };
        let supported = check_kernel_support(&config).is_ok();
        // The ruleset binds to the calling thread, so a scratch thread keeps
        // it away from the rest of the test run.
        std::thread::spawn(move || {
            let applied = apply_landlock(&config, None);
            assert_eq!(applied.is_ok(), supported, "{applied:?}");
            if applied.is_ok() {
                let ret = unsafe { nix::libc::truncate(path.as_ptr(), 0) };
                assert_eq!(ret, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(nix::libc::EACCES)
                );
            }
        })
        .join()
        .unwrap();
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "data");
    }

    #[test]
    fn test_unix_socket_restriction_needs_resolve_unix() {
        let none = SandboxConfig::default();
        assert!(!handled_access(&none).contains(AccessFs::ResolveUnix));

        for config in [
            SandboxConfig {
                deny_unix: true,
                ..Default::default()
            },
            SandboxConfig {
                allow_unix: vec!["/run/agent.sock".into()],
                ..Default::default()
            },
        ] {
            let handled = handled_access(&config);
            assert!(handled.contains(AccessFs::ResolveUnix));
            // A kernel without it must refuse the sandbox, not drop the right.
            assert!(required_access(handled).contains(AccessFs::ResolveUnix));
        }
    }
}
