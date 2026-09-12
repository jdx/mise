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

/// Apply a seccomp-bpf filter that blocks network and/or process syscalls.
///
/// Blocks AF_INET and AF_INET6 sockets while allowing AF_UNIX (needed by many tools).
/// Based on the syscall list from OpenAI's codex-linux-sandbox.
pub(super) fn apply_seccomp_filter(deny_net: bool, deny_process: bool) -> Result<()> {
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

    // Block socket() and socketpair() for AF_INET (2) and AF_INET6 (10)
    // Allow AF_UNIX (1) — needed by many tools for IPC
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

    if deny_net {
        // Block socket() and socketpair() for inet families. Without an inet
        // socket the process cannot initiate network traffic.
        for syscall in [
            syscall_number(libc::SYS_socket),
            syscall_number(libc::SYS_socketpair),
        ] {
            rules.insert(
                syscall,
                vec![socket_rule_inet.clone(), socket_rule_inet6.clone()],
            );
        }
    }

    if deny_process {
        // An empty rule chain means the syscall number alone triggers the
        // filter's match action.
        let deny = Vec::<SeccompRule>::new();
        // Linux generic syscall ABI has no fork/vfork; aarch64 implements fork via clone.
        for syscall in [
            syscall_number(libc::SYS_clone),
            syscall_number(libc::SYS_clone3),
            #[cfg(not(any(
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64"
            )))]
            syscall_number(libc::SYS_fork),
            #[cfg(not(any(
                target_arch = "aarch64",
                target_arch = "riscv64",
                target_arch = "loongarch64"
            )))]
            syscall_number(libc::SYS_vfork),
            syscall_number(libc::SYS_kill),
            syscall_number(libc::SYS_tkill),
            syscall_number(libc::SYS_tgkill),
            syscall_number(libc::SYS_ptrace),
        ] {
            rules.insert(syscall, deny.clone());
        }
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
