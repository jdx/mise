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

/// An ioctl request number as the filter compares it. libc gives the numbers a
/// different type per C library (an unsigned long on glibc, an int on musl);
/// every request the kernel defines fits in the unsigned int it reads.
fn ioctl_request<T: TryInto<u32>>(request: T) -> Result<u64> {
    request
        .try_into()
        .map(u64::from)
        .map_err(|_| eyre!("ioctl request number does not fit in 32 bits"))
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NetworkPolicy {
    Unrestricted,
    Blocked,
    TcpOnly,
}

/// Apply the sandbox's seccomp-bpf filter.
///
/// Always refuses the ioctls that push input into a terminal (TIOCSTI) or
/// paste a console selection into it (TIOCLINUX): `/dev/tty` stays writable in
/// every sandbox, and a typed-in command runs in the user's shell outside it.
///
/// With `deny_net`, blocks every socket family except AF_UNIX (needed by many
/// tools) and AF_NETLINK, and blocks io_uring, which can create sockets without
/// entering socket(). The port allowlist mode also permits ordinary IPv4 and
/// IPv6 TCP sockets, whose connections are checked by Landlock. Based on the
/// syscall list from OpenAI's
/// codex-linux-sandbox.
///
/// The filter kills a process that makes a system call through another
/// architecture's ABI, such as a 32-bit program on x86_64, because its numbers
/// would mean other calls.
pub(super) fn apply_seccomp_filter(network: NetworkPolicy, deny_process: bool) -> Result<()> {
    let arch = std::env::consts::ARCH;
    let target_arch = match arch {
        "x86_64" => TargetArch::x86_64,
        "aarch64" => TargetArch::aarch64,
        // the terminal rule alone is not worth refusing to run over
        _ if network == NetworkPolicy::Unrestricted && !deny_process => return Ok(()),
        _ => return Err(eyre!("unsupported architecture for seccomp: {arch}")),
    };

    // Must set PR_SET_NO_NEW_PRIVS before installing seccomp filter
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret != 0 {
        return Err(eyre!(
            "failed to set PR_SET_NO_NEW_PRIVS: {}",
            std::io::Error::last_os_error()
        ));
    }

    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

    // The kernel reads an ioctl request as an unsigned int, so compare the low
    // 32 bits: a request with upper bits set still means TIOCSTI.
    let mut terminal_injection = Vec::new();
    for request in [libc::TIOCSTI, libc::TIOCLINUX] {
        terminal_injection.push(SeccompRule::new(vec![SeccompCondition::new(
            1,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            ioctl_request(request)?,
        )?])?);
    }
    rules.insert(syscall_number(libc::SYS_ioctl), terminal_injection.clone());
    // ioctl is not shared with x32, which has its own number for it.
    #[cfg(target_arch = "x86_64")]
    rules.insert(514 | X32_SYSCALL_BIT, terminal_injection);

    if network != NetworkPolicy::Unrestricted {
        let local_only = vec![SeccompRule::new(vec![
            socket_arg_ne(0, libc::AF_UNIX)?,
            socket_arg_ne(0, libc::AF_NETLINK)?,
        ])?];
        let socket_rules = match network {
            NetworkPolicy::TcpOnly => tcp_socket_rules()?,
            _ => local_only.clone(),
        };
        for syscall in native_and_x32(syscall_number(libc::SYS_socket)) {
            rules.insert(syscall, socket_rules.clone());
        }
        for syscall in native_and_x32(syscall_number(libc::SYS_socketpair)) {
            rules.insert(syscall, local_only.clone());
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

fn socket_arg_ne(arg: u8, value: libc::c_int) -> Result<SeccompCondition> {
    Ok(SeccompCondition::new(
        arg,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Ne,
        value as u64,
    )?)
}

fn tcp_socket_rules() -> Result<Vec<SeccompRule>> {
    // Allow only the local families and IP sockets that Landlock can mediate.
    // Each seccomp rule ANDs its conditions. Either a non-stream type or a
    // non-TCP protocol is enough to reject a socket in one of the IP families.
    let mut rules = vec![SeccompRule::new(vec![
        socket_arg_ne(0, libc::AF_UNIX)?,
        socket_arg_ne(0, libc::AF_NETLINK)?,
        socket_arg_ne(0, libc::AF_INET)?,
        socket_arg_ne(0, libc::AF_INET6)?,
    ])?];
    for family in [libc::AF_INET, libc::AF_INET6] {
        let is_family =
            SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, family as u64)?;
        rules.push(SeccompRule::new(vec![
            is_family.clone(),
            // Compare the type without the permitted CLOEXEC/NONBLOCK flags.
            socket_arg_ne(1, libc::SOCK_STREAM)?,
            socket_arg_ne(1, libc::SOCK_STREAM | libc::SOCK_CLOEXEC)?,
            socket_arg_ne(1, libc::SOCK_STREAM | libc::SOCK_NONBLOCK)?,
            socket_arg_ne(
                1,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            )?,
        ])?);
        rules.push(SeccompRule::new(vec![
            is_family,
            socket_arg_ne(2, 0)?,
            socket_arg_ne(2, libc::IPPROTO_TCP)?,
        ])?);
    }
    Ok(rules)
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
            apply_seccomp_filter(NetworkPolicy::Blocked, false).unwrap();
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

    fn ioctl_errno(number: i64, fd: libc::c_int, request: u64) -> i32 {
        let byte = b'x';
        let ret = unsafe { libc::syscall(number as libc::c_long, fd, request, &byte) };
        if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap()
        } else {
            0
        }
    }

    /// Every sandbox refuses TIOCSTI and TIOCLINUX, so a sandboxed program
    /// cannot type a command into the terminal it shares with the user's
    /// shell. On a file that is not a terminal the kernel answers ENOTTY, and
    /// the filter answers EPERM before the kernel looks, so this holds on
    /// kernels that refuse TIOCSTI to everyone as well.
    #[test]
    fn test_terminal_input_injection_is_refused_in_every_sandbox() {
        let null = std::fs::File::open("/dev/null").unwrap();
        let fd = std::os::fd::AsRawFd::as_raw_fd(&null);
        let ioctl = syscall_number(libc::SYS_ioctl);
        let tiocsti = ioctl_request(libc::TIOCSTI).unwrap();
        assert_eq!(ioctl_errno(ioctl, fd, tiocsti), libc::ENOTTY);
        std::thread::spawn(move || {
            apply_seccomp_filter(NetworkPolicy::Unrestricted, false).unwrap();
            for request in [
                tiocsti,
                tiocsti | 0xffff_ffff_0000_0000,
                ioctl_request(libc::TIOCLINUX).unwrap(),
            ] {
                assert_eq!(ioctl_errno(ioctl, fd, request), libc::EPERM, "{request:#x}");
            }
            #[cfg(target_arch = "x86_64")]
            assert_eq!(ioctl_errno(514 | X32_SYSCALL_BIT, fd, tiocsti), libc::EPERM);
            // other ioctls still reach the kernel
            assert_eq!(
                ioctl_errno(ioctl, fd, ioctl_request(libc::TIOCGWINSZ).unwrap()),
                libc::ENOTTY
            );
        })
        .join()
        .unwrap();
        drop(null);
    }
}
