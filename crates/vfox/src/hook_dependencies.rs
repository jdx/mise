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
    /// The directory relative `watch_files` resolved from. The result only
    /// applies there: elsewhere the same relative paths name other files.
    pub cwd: Option<PathBuf>,
}

impl Default for HookDependencies {
    fn default() -> Self {
        Self {
            env_reads: BTreeMap::new(),
            watch_files: vec![],
            cacheable: true,
            cwd: None,
        }
    }
}

impl HookDependencies {
    /// Add files to watch, resolving relative paths from the current directory.
    pub(crate) fn watch(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if path.is_absolute() {
                self.watch_files.push(path);
            } else {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.watch_files.push(cwd.join(path));
                self.cwd = Some(cwd);
            }
        }
    }
}

/// Where a running hook records its [`HookDependencies`]. See
/// [`crate::Vfox::hook_dependencies`].
pub type HookDependencyRecorder = Arc<Mutex<HookDependencies>>;
