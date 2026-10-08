//! Per-machine answers for `[vars]` entries that declare a `prompt`.
//!
//! An answer lives in `$MISE_STATE_DIR/vars.toml`, outside every config file, so
//! it never reaches a dotfiles repository. Reading an answer never prompts;
//! only a command that called [`enable`] asks, and only on a terminal.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use demand::Input;
use eyre::{Result, WrapErr};

use crate::dirs;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Let this process ask for unanswered `prompt` vars. Without it, an
/// unanswered var falls back to its `default`, or to `required`'s error.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// The file that holds the saved answers.
pub(crate) fn answers_path() -> PathBuf {
    dirs::STATE.join("vars.toml")
}

fn read_answers() -> toml::Table {
    let Ok(raw) = std::fs::read_to_string(answers_path()) else {
        return toml::Table::new();
    };
    match toml::from_str::<toml::Table>(&raw) {
        Ok(table) => table,
        Err(err) => {
            warn!("ignoring unreadable {}: {err}", answers_path().display());
            toml::Table::new()
        }
    }
}

/// The answer saved for `key` on this machine, if any.
pub(crate) fn saved(key: &str) -> Option<String> {
    match read_answers().get("vars")?.as_table()?.get(key)? {
        // An empty answer counts as none, so a blank line never satisfies `required`.
        toml::Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn save(key: &str, value: &str) -> Result<()> {
    let mut table = read_answers();
    let vars = table
        .entry("vars")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let Some(vars) = vars.as_table_mut() {
        vars.insert(key.to_string(), toml::Value::String(value.to_string()));
    }
    let path = answers_path();
    if let Some(parent) = path.parent() {
        crate::file::create_dir_all(parent)?;
    }
    write_private(&path, toml::to_string(&table)?.as_bytes())
        .wrap_err_with(|| format!("failed to save the answer for var '{key}'"))
}

/// Write `contents` to `path` through a sibling file that is owner-only from
/// creation, so the answers are never readable by others, even briefly.
#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    drop(file);
    let renamed = written.and_then(|()| std::fs::rename(&tmp, path));
    if renamed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(renamed?)
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    crate::file::write_atomic(path, contents)
}

/// The saved answer for `key`, or one asked for and saved now.
///
/// Returns `None` when nothing is saved and this run may not ask: prompting is
/// not enabled, or stderr is not a terminal. `default` is offered to the user,
/// and Enter accepts it.
pub(crate) fn answer(key: &str, prompt: &str, default: Option<&str>) -> Result<Option<String>> {
    if let Some(value) = saved(key) {
        return Ok(Some(value));
    }
    if !ENABLED.load(Ordering::Relaxed) || !console::user_attended_stderr() {
        return Ok(None);
    }
    let theme = crate::ui::theme::get_theme();
    let mut input = Input::new(prompt).theme(&theme);
    if let Some(default) = default {
        input = input.placeholder(default);
    }
    let mut value = input.run()?;
    if value.is_empty()
        && let Some(default) = default
    {
        value = default.to_string();
    }
    // Nothing to offer and nothing typed: leave the var unanswered rather than
    // saving a blank that `required` would accept.
    if value.is_empty() {
        return Ok(None);
    }
    save(key, &value)?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unanswered_var_without_enabled_prompt_is_none() {
        assert_eq!(
            answer("mise_test_unanswered_var", "Name", None).unwrap(),
            None
        );
    }
}
