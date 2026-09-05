//! The per-generation journal: what a bootstrap run changed.
//!
//! Entries are appended to the active generation as the run proceeds and
//! are the input a later rollback inverts. This version records only what
//! cannot be inverted (`Unrecorded`) and free-form notes; the reversible
//! entry kinds land with the parts that produce them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum JournalEntry {
    /// Context for someone reading `mise bootstrap generations show`.
    Note { message: String },
    /// A change rollback cannot undo. Reported, never inverted.
    Unrecorded {
        part: String,
        item: String,
        reason: String,
    },
    /// Written by a newer mise than this one.
    #[serde(other)]
    Unknown,
}

impl JournalEntry {
    /// One-line rendering for tables and `show`.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Note { message } => message.clone(),
            Self::Unrecorded { part, item, reason } => {
                format!("{part}: {item} — not recorded ({reason})")
            }
            Self::Unknown => "recorded by a newer mise".to_string(),
        }
    }
}

/// Records a free-form note on the open generation.
pub(crate) fn note(message: impl Into<String>) {
    super::scope::record(JournalEntry::Note {
        message: message.into(),
    });
}

/// Records that `part` changed `item` in a way rollback cannot undo.
pub(crate) fn unrecorded(part: &str, item: impl Into<String>, reason: impl Into<String>) {
    super::scope::record(JournalEntry::Unrecorded {
        part: part.to_string(),
        item: item.into(),
        reason: reason.into(),
    });
}
