//! Deprecated spellings the parsers in this crate still accept.
//!
//! A parser reports each one with [`report`] instead of logging it, because the warning, the
//! version it starts in and the version it is removed in belong to mise.

use std::sync::OnceLock;

/// A deprecated spelling a parser in this crate accepted.
///
/// The parser reports it with [`report`] instead of logging, because the warning, the
/// version it starts in and the version it is removed in belong to mise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deprecation {
    /// `env.mise` in place of `env._`.
    EnvMise,
    /// `value` in a built-in `file`, `path` or `source` directive object.
    DirectiveValue,
    /// `values` in a built-in `file`, `path` or `source` directive object.
    DirectiveValues,
    /// `alias.<tool>.<from> = <to>` in place of `alias.<tool>.versions.<from> = <to>`.
    ToolVersionAliases,
}

static HANDLER: OnceLock<fn(Deprecation)> = OnceLock::new();

/// Where the parser sends each [`Deprecation`] it accepts. mise registers this at startup.
pub fn set_handler(handler: fn(Deprecation)) {
    let _ = HANDLER.set(handler);
}

pub(crate) fn report(deprecation: Deprecation) {
    if let Some(handler) = HANDLER.get() {
        handler(deprecation);
    }
}
