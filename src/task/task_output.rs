use crate::config::Settings;
use crate::env;
use console;

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    strum::Display,
    strum::EnumString,
    strum::EnumIs,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum TaskOutput {
    Interleave,
    KeepOrder,
    #[default]
    Prefix,
    Replacing,
    Timed,
    Quiet,
    Silent,
}

impl TaskOutput {
    /// Reduce a (possibly verbosity-carrying) output value to a STYLE-only value.
    ///
    /// `Quiet` historically meant "interleaved streams + suppressed mise output".
    /// The stream-style half of that is `Interleave`; the suppression half is
    /// applied separately via the `quiet()` predicate at mise's own metadata
    /// print sites. `Silent` is resolved before this is ever called (see
    /// `OutputHandler::output`), so it passes through unchanged.
    pub(crate) fn style_only(self) -> TaskOutput {
        match self {
            TaskOutput::Quiet => TaskOutput::Interleave,
            other => other,
        }
    }

    /// Like [`style_only`](Self::style_only), but a `raw` task downgrades every
    /// non-suppression style to `Interleave` (raw needs inherited stdio for
    /// stdin passthrough). Mirrors the pre-existing `raw` handling for the
    /// global `task.output` setting.
    pub(crate) fn style_with_raw(self, raw: bool) -> TaskOutput {
        let style = self.style_only();
        // `raw` needs inherited stdio, but must never un-silence a `Silent` task.
        if raw && !matches!(style, TaskOutput::Silent) {
            TaskOutput::Interleave
        } else {
            style
        }
    }
}

/// Returns the first line of a message for display unless task_show_full_cmd is true
/// In CI mode, returns the full first line without truncation
/// Otherwise, truncates to terminal width with ellipsis
pub fn trunc(prefix: &str, msg: &str) -> String {
    let settings = Settings::get();

    // Skip width truncation when explicitly disabled
    if settings.task.show_full_cmd {
        return msg.to_string();
    }
    let msg = msg.lines().next().unwrap_or_default();
    if settings.ci {
        return msg.to_string();
    }
    let prefix_len = console::measure_text_width(prefix);
    // Ensure we have at least 20 characters for the message, even with very long prefixes
    let available_width = (*env::TERM_WIDTH).saturating_sub(prefix_len + 1);
    let max_width = available_width.max(20); // Always show at least 20 chars of message
    console::truncate_str(msg, max_width, "…").to_string()
}
