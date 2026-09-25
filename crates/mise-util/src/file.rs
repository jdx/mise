//! Path helpers that need nothing beyond the environment.

use crate::dirs;
use std::path::{Path, PathBuf};

/// replaces "~" with $HOME
///
/// The remainder is re-joined one component at a time rather than pushed as a
/// single slice. `Path::strip_prefix` returns a raw subslice of the input — it
/// trims the remainder's leading/trailing separators but leaves interior ones
/// untouched — so `HOME.join(rest)` only prepends a separator. On Windows that
/// made `~/.local/share/mise` expand to `C:\Users\me\.local/share/mise`, and
/// since `MISE_DATA_DIR` flows into `dirs::DATA`/`INSTALLS`, that mixed-separator
/// path surfaced verbatim in `mise where`, `mise which`, `mise ls --json`,
/// `mise bin-paths`, shims, and error messages.
///
/// Rebuilding from `components()` also folds redundant separators and `.`
/// segments; `..` is preserved. On unix the result is byte-identical to the old
/// behavior for any ordinary input.
///
/// Paths without a `~/` prefix are returned unchanged: a user-supplied
/// `C:/mise/data` stays exactly as typed. This is a tilde expander, not a path
/// normalizer — one caller passes glob patterns through it
/// (`config::expand_task_include`).
pub fn replace_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    match path.strip_prefix("~/") {
        Ok(rest) => {
            let mut expanded = dirs::HOME.to_path_buf();
            for component in rest.components() {
                expanded.push(component.as_os_str());
            }
            expanded
        }
        Err(_) => path.to_path_buf(),
    }
}
