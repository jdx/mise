use std::sync::LazyLock as Lazy;

use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::platform::{ARCH, OS};

pub static VERSION_PLAIN: Lazy<String> = Lazy::new(|| {
    let mut v = V.to_string();
    if cfg!(debug_assertions) {
        v.push_str("-DEBUG");
    };
    v
});

pub static VERSION: Lazy<String> = Lazy::new(|| {
    let build_time = BUILD_TIME.format("%Y-%m-%d");
    let v = &*VERSION_PLAIN;
    format!("{v} {os}-{arch} ({build_time})", os = *OS, arch = *ARCH)
});

pub static V: Lazy<Versioning> = Lazy::new(|| Versioning::new(env!("CARGO_PKG_VERSION")).unwrap());

/// Whether mise version `a` precedes `b`, comparing their dot-separated
/// numbers in order; a missing number counts as 0. Only for mise's own
/// `YYYY.M.P` versions, at compile time, where `Versioning` cannot run.
pub const fn version_lt(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let (mut x, mut y) = (0u64, 0u64);
        while i < a.len() && a[i] != b'.' {
            x = x * 10 + (a[i] - b'0') as u64;
            i += 1;
        }
        while j < b.len() && b[j] != b'.' {
            y = y * 10 + (b[j] - b'0') as u64;
            j += 1;
        }
        if x != y {
            return x < y;
        }
        i += 1;
        j += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::version_lt;

    #[test]
    fn version_lt_compares_each_number() {
        assert!(version_lt("2026.10.3", "2027.1.0"));
        assert!(version_lt("2026.9.9", "2026.10.0"));
        assert!(version_lt("2026.12", "2026.12.1"));
        assert!(!version_lt("2027.1.0", "2027.1.0"));
        assert!(!version_lt("2027.1", "2027.1.0"));
        assert!(!version_lt("2027.2.0", "2027.1.9"));
    }
}
