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
/// runners that guarantee cleanup of a dedicated child group.
/// Based on the syscall list from OpenAI's codex-linux-sandbox.
pub fn apply_seccomp_net_filter(
    deny_local_sockets: bool,
    deny_process_group_escape: bool,
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

    // Block socket() and socketpair() for the selected families.
    // This is sufficient — if you can't create an inet socket, you can't do networking
    for syscall in [
        syscall_number(libc::SYS_socket),
        syscall_number(libc::SYS_socketpair),
    ] {
        rules.insert(syscall, socket_rules.clone());
    }
    if deny_process_group_escape {
        // The command leader has already entered a dedicated group before this
        // filter is installed. Unconditional syscall entries prevent every
        // filtered descendant from moving itself or a sibling out of it.
        rules.insert(syscall_number(libc::SYS_setpgid), vec![]);
        rules.insert(syscall_number(libc::SYS_setsid), vec![]);
    }

    let filter: BpfProgram = SeccompFilter::new(
        rules,
        SeccompAction::Allow,                     // default: allow everything
        SeccompAction::Errno(libc::EPERM as u32), // blocked syscalls return EPERM
        target_arch,
    )?
    .try_into()?;

    seccompiler::apply_filter(&filter).map_err(|e| eyre!("failed to apply seccomp filter: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD_ENV: &str = "MISE_TEST_SECCOMP_STRICT_POLICY";

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

    #[test]
    fn test_local_socket_policy_is_opt_in() {
        if let Ok(value) = std::env::var(CHILD_ENV) {
            let deny_local_sockets = matches!(value.as_str(), "local" | "strict");
            let deny_process_group_escape = matches!(value.as_str(), "escape" | "strict");
            apply_seccomp_net_filter(deny_local_sockets, deny_process_group_escape).unwrap();

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
            return;
        }

        let test_name = "sandbox::seccomp::tests::test_local_socket_policy_is_opt_in";
        for policy in ["default", "local", "escape", "strict"] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name])
                .env(CHILD_ENV, policy)
                .status()
                .unwrap();
            assert!(status.success(), "seccomp child failed for {policy}");
        }
    }
}
