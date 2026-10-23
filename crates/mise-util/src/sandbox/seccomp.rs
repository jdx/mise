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

/// Set on a system call number to call it through the x32 ABI.
///
/// An x86_64 kernel built with x32 support takes x32 calls from any 64-bit
/// process, and they report the same audit architecture as native calls, so
/// a filter that names only the native number lets the x32 one through.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: i64 = 0x4000_0000;

/// The numbers under which the kernel accepts `number`: the native one and,
/// on x86_64, the x32 one. Only for system calls the x32 ABI shares with
/// x86_64 (marked `common` in the kernel's syscall table).
fn native_and_x32(number: i64) -> Vec<i64> {
    #[cfg(target_arch = "x86_64")]
    return vec![number, number | X32_SYSCALL_BIT];
    #[cfg(not(target_arch = "x86_64"))]
    return vec![number];
}

/// Apply a seccomp-bpf filter that blocks network and/or process syscalls.
///
/// Blocks every socket family except AF_UNIX (needed by many tools) and
/// AF_NETLINK, and blocks io_uring, which can create sockets without entering
/// socket(). Based on the syscall list from OpenAI's codex-linux-sandbox.
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

    // Block socket() and socketpair() for every family that is not host-local.
    // Refusing only AF_INET and AF_INET6 misses families that carry IP traffic
    // under another number, such as AF_SMC, which falls back to plain TCP.
    // The conditions of one rule are ANDed.
    // Allow AF_UNIX (1) — needed by many tools for IPC
    // Allow AF_NETLINK (16) — libc uses it to list interfaces and addresses
    let socket_rule_not_local = SeccompRule::new(vec![
        SeccompCondition::new(
            0, // first arg: domain/family
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            libc::AF_UNIX as u64,
        )?,
        SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            libc::AF_NETLINK as u64,
        )?,
    ])?;

    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

    if deny_net {
        // Without a network socket the process cannot initiate network traffic.
        for syscall in [
            syscall_number(libc::SYS_socket),
            syscall_number(libc::SYS_socketpair),
        ]
        .into_iter()
        .flat_map(native_and_x32)
        {
            rules.insert(syscall, vec![socket_rule_not_local.clone()]);
        }
        // IORING_OP_SOCKET creates a socket while the kernel services a ring
        // request, which never enters socket() and so is invisible to the
        // rule above.
        for syscall in [
            syscall_number(libc::SYS_io_uring_setup),
            syscall_number(libc::SYS_io_uring_enter),
            syscall_number(libc::SYS_io_uring_register),
        ]
        .into_iter()
        .flat_map(native_and_x32)
        {
            rules.insert(syscall, Vec::new());
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
        ]
        .into_iter()
        .flat_map(native_and_x32)
        {
            rules.insert(syscall, deny.clone());
        }
        // ptrace is not shared with x32, which has its own number for it.
        #[cfg(target_arch = "x86_64")]
        rules.insert(521 | X32_SYSCALL_BIT, deny.clone());
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

#[cfg(all(test, any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tests {
    use super::*;

    fn errno_of(ret: libc::c_long) -> i32 {
        if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap()
        } else {
            unsafe { libc::close(ret as libc::c_int) };
            0
        }
    }

    fn socket_errno(number: i64, family: libc::c_int, ty: libc::c_int) -> i32 {
        errno_of(unsafe {
            libc::syscall(number as libc::c_long, family, ty | libc::SOCK_CLOEXEC, 0)
        })
    }

    /// `--deny-net` leaves only host-local sockets: refusing AF_INET and
    /// AF_INET6 alone let AF_SMC (which falls back to TCP), io_uring's
    /// IORING_OP_SOCKET and, on x86_64, the x32 socket() reach the network.
    #[test]
    fn test_deny_net_leaves_only_local_sockets() {
        const AF_SMC: libc::c_int = 43;
        // The filter binds to the calling thread, so a scratch thread keeps it
        // away from the rest of the test run.
        std::thread::spawn(|| {
            apply_seccomp_filter(true, false).unwrap();
            let socket = syscall_number(libc::SYS_socket);
            for family in [libc::AF_INET, libc::AF_INET6, libc::AF_VSOCK, AF_SMC] {
                assert_eq!(
                    socket_errno(socket, family, libc::SOCK_STREAM),
                    libc::EPERM,
                    "family {family}"
                );
            }
            assert_eq!(socket_errno(socket, libc::AF_UNIX, libc::SOCK_STREAM), 0);
            assert_eq!(socket_errno(socket, libc::AF_NETLINK, libc::SOCK_RAW), 0);
            #[cfg(target_arch = "x86_64")]
            assert_eq!(
                socket_errno(socket | X32_SYSCALL_BIT, libc::AF_INET, libc::SOCK_STREAM),
                libc::EPERM
            );
            let mut params = [0u8; 120]; // struct io_uring_params
            let ret = unsafe { libc::syscall(libc::SYS_io_uring_setup, 1, params.as_mut_ptr()) };
            assert_eq!(errno_of(ret), libc::EPERM);
        })
        .join()
        .unwrap();
    }
}
