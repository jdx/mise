//! Deprecation warnings gated on the running mise version, which mise registers
//! at startup. The same rules as mise's own `deprecated_at!`: warn from
//! `warn_at`, and fail debug builds once `remove_at` is reached.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, OnceLock};

use versions::Versioning;

static VERSION: OnceLock<Versioning> = OnceLock::new();

#[doc(hidden)]
pub static DEPRECATED: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Default::default);

/// Register the running mise version, e.g. `env!("CARGO_PKG_VERSION")` of mise.
pub fn set_version(version: &str) {
    let _ = VERSION.set(Versioning::new(version).expect("invalid mise version"));
}

/// The registered mise version.
pub fn version() -> &'static Versioning {
    VERSION
        .get()
        .expect("mise_util::deprecation::set_version must be called first")
}
