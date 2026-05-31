pub const BINARY_OS_TOKENS: &[&str] = &[
    "linux",
    "manylinux",
    "musllinux",
    "darwin",
    "macos",
    "osx",
    "windows",
    "win",
    "freebsd",
    "openbsd",
    "netbsd",
    "android",
    "unknown",
];

pub const BINARY_ARCH_TOKENS: &[&str] = &[
    "x86_64", "aarch64", "ppc64le", "ppc64", "armv7", "armv6", "arm64", "amd64", "mipsel",
    "riscv64", "s390x", "i686", "i386", "x64", "mips", "arm", "x86",
];

const PREFERRED_NAME_OS_TOKENS: &[&str] = &[
    "ubuntu", "debian", "fedora", "centos", "rhel", "alpine", "arch", "mac", "macosx", "win32",
    "win64", "mingw", "mingw32", "mingw64", "w64",
];
const PREFERRED_NAME_ARCH_TOKENS: &[&str] = &["64"];
const QUALIFIER_TOKENS: &[&str] = &["gnu", "glibc", "musl", "msvc", "pc", "apple"];

pub fn is_platform_or_version_token(token: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    if is_os_token(token) || is_arch_token(token) || QUALIFIER_TOKENS.contains(&token) {
        return true;
    }
    if token
        .strip_prefix('v')
        .and_then(|token| token.chars().next())
        .is_some_and(|c| c.is_ascii_digit())
    {
        return true;
    }

    token.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn is_os_token(token: &str) -> bool {
    token.starts_with("manylinux")
        || token.starts_with("musllinux")
        || BINARY_OS_TOKENS.contains(&token)
        || PREFERRED_NAME_OS_TOKENS.contains(&token)
}

fn is_arch_token(token: &str) -> bool {
    BINARY_ARCH_TOKENS.contains(&token) || PREFERRED_NAME_ARCH_TOKENS.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_or_version_tokens_include_linux_variants() {
        for token in [
            "osx",
            "manylinux",
            "manylinux2014",
            "musllinux",
            "musllinux_1_2",
        ] {
            assert!(is_platform_or_version_token(token));
        }
    }
}
