use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What a hook's result depends on, gathered while the hook runs, so a caller
/// that caches the result can tell when it no longer applies.
#[derive(Debug, Clone)]
pub struct HookDependencies {
    /// Each variable Lua `os.getenv` read, with the value it returned (`None`
    /// for unset).
    pub env_reads: BTreeMap<String, Option<String>>,
    /// Files and directories whose changes make the result stale, from
    /// `mise.cache.watch_files` or a `watch_files` field in the hook's result.
    pub watch_files: Vec<PathBuf>,
    /// False when the hook asked for its result not to be cached, through
    /// `mise.cache.disable` or `cacheable = false` in its result.
    pub cacheable: bool,
}

impl Default for HookDependencies {
    fn default() -> Self {
        Self {
            env_reads: BTreeMap::new(),
            watch_files: vec![],
            cacheable: true,
        }
    }
}

/// Where a running hook records its [`HookDependencies`]. See
/// [`crate::Vfox::hook_dependencies`].
pub type HookDependencyRecorder = Arc<Mutex<HookDependencies>>;
