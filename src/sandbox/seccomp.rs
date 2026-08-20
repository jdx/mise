use eyre::{Result, eyre};
use nix::libc;
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};
use std::collections::BTreeMap;

fn syscall_number<T: Into<i64>>(number: T) -> i64 {
    number.into()
}

/// Apply a seccomp-bpf filter that blocks network syscalls.
///
/// Blocks AF_INET and AF_INET6 sockets. AF_UNIX remains available by default
/// for compatibility. Process-group escape denial is a separate opt-in for
/// runners that guarantee cleanup of a dedicated child group. Strict formula
/// execution additionally rejects cross-process inspection, kernel control,
/// namespaces, async-I/O bypasses, and path-based metadata mutation.
/// Based on the syscall list from OpenAI's codex-linux-sandbox.
pub fn apply_seccomp_net_filter(
    deny_local_sockets: bool,
    deny_process_group_escape: bool,
    strict_formula_execution: bool,
) -> Result<()> {
    // Must set PR_SET_NO_NEW_PRIVS before installing seccomp filter
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret != 0 {
        return Err(eyre!(
            "failed to set PR_SET_NO_NEW_PRIVS: {}",
            std::io::Error::last_os_error()
        ));
    }

    let arch = std::env::consts::ARCH;
    let target_arch = match arch {
        "x86_64" => TargetArch::x86_64,
        "aarch64" => TargetArch::aarch64,
        _ => return Err(eyre!("unsupported architecture for seccomp: {arch}")),
    };

    // Block socket() and socketpair() for AF_INET (2) and AF_INET6 (10).
    // AF_UNIX (1) is included only for strict callers.
    let socket_rule_inet = SeccompRule::new(vec![SeccompCondition::new(
        0, // first arg: domain/family
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Eq,
        libc::AF_INET as u64,
    )?])?;

    let socket_rule_inet6 = SeccompRule::new(vec![SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Eq,
        libc::AF_INET6 as u64,
    )?])?;

    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

    let socket_rule_unix = SeccompRule::new(vec![SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Eq,
        libc::AF_UNIX as u64,
    )?])?;

    let mut socket_rules = vec![socket_rule_inet, socket_rule_inet6];
    if deny_local_sockets {
        socket_rules.push(socket_rule_unix);
    }

    // Compatibility callers block only the requested families. Strict formula
    // callers must not inherit gaps for privileged or future address families,
    // so reject every socket and socketpair creation attempt.
    for syscall in [
        syscall_number(libc::SYS_socket),
        syscall_number(libc::SYS_socketpair),
    ] {
        if strict_formula_execution {
            rules.insert(syscall, vec![]);
        } else {
            rules.insert(syscall, socket_rules.clone());
        }
    }
    if deny_process_group_escape {
        // The command leader has already entered a dedicated group before this
        // filter is installed. Unconditional syscall entries prevent every
        // filtered descendant from moving itself or a sibling out of it.
        rules.insert(syscall_number(libc::SYS_setpgid), vec![]);
        rules.insert(syscall_number(libc::SYS_setsid), vec![]);
    }

    if strict_formula_execution {
        for syscall in [
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_process_madvise,
            libc::SYS_process_mrelease,
            libc::SYS_pidfd_getfd,
            libc::SYS_pidfd_send_signal,
            libc::SYS_kcmp,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_request_key,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_unshare,
            libc::SYS_setns,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
            libc::SYS_init_module,
            libc::SYS_finit_module,
            libc::SYS_delete_module,
            libc::SYS_reboot,
            libc::SYS_kexec_load,
            libc::SYS_kexec_file_load,
            libc::SYS_swapon,
            libc::SYS_swapoff,
            libc::SYS_acct,
            libc::SYS_open_by_handle_at,
            libc::SYS_userfaultfd,
            libc::SYS_sethostname,
            libc::SYS_setdomainname,
            libc::SYS_fsopen,
            libc::SYS_fsconfig,
            libc::SYS_fsmount,
            libc::SYS_move_mount,
            libc::SYS_open_tree,
            libc::SYS_mount_setattr,
            libc::SYS_fchown,
            libc::SYS_fchownat,
            libc::SYS_setxattr,
            libc::SYS_lsetxattr,
            libc::SYS_fsetxattr,
            libc::SYS_getxattr,
            libc::SYS_lgetxattr,
            libc::SYS_fgetxattr,
            libc::SYS_listxattr,
            libc::SYS_llistxattr,
            libc::SYS_flistxattr,
            libc::SYS_removexattr,
            libc::SYS_lremovexattr,
            libc::SYS_fremovexattr,
        ] {
            rules.insert(syscall_number(syscall), vec![]);
        }
        // Permission and timestamp mutation remain available. Every retained authority is
        // CLOEXEC, so executed formula code can acquire timestamp-capable file
        // descriptors only through the Landlock-confined hierarchy.
        // clone remains available for ordinary processes and threads, but no
        // descendant may create a new namespace.
        let namespace_flags = [
            libc::CLONE_NEWCGROUP,
            libc::CLONE_NEWIPC,
            libc::CLONE_NEWNET,
            libc::CLONE_NEWNS,
            libc::CLONE_NEWPID,
            libc::CLONE_NEWUSER,
            libc::CLONE_NEWUTS,
        ];
        let mut clone_rules = Vec::with_capacity(namespace_flags.len());
        for flag in namespace_flags {
            let flag = flag as u64;
            clone_rules.push(SeccompRule::new(vec![SeccompCondition::new(
                0,
                SeccompCmpArgLen::Qword,
                SeccompCmpOp::MaskedEq(flag),
                flag,
            )?])?);
        }
        rules.insert(syscall_number(libc::SYS_clone), clone_rules);

        // Keep self-directed scheduler/resource operations available while
        // preventing a formula from changing another same-UID process.
        let nonzero_pid_rule = SeccompRule::new(vec![SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            0,
        )?])?;
        for syscall in [
            libc::SYS_prlimit64,
            libc::SYS_sched_setaffinity,
            libc::SYS_sched_setscheduler,
            libc::SYS_sched_setparam,
            libc::SYS_sched_setattr,
            libc::SYS_get_robust_list,
            libc::SYS_migrate_pages,
            libc::SYS_move_pages,
        ] {
            rules.insert(syscall_number(syscall), vec![nonzero_pid_rule.clone()]);
        }
        rules.insert(
            syscall_number(libc::SYS_setpriority),
            vec![
                SeccompRule::new(vec![SeccompCondition::new(
                    0,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::Ne,
                    libc::PRIO_PROCESS as u64,
                )?])?,
                SeccompRule::new(vec![SeccompCondition::new(
                    1,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::Ne,
                    0,
                )?])?,
            ],
        );
        // IOPRIO_WHO_PROCESS is 1 in the Linux UAPI. The libc crate does not
        // expose the constant on every supported architecture.
        rules.insert(
            syscall_number(libc::SYS_ioprio_set),
            vec![
                SeccompRule::new(vec![SeccompCondition::new(
                    0,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::Ne,
                    1,
                )?])?,
                SeccompRule::new(vec![SeccompCondition::new(
                    1,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::Ne,
                    0,
                )?])?,
            ],
        );

        #[cfg(target_arch = "x86_64")]
        for syscall in [
            libc::SYS_chown,
            libc::SYS_lchown,
            libc::SYS_iopl,
            libc::SYS_ioperm,
        ] {
            rules.insert(syscall_number(syscall), vec![]);
        }

        // Make modern libc fall back to clone(2), whose namespace flags can be
        // inspected above, without breaking normal thread/process creation.
        let clone3_rules = BTreeMap::from([(syscall_number(libc::SYS_clone3), vec![])]);
        apply_rules(
            clone3_rules,
            SeccompAction::Errno(libc::ENOSYS as u32),
            target_arch,
        )?;
    }

    apply_rules(rules, SeccompAction::Errno(libc::EPERM as u32), target_arch)
}

fn apply_rules(
    rules: BTreeMap<i64, Vec<SeccompRule>>,
    match_action: SeccompAction,
    target_arch: TargetArch,
) -> Result<()> {
    let filter: BpfProgram =
        SeccompFilter::new(rules, SeccompAction::Allow, match_action, target_arch)?.try_into()?;
    seccompiler::apply_filter(&filter).map_err(|e| eyre!("failed to apply seccomp filter: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD_ENV: &str = "MISE_TEST_SECCOMP_STRICT_POLICY";
    const SENTINEL_ENV: &str = "MISE_TEST_SECCOMP_OUTSIDE_SENTINEL";

    unsafe fn call_setpgid() -> i32 {
        unsafe { libc::setpgid(0, 0) }
    }

    unsafe fn call_setsid() -> i32 {
        unsafe { libc::setsid() }
    }

    fn assert_child_syscall(blocked: bool, syscall: unsafe fn() -> i32) {
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            let result = unsafe { syscall() };
            let matched = if blocked { result == -1 } else { result != -1 };
            unsafe { libc::_exit(i32::from(!matched)) };
        }
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }

    fn assert_strict_syscall_blocked(syscall: libc::c_long) {
        let result = unsafe { libc::syscall(syscall, 0, 0, 0, 0, 0, 0) };
        assert_eq!(result, -1, "syscall {syscall} unexpectedly succeeded");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM),
            "syscall {syscall} was not rejected by strict seccomp"
        );
    }

    #[test]
    fn test_local_socket_policy_is_opt_in() {
        if let Ok(value) = std::env::var(CHILD_ENV) {
            let deny_local_sockets = matches!(value.as_str(), "local" | "strict");
            let deny_process_group_escape = matches!(value.as_str(), "escape" | "strict");
            let strict_formula_execution = value == "strict";
            apply_seccomp_net_filter(
                deny_local_sockets,
                deny_process_group_escape,
                strict_formula_execution,
            )
            .unwrap();

            let inet = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            assert_eq!(inet, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EPERM)
            );

            let local = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            let mut pair = [-1; 2];
            let local_pair =
                unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, pair.as_mut_ptr()) };
            if deny_local_sockets {
                assert_eq!(local, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                assert_eq!(local_pair, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
            } else {
                assert!(local >= 0);
                assert_eq!(local_pair, 0);
                unsafe { libc::close(local) };
                for fd in pair {
                    unsafe { libc::close(fd) };
                }
            }
            assert_child_syscall(deny_process_group_escape, call_setpgid);
            assert_child_syscall(deny_process_group_escape, call_setsid);
            if strict_formula_execution {
                use std::os::fd::AsRawFd;
                let sentinel = std::path::PathBuf::from(std::env::var_os(SENTINEL_ENV).unwrap());
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&sentinel)
                    .unwrap();
                assert_eq!(
                    unsafe { libc::fchown(file.as_raw_fd(), libc::geteuid(), libc::getegid()) },
                    -1
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                let sentinel_c =
                    std::ffi::CString::new(sentinel.as_os_str().as_encoded_bytes().to_vec())
                        .unwrap();
                let xattr_name = c"user.mise-seccomp-test";
                let path_xattr = unsafe {
                    libc::getxattr(
                        sentinel_c.as_ptr(),
                        xattr_name.as_ptr(),
                        std::ptr::null_mut(),
                        0,
                    )
                };
                assert_eq!(path_xattr, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                let fd_xattr = unsafe {
                    libc::fgetxattr(
                        file.as_raw_fd(),
                        xattr_name.as_ptr(),
                        std::ptr::null_mut(),
                        0,
                    )
                };
                assert_eq!(fd_xattr, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                let xattr_value = b"value";
                assert_eq!(
                    unsafe {
                        libc::fsetxattr(
                            file.as_raw_fd(),
                            xattr_name.as_ptr(),
                            xattr_value.as_ptr().cast(),
                            xattr_value.len(),
                            0,
                        )
                    },
                    -1
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                assert_eq!(
                    unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) },
                    -1
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                assert_eq!(
                    unsafe { libc::fremovexattr(file.as_raw_fd(), xattr_name.as_ptr()) },
                    -1
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                assert_eq!(
                    unsafe {
                        libc::utimensat(libc::AT_FDCWD, sentinel_c.as_ptr(), std::ptr::null(), 0)
                    },
                    0,
                    "pathname timestamp updates must remain available to Landlock-confined builds"
                );
                assert_eq!(
                    unsafe {
                        libc::syscall(
                            libc::SYS_utimensat,
                            file.as_raw_fd(),
                            std::ptr::null::<libc::c_char>(),
                            std::ptr::null::<libc::timespec>(),
                            0,
                        )
                    },
                    0
                );

                for family in [libc::AF_NETLINK, libc::AF_PACKET] {
                    let socket = unsafe { libc::socket(family, libc::SOCK_RAW, 0) };
                    assert_eq!(
                        socket, -1,
                        "strict formula execution created family {family} socket"
                    );
                    assert_eq!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::EPERM)
                    );
                }

                for syscall in [
                    libc::SYS_ptrace,
                    libc::SYS_process_vm_readv,
                    libc::SYS_process_madvise,
                    libc::SYS_process_mrelease,
                    libc::SYS_pidfd_getfd,
                    libc::SYS_pidfd_send_signal,
                    libc::SYS_kcmp,
                    libc::SYS_keyctl,
                    libc::SYS_bpf,
                    libc::SYS_perf_event_open,
                    libc::SYS_mount,
                    libc::SYS_unshare,
                    libc::SYS_setns,
                    libc::SYS_io_uring_setup,
                    libc::SYS_open_by_handle_at,
                    libc::SYS_getxattr,
                    libc::SYS_listxattr,
                ] {
                    assert_strict_syscall_blocked(syscall);
                }
                for syscall in [
                    libc::SYS_prlimit64,
                    libc::SYS_sched_setaffinity,
                    libc::SYS_sched_setscheduler,
                    libc::SYS_sched_setparam,
                    libc::SYS_sched_setattr,
                    libc::SYS_get_robust_list,
                    libc::SYS_migrate_pages,
                    libc::SYS_move_pages,
                ] {
                    let result = unsafe { libc::syscall(syscall, 1, 0, 0, 0, 0, 0) };
                    assert_eq!(
                        result, -1,
                        "cross-process syscall {syscall} unexpectedly succeeded"
                    );
                    assert_eq!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::EPERM)
                    );
                }
                for (syscall, arg0, arg1) in [
                    (libc::SYS_setpriority, libc::PRIO_PGRP as libc::c_long, 0),
                    (libc::SYS_setpriority, libc::PRIO_PROCESS as libc::c_long, 1),
                    (libc::SYS_ioprio_set, 2, 0),
                    (libc::SYS_ioprio_set, 1, 1),
                ] {
                    let result = unsafe { libc::syscall(syscall, arg0, arg1, 0, 0, 0, 0) };
                    assert_eq!(
                        result, -1,
                        "cross-process syscall {syscall} unexpectedly succeeded"
                    );
                    assert_eq!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::EPERM)
                    );
                }
                let clone = unsafe {
                    libc::syscall(
                        libc::SYS_clone,
                        libc::CLONE_NEWUSER | libc::CLONE_THREAD,
                        0,
                        0,
                        0,
                        0,
                    )
                };
                assert_eq!(clone, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EPERM)
                );
                let clone3 = unsafe { libc::syscall(libc::SYS_clone3, 0, 0) };
                assert_eq!(clone3, -1);
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ENOSYS)
                );
            }
            return;
        }

        let test_name = "sandbox::seccomp::tests::test_local_socket_policy_is_opt_in";
        let cwd_sentinel = tempfile::NamedTempFile::new_in(std::env::current_dir().unwrap())
            .expect("create parent cwd sentinel");
        let outside_sentinel = tempfile::NamedTempFile::new().unwrap();
        for policy in ["default", "local", "escape", "strict"] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name])
                .env(CHILD_ENV, policy)
                .env(SENTINEL_ENV, outside_sentinel.path())
                .env(crate::test::INHERIT_TEST_PROCESS_ENV, "1")
                .status()
                .unwrap();
            assert!(status.success(), "seccomp child failed for {policy}");
            assert!(
                cwd_sentinel.path().is_file(),
                "seccomp child reset the parent test process's cwd"
            );
        }
    }
}
