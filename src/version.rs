use std::sync::LazyLock as Lazy;

use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::platform::{ARCH, OS};

pub(crate) static VERSION_PLAIN: Lazy<String> = Lazy::new(|| {
    let mut v = V.to_string();
    if cfg!(debug_assertions) {
        v.push_str("-DEBUG");
    };
    v
});

pub(crate) static VERSION: Lazy<String> = Lazy::new(|| {
    let build_time = BUILD_TIME.format("%Y-%m-%d");
    let v = &*VERSION_PLAIN;
    format!("{v} {os}-{arch} ({build_time})", os = *OS, arch = *ARCH)
});

pub(crate) static V: Lazy<Versioning> =
    Lazy::new(|| Versioning::new(env!("CARGO_PKG_VERSION")).unwrap());
