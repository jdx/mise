//! Per-machine answers for `[vars]` entries that declare a `prompt`.
//!
//! An answer lives in `$MISE_STATE_DIR/vars.toml`, outside every config file, so
//! it never reaches a dotfiles repository. Reading an answer never prompts;
//! only a command that called [`enable`] asks, and only on a terminal.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use demand::Input;
use eyre::{Result, WrapErr, bail};

use crate::dirs;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Held while asking, so concurrent resolutions never interleave prompts.
static ASKING: Mutex<()> = Mutex::new(());
/// When set, only these vars may be asked for.
static ONLY: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
/// Every var seen declaring a `prompt` while config resolved.
static DECLARED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Vars that were asked for and left blank.
static BLANK: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Whether a missing `required` var should warn instead of failing config load.
static TOLERATE_MISSING: AtomicBool = AtomicBool::new(false);
/// Vars answered by this process, in order.
static ANSWERED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Let this process ask for unanswered `prompt` vars. Without it, an
/// unanswered var falls back to its `default`, or to `required`'s error.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Let config load with unanswered `required` vars, warning about them. A
/// command that asks for only some vars would otherwise fail on the others.
pub fn tolerate_missing() {
    TOLERATE_MISSING.store(true, Ordering::Relaxed);
}

pub(crate) fn tolerates_missing() -> bool {
    TOLERATE_MISSING.load(Ordering::Relaxed)
}

/// Record that `key` declares a `prompt`.
pub(crate) fn note_declared(key: &str) {
    if let Ok(mut declared) = DECLARED.lock() {
        declared.insert(key.to_string());
    }
}

/// Vars this process asked for and got a blank answer to, with no default to fall back on.
pub fn left_blank() -> BTreeSet<String> {
    BLANK.lock().map(|b| b.clone()).unwrap_or_default()
}

/// Every var that declared a `prompt` while config resolved.
pub fn declared() -> BTreeSet<String> {
    DECLARED.lock().map(|d| d.clone()).unwrap_or_default()
}

/// Stop asking. Called once the config has loaded, so later var resolution
/// (per task, possibly in parallel) uses saved answers and never touches the terminal.
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Limit prompting to `names`. An empty list leaves it unrestricted.
pub fn only(names: &[String]) {
    if let Ok(mut only) = ONLY.lock() {
        *only = (!names.is_empty()).then(|| names.iter().cloned().collect());
    }
}

/// The vars this process asked for and saved, in order.
pub fn answered() -> Vec<String> {
    ANSWERED.lock().map(|a| a.clone()).unwrap_or_default()
}

fn may_ask(key: &str) -> bool {
    ONLY.lock()
        .map(|only| only.as_ref().is_none_or(|names| names.contains(key)))
        .unwrap_or(false)
}

/// The file that holds the saved answers.
pub fn answers_path() -> PathBuf {
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
pub fn saved(key: &str) -> Option<String> {
    match read_answers().get("vars")?.as_table()?.get(key)? {
        // An empty answer counts as none, so a blank line never satisfies `required`.
        toml::Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Apply `change` to the saved answers while holding the lock, so a
/// concurrent save of another var is not overwritten by a stale table.
fn update<T>(what: &str, change: impl FnOnce(&mut toml::Table) -> T) -> Result<T> {
    let path = answers_path();
    if let Some(parent) = path.parent() {
        crate::file::create_dir_all(parent)?;
    }
    let mut lock = fslock::LockFile::open(&path.with_extension("toml.lock"))?;
    lock.lock()?;
    let mut table = read_answers();
    let vars = table
        .entry("vars")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let result = match vars.as_table_mut() {
        Some(vars) => change(vars),
        None => bail!("{} has a non-table `vars` entry", path.display()),
    };
    write_private(&path, toml::to_string(&table)?.as_bytes())
        .wrap_err_with(|| format!("failed to {what}"))?;
    Ok(result)
}

fn save(key: &str, value: &str) -> Result<()> {
    update(&format!("save the answer for var '{key}'"), |vars| {
        vars.insert(key.to_string(), toml::Value::String(value.to_string()));
    })?;
    if let Ok(mut answered) = ANSWERED.lock() {
        answered.push(key.to_string());
    }
    Ok(())
}

/// Every saved answer, sorted by name.
pub fn saved_all() -> Vec<(String, String)> {
    let table = read_answers();
    let Some(vars) = table.get("vars").and_then(|v| v.as_table()) else {
        return vec![];
    };
    let mut all: Vec<_> = vars
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
        .collect();
    all.sort();
    all
}

/// Save `value` as the answer for `key` without asking.
pub fn set(key: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!(
            "an empty answer for '{key}' would count as unanswered; use `mise vars unset {key}` to forget it"
        );
    }
    update(&format!("save the answer for var '{key}'"), |vars| {
        vars.insert(key.to_string(), toml::Value::String(value.to_string()));
    })
}

/// Forget the saved answer for `key`. Returns whether there was one.
pub fn remove(key: &str) -> Result<bool> {
    update(&format!("remove the answer for var '{key}'"), |vars| {
        vars.remove(key).is_some()
    })
}

/// Write `contents` to `path` through a sibling file that is owner-only from
/// creation, so the answers are never readable by others, even briefly.
#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let tmp = path.with_extension(format!(
        "toml.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
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
    if !ENABLED.load(Ordering::Relaxed) || !may_ask(key) || !console::user_attended_stderr() {
        return Ok(None);
    }
    // One prompt at a time; whoever waited may find the answer already saved.
    let _asking = ASKING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(value) = saved(key) {
        return Ok(Some(value));
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
        if let Ok(mut blank) = BLANK.lock() {
            blank.insert(key.to_string());
        }
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
