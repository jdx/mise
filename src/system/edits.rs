//! `[dotfiles]` — declarative edits to files mise doesn't own,
//! applied by `mise dotfiles apply` or `mise bootstrap`.
//!
//! Where whole-file dotfile entries manage whole files, an edit owns one small piece
//! of a file something else owns — the `mise activate` line in a shell rc,
//! an entry in /etc/hosts. Entries are keyed by target path plus an id naming
//! each edit within the file:
//!
//! ```toml
//! [dotfiles]
//! "~/.zshrc/activate" = { block = 'eval "$(mise activate zsh)"' }
//! "~/.zshrc/aliases" = { source = "snippets/aliases.sh", template = "tera" }
//! "/etc/hosts/dev" = { line = "127.0.0.1 dev.local" }
//! ```
//!
//! A `block` is delimited by marker comments in the target file —
//! `# >>> mise:activate >>>` / `# <<< mise:activate <<<` — which double as
//! the ownership record: apply replaces only what's between them, so the
//! design stays stateless like the rest of `[dotfiles]`. A `line` ensures an
//! exact line exists, appending it if absent.
//!
//! Entries merge across the config hierarchy as a union keyed by
//! `(path, id)` — a more local config overrides an edit with the same id,
//! exactly like whole-file entries override by target.

use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::{Config, ConfigMap};
use crate::file;
use crate::path::PathExt;
use crate::system::files::FileState;
use crate::ui::prompt;

/// one `[dotfiles]` edit entry as written in mise.toml. Operations stay loosely typed so configs using operations
/// from newer mise versions still parse (entries with no recognized
/// operation warn and are skipped)
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EditTomlEntry {
    /// `activate = 'eval "$(mise activate zsh)"'` — inline block content
    Block(String),
    /// `aliases = { source = "...", template = "tera" }` /
    /// `dev = { line = "..." }`
    Table(EditTomlTable),
}

#[derive(Debug, Clone, Deserialize)]
pub struct EditTomlTable {
    /// inline block content
    #[serde(default)]
    pub block: Option<String>,
    /// block content from a file (relative to the declaring config file)
    #[serde(default)]
    pub source: Option<String>,
    /// template engine to render the block content with; currently only
    /// `"tera"` (string-typed so engines from newer mise versions warn and
    /// skip instead of failing to parse)
    #[serde(default)]
    pub template: Option<String>,
    /// exact line to ensure exists
    #[serde(default)]
    pub line: Option<String>,
    /// comment prefix for the markers; inferred from the file extension
    /// when omitted
    #[serde(default)]
    pub comment: Option<String>,
}

/// where a block's content comes from
#[derive(Debug, Clone)]
pub enum BlockSource {
    Inline(String),
    /// absolute path, resolved against the declaring config file
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub enum EditOp {
    Block {
        source: BlockSource,
        template: bool,
        comment: String,
    },
    Line {
        line: String,
    },
}

/// one edit, resolved against the config file that declared it
#[derive(Debug, Clone)]
pub struct EditRequest {
    /// target path as written in config (display)
    pub path_raw: String,
    /// absolute target path (`~` expanded)
    pub path: PathBuf,
    /// the entry's key within its file: merge identity and, for blocks, the
    /// marker name
    pub id: String,
    pub op: EditOp,
    /// directory of the declaring config file — base dir for relative
    /// sources and template functions like `exec` and `read_file`
    pub base: PathBuf,
    /// config file that declared this edit
    pub config_path: PathBuf,
}

impl EditRequest {
    /// short operation label for status tables and dry-run output
    pub fn describe_op(&self) -> String {
        match &self.op {
            EditOp::Block { .. } => format!("block:{}", self.id),
            EditOp::Line { .. } => format!("line:{}", self.id),
        }
    }

    pub fn config_key(&self) -> String {
        format!("{}/{}", self.path_raw.trim_end_matches('/'), self.id)
    }
}

pub fn matches_target(req: &EditRequest, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == &req.path_raw
                || filter == &req.config_key()
                || filter == &format!("{}/{}", req.path.display_user(), req.id)
                || filter.rsplit_once('/').is_some_and(|(path, id)| {
                    id == req.id && {
                        let resolved = crate::system::files::resolve_target_arg(path);
                        resolved == req.path
                    }
                })
                || {
                    let resolved = crate::system::files::resolve_target_arg(filter);
                    resolved == req.path
                }
        })
}

/// Aggregate edit `[dotfiles]` entries across all loaded config files. Entries
/// union global -> local, keyed by `(path, id)`; a more local config overrides
/// an edit with the same id. Malformed entries warn and are skipped.
pub fn edits_from_config(config: &Config) -> Vec<EditRequest> {
    edits_from_config_files(&config.config_files)
}

pub fn edits_from_config_files(config_files: &ConfigMap) -> Vec<EditRequest> {
    let mut merged: IndexMap<String, EditRequest> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for (cf_path, cf) in config_files.iter().rev() {
        let base = cf_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let Some(dotfiles) = cf.dotfiles_config() else {
            continue;
        };
        for (path_and_id, value) in dotfiles.0 {
            let Some(entry) = edit_entry_from_toml(&path_and_id, value) else {
                continue;
            };
            match split_edit_key(&path_and_id) {
                Some((path_raw, id)) => match resolve_entry(&path_raw, id, entry, &base, cf_path) {
                    Ok(req) => {
                        merged.insert(format!("{}\u{0}{}", req.path.display(), req.id), req);
                    }
                    Err(err) => warn!("[dotfiles]: {err}"),
                },
                None => warn!(
                    "[dotfiles].\"{path_and_id}\": edit entries must end with an id path segment"
                ),
            }
        }
    }
    merged.into_values().collect()
}

fn edit_entry_from_toml(path_and_id: &str, value: toml::Value) -> Option<EditTomlEntry> {
    match &value {
        toml::Value::Table(table) => {
            let is_whole_file_table = table.is_empty()
                || table.contains_key("mode")
                || table.contains_key("source")
                    && !table.contains_key("block")
                    && !table.contains_key("line")
                    && !table.contains_key("template")
                    && !table.contains_key("comment");
            if is_whole_file_table {
                return None;
            }
        }
        _ => return None,
    }
    match value.try_into() {
        Ok(entry) => Some(entry),
        Err(err) => {
            warn!("[dotfiles].\"{path_and_id}\": invalid edit entry: {err}");
            None
        }
    }
}

fn split_edit_key(path_and_id: &str) -> Option<(String, String)> {
    let (path, id) = path_and_id.rsplit_once('/')?;
    if path.is_empty() || path == "~" || path == "/" || id.is_empty() {
        return None;
    }
    Some((path.to_string(), id.to_string()))
}

fn resolve_entry(
    path_raw: &str,
    id: String,
    entry: EditTomlEntry,
    base: &Path,
    config_path: &Path,
) -> Result<EditRequest> {
    let path = file::replace_path(path_raw);
    if path.is_relative() {
        bail!("path \"{path_raw}\" must be absolute or start with ~/, ignoring entry");
    }
    // ids end up inside marker lines — keep them to characters that can't
    // collide with the marker syntax itself
    if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || "_-.".contains(c)) {
        bail!(
            "\"{path_raw}\".{id:?}: ids may only contain letters, digits, '_', '-', and '.', ignoring entry"
        );
    }
    let entry = match entry {
        EditTomlEntry::Block(inline) => EditTomlTable {
            block: Some(inline),
            source: None,
            template: None,
            line: None,
            comment: None,
        },
        EditTomlEntry::Table(table) => table,
    };
    let is_block = entry.block.is_some() || entry.source.is_some();
    let op = match (&is_block, &entry.line) {
        (true, Some(_)) => {
            bail!(
                "\"{path_raw}\".{id}: block/source and line are mutually exclusive, ignoring entry"
            )
        }
        (false, None) => {
            bail!(
                "\"{path_raw}\".{id}: no recognized operation (block, source, or line), ignoring entry"
            )
        }
        (true, None) => {
            let source = match (entry.block, entry.source) {
                (Some(_), Some(_)) => {
                    bail!(
                        "\"{path_raw}\".{id}: block and source are mutually exclusive, ignoring entry"
                    )
                }
                (Some(inline), None) => BlockSource::Inline(inline),
                (None, Some(src)) => {
                    let src = file::replace_path(&src);
                    BlockSource::File(if src.is_relative() {
                        base.join(src)
                    } else {
                        src
                    })
                }
                (None, None) => unreachable!("is_block"),
            };
            let template = match entry.template.as_deref() {
                None => false,
                Some("tera") => true,
                Some(other) => {
                    bail!(
                        "\"{path_raw}\".{id}: unknown template engine '{other}' (expected \"tera\"), ignoring entry"
                    )
                }
            };
            let comment = entry
                .comment
                .unwrap_or_else(|| infer_comment(&path).to_string());
            EditOp::Block {
                source,
                template,
                comment,
            }
        }
        (false, Some(line)) => {
            // a "line" is matched against the file's individual lines, so an
            // embedded newline could never converge — use a block for
            // multi-line content
            if line.contains('\n') {
                bail!(
                    "\"{path_raw}\".{id}: line may not contain a newline; use a block for multi-line content, ignoring entry"
                )
            }
            EditOp::Line { line: line.clone() }
        }
    };
    Ok(EditRequest {
        path_raw: path_raw.to_string(),
        path,
        id,
        op,
        base: base.to_path_buf(),
        config_path: config_path.to_path_buf(),
    })
}

/// comment prefix for marker lines, by file extension; `#` covers most
/// config and shell files (and extensionless files like `.zshrc`, `hosts`)
fn infer_comment(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "lua" => "--",
        "vim" | "vimrc" => "\"",
        "el" | "lisp" | "scm" => ";;",
        "ini" | "reg" => ";",
        "c" | "h" | "cpp" | "hpp" | "cc" | "js" | "ts" | "jsx" | "tsx" | "rs" | "go" | "java"
        | "kt" | "swift" | "cs" | "scala" | "php" | "zig" => "//",
        _ => "#",
    }
}

fn begin_marker(comment: &str, id: &str) -> String {
    format!("{comment} >>> mise:{id} >>> managed by mise — do not edit between markers")
}

fn end_marker(comment: &str, id: &str) -> String {
    format!("{comment} <<< mise:{id} <<<")
}

/// a line is a marker only when the pattern sits at the start of the line,
/// preceded by either the configured comment token or at most a short
/// generic one (`# `, `// `, `-- `, `<!-- `) — content that merely
/// *mentions* a marker (`echo ">>> mise:x >>>"`, docs) must not count as one
fn is_marker_line(line: &str, pat: &str, comment: &str) -> bool {
    let trimmed = line.trim_start();
    match trimmed.find(pat) {
        Some(idx) => {
            let prefix = trimmed[..idx].trim();
            // the configured comment always counts, however exotic (`REM`),
            // so markers we write are always markers we can find again
            prefix == comment || (prefix.len() <= 8 && !prefix.chars().any(|c| c.is_alphanumeric()))
        }
        None => false,
    }
}

/// locate an id's marker pair in the file's lines: Ok(None) = no markers,
/// Ok(Some((begin, end))) = line indexes, Err = corrupted markers
fn find_block(
    lines: &[&str],
    id: &str,
    comment: &str,
) -> std::result::Result<Option<(usize, usize)>, String> {
    let begin_pat = format!(">>> mise:{id} >>>");
    let end_pat = format!("<<< mise:{id} <<<");
    let begins: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_marker_line(l, &begin_pat, comment))
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_marker_line(l, &end_pat, comment))
        .map(|(i, _)| i)
        .collect();
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([b], [e]) if b < e => Ok(Some((*b, *e))),
        ([_], [_]) => Err("end marker appears before begin marker".into()),
        ([], _) => Err("end marker without begin marker".into()),
        (_, []) => Err("begin marker without end marker".into()),
        _ => Err("duplicate markers".into()),
    }
}

/// the content a block should contain, resolved and rendered at most once
/// per check/apply cycle (templates may use exec())
fn desired_content(config: &Config, req: &EditRequest) -> Result<Option<String>> {
    let EditOp::Block {
        source,
        template,
        comment,
    } = &req.op
    else {
        return Ok(None);
    };
    let id = &req.id;
    let raw = match source {
        BlockSource::Inline(s) => s.clone(),
        BlockSource::File(p) => file::read_to_string(p)?,
    };
    let content = if *template {
        let mut tera = crate::tera::get_tera(Some(&req.base));
        tera.render_str(&raw, &config.tera_ctx).map_err(|err| {
            eyre::eyre!(
                "[dotfiles].\"{}/{}\": failed to render template: {err}",
                req.path_raw,
                req.id
            )
        })?
    } else {
        raw
    };
    let content = content.trim_end_matches('\n').to_string();
    // a block containing its own marker lines would write a file that can't
    // be parsed back — refuse up front instead of corrupting on reapply
    for pat in [format!(">>> mise:{id} >>>"), format!("<<< mise:{id} <<<")] {
        if content.lines().any(|l| is_marker_line(l, &pat, comment)) {
            bail!(
                "[dotfiles].\"{}/{}\": block content may not contain its own marker lines",
                req.path_raw,
                req.id
            );
        }
    }
    Ok(Some(content))
}

/// Current state of one edit on this machine.
///
/// Note: comparing a template block against existing markers requires
/// rendering it, so this can run the template engine — including `exec()` —
/// from `mise dotfiles status`. That's the same trust model as `[env]`
/// templates. Rendering only happens once every render-free outcome (symlink
/// target, missing file, absent or corrupted markers) has been ruled out,
/// and `--dry-run` skips template rendering entirely (see [`apply`]).
pub fn check(config: &Config, req: &EditRequest) -> Result<FileState> {
    if let EditOp::Block {
        source: BlockSource::File(p),
        ..
    } = &req.op
        && !p.exists()
    {
        return Ok(FileState::SourceMissing);
    }
    match precheck(req)? {
        Some(EditCheck::State(state)) => Ok(state),
        Some(EditCheck::Blocked(reason)) => Ok(FileState::Differs(reason)),
        None => {
            let desired = desired_content(config, req)?;
            block_state(req, desired.as_deref())
        }
    }
}

const SYMLINK_REASON: &str = "target is a symlink; edit the real file instead";

/// outcome of inspecting one edit: an ordinary state, or a condition mise
/// refuses to apply automatically (corrupted markers, symlink target)
enum EditCheck {
    State(FileState),
    Blocked(String),
}

/// everything that can be decided without rendered content: symlink targets,
/// file existence, marker integrity, and line presence. Returns Ok(None)
/// when the entry's markers exist and a content comparison (which may
/// require rendering) is still needed — callers must not render before this
/// has been consulted, so blocked entries never execute template code
fn precheck(req: &EditRequest) -> Result<Option<EditCheck>> {
    // edits write through symlinks into whatever they point at (often a
    // dotfile source) — surface that instead of silently doing it
    if req.path.is_symlink() {
        return Ok(Some(EditCheck::Blocked(SYMLINK_REASON.into())));
    }
    if !req.path.exists() {
        return Ok(Some(EditCheck::State(FileState::Missing)));
    }
    let text = file::read_to_string(&req.path)?;
    let lines: Vec<&str> = text.lines().collect();
    match &req.op {
        EditOp::Block { comment, .. } => match find_block(&lines, &req.id, comment) {
            Err(reason) => Ok(Some(EditCheck::Blocked(reason))),
            Ok(None) => Ok(Some(EditCheck::State(FileState::Missing))),
            Ok(Some(_)) => Ok(None),
        },
        EditOp::Line { line } => Ok(Some(EditCheck::State(if lines.contains(&line.as_str()) {
            FileState::Applied
        } else {
            FileState::Missing
        }))),
    }
}

/// content comparison for a block whose markers exist ([`precheck`]
/// returned None)
fn block_state(req: &EditRequest, desired: Option<&str>) -> Result<FileState> {
    let EditOp::Block { comment, .. } = &req.op else {
        unreachable!("only blocks reach a content comparison");
    };
    let id = &req.id;
    let text = file::read_to_string(&req.path)?;
    let lines: Vec<&str> = text.lines().collect();
    match find_block(&lines, id, comment) {
        Ok(Some((begin, end))) => {
            let current = lines[begin + 1..end].join("\n");
            if current == desired.expect("resolved block content") {
                Ok(FileState::Applied)
            } else {
                Ok(FileState::Differs("block content differs".into()))
            }
        }
        // precheck just vetted the markers; a race is a plain differs
        _ => Ok(FileState::Differs("markers changed during check".into())),
    }
}

pub struct ApplyOpts {
    pub dry_run: bool,
    pub verbose: bool,
    pub yes: bool,
}

/// Apply all edits that aren't already in the desired state. Edits never
/// replace files, so there is no --force here — but corrupted markers and
/// symlink targets are reported as errors rather than guessed at.
pub fn apply(config: &Config, requests: &[EditRequest], opts: &ApplyOpts) -> Result<()> {
    let mut todo: Vec<(&EditRequest, Option<String>)> = vec![];
    let mut problems = vec![];
    for req in requests {
        if let EditOp::Block {
            source: BlockSource::File(p),
            ..
        } = &req.op
            && !p.exists()
        {
            problems.push(format!(
                "  \"{}\" ({}): source does not exist: {}",
                req.path_raw,
                req.describe_op(),
                p.display_user()
            ));
            continue;
        }
        // a render or check failure on one entry must not hide problems
        // with the others — keep evaluating, like status does. Render-free
        // outcomes (symlink target, marker integrity, line presence) are
        // decided first so blocked or already-applied entries never execute
        // template code
        let pre = match precheck(req) {
            Ok(pre) => pre,
            Err(err) => {
                problems.push(format!(
                    "  \"{}\" ({}): {err}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
        };
        match &pre {
            Some(EditCheck::Blocked(reason)) => {
                problems.push(format!(
                    "  \"{}\" ({}): {reason}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
            Some(EditCheck::State(FileState::Applied)) => continue,
            _ => {}
        }
        // rendering can run exec() — a dry run must not execute anything,
        // so template blocks are listed without computing their content
        // (same policy as template file entries)
        if opts.dry_run && matches!(&req.op, EditOp::Block { template: true, .. }) {
            todo.push((req, None));
            continue;
        }
        let desired = match desired_content(config, req) {
            Ok(desired) => desired,
            // already carries the entry's context
            Err(err) => {
                problems.push(format!("  {err}"));
                continue;
            }
        };
        match pre {
            // markers exist: compare content to see if anything would change
            None => match block_state(req, desired.as_deref()) {
                Ok(FileState::Applied) => continue,
                Ok(_) => todo.push((req, desired)),
                Err(err) => {
                    problems.push(format!(
                        "  \"{}\" ({}): {err}",
                        req.path_raw,
                        req.describe_op()
                    ));
                    continue;
                }
            },
            // missing — needs applying
            Some(_) => todo.push((req, desired)),
        }
    }
    if !problems.is_empty() {
        bail!(
            "edits: cannot apply these entries, fix them manually:\n{}",
            problems.join("\n")
        );
    }
    if todo.is_empty() {
        info!("edits: all edits are applied");
        return Ok(());
    }
    if opts.dry_run {
        for (req, desired) in &todo {
            // template state wasn't computed (no rendering on dry runs), so
            // the entry may already be converged
            let conditional =
                desired.is_none() && matches!(&req.op, EditOp::Block { template: true, .. });
            let suffix = if conditional { " (if changed)" } else { "" };
            miseprintln!(
                "edit {} ({}){suffix}",
                req.path.display_user(),
                req.describe_op()
            );
            if opts.verbose && !conditional {
                miseprintln!("  desired {}", req.describe_op());
            }
        }
        return Ok(());
    }
    if !opts.yes && console::user_attended_stderr() {
        let list = todo
            .iter()
            .map(|(r, _)| format!("{} ({})", r.path_raw, r.describe_op()))
            .collect::<Vec<_>>()
            .join(", ");
        if !prompt::confirm(format!("edits: apply {list}?"))? {
            info!("edits: skipped");
            return Ok(());
        }
    }
    for (req, desired) in &todo {
        apply_one(req, desired.as_deref())?;
    }
    info!(
        "edits: applied {}",
        todo.iter()
            .map(|(r, _)| format!("{} ({})", r.path_raw, r.describe_op()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

/// Simulate applying an edit to in-memory text for bootstrap dry-run config
/// discovery. Template edits are intentionally not rendered during dry-runs
/// because rendering may execute user commands.
pub fn apply_dry_run_to_string(
    config: &Config,
    req: &EditRequest,
    text: &str,
) -> Result<Option<String>> {
    if matches!(&req.op, EditOp::Block { template: true, .. }) {
        return Ok(None);
    }
    let desired = desired_content(config, req)?;
    apply_to_string(req, desired.as_deref(), text).map(Some)
}

fn apply_one(req: &EditRequest, desired: Option<&str>) -> Result<()> {
    debug!("edits: {} ({})", req.path.display_user(), req.describe_op());
    if let Some(parent) = req.path.parent() {
        file::create_dir_all(parent)?;
    }
    let text = if req.path.exists() {
        file::read_to_string(&req.path)?
    } else {
        String::new()
    };
    let out = apply_to_string(req, desired, &text)?;
    // file::write truncates in place, preserving the file's permissions
    file::write(&req.path, &out)?;
    Ok(())
}

fn apply_to_string(req: &EditRequest, desired: Option<&str>, text: &str) -> Result<String> {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    match &req.op {
        EditOp::Block { comment, .. } => {
            let id = &req.id;
            let desired = desired.expect("resolved block content");
            let mut block = vec![begin_marker(comment, id)];
            // a desired of "" means an empty block, not a blank line
            if !desired.is_empty() {
                block.extend(desired.lines().map(|l| l.to_string()));
            }
            block.push(end_marker(comment, id));
            match find_block(
                &lines.iter().map(|l| l.as_str()).collect::<Vec<_>>(),
                id,
                comment,
            ) {
                // markers are rewritten too, so a changed comment style or
                // marker wording converges on reapply
                Ok(Some((begin, end))) => {
                    lines.splice(begin..=end, block);
                }
                Ok(None) => lines.extend(block),
                Err(reason) => bail!(
                    "edits: \"{}\": {reason}, fix the file manually",
                    req.path_raw
                ),
            }
        }
        EditOp::Line { line } => {
            // an earlier entry in the same batch may have just written an
            // identical line (two ids, same text) — stay idempotent against
            // the file's current content, not the state at plan time
            if !lines.iter().any(|l| l == line) {
                lines.push(line.clone());
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_comment() {
        assert_eq!(infer_comment(Path::new("/a/.zshrc")), "#");
        assert_eq!(infer_comment(Path::new("/etc/hosts")), "#");
        assert_eq!(infer_comment(Path::new("/a/init.lua")), "--");
        assert_eq!(infer_comment(Path::new("/a/foo.rs")), "//");
        assert_eq!(infer_comment(Path::new("/a/foo.ini")), ";");
    }

    #[test]
    fn test_find_block() {
        let lines = vec![
            "before",
            "# >>> mise:a >>> managed by mise",
            "content",
            "# <<< mise:a <<<",
            "after",
        ];
        assert_eq!(find_block(&lines, "a", "#"), Ok(Some((1, 3))));
        assert_eq!(find_block(&lines, "b", "#"), Ok(None));
        // ids are delimited — "a" must not match "ab"
        let lines = vec!["# >>> mise:ab >>>", "# <<< mise:ab <<<"];
        assert_eq!(find_block(&lines, "a", "#"), Ok(None));
        // content that mentions a marker mid-line is not a marker
        let lines = vec![
            "# >>> mise:a >>>",
            r#"echo "keep the >>> mise:a >>> line intact""#,
            "# <<< mise:a <<<",
        ];
        assert_eq!(find_block(&lines, "a", "#"), Ok(Some((0, 2))));
        // ...but indented comment markers still count
        let lines = vec!["  # >>> mise:a >>>", "  # <<< mise:a <<<"];
        assert_eq!(find_block(&lines, "a", "#"), Ok(Some((0, 1))));
        let lines = vec!["<!-- >>> mise:a >>>", "<!-- <<< mise:a <<<"];
        assert_eq!(find_block(&lines, "a", "#"), Ok(Some((0, 1))));
        let lines = vec!["# >>> mise:a >>>"];
        assert!(find_block(&lines, "a", "#").is_err());
        let lines = vec!["# <<< mise:a <<<", "# >>> mise:a >>>"];
        assert!(find_block(&lines, "a", "#").is_err());
        // an exotic configured comment token (alphanumeric, like batch REM)
        // is always recognized, so written markers can be found again
        let lines = vec!["REM >>> mise:a >>>", "REM <<< mise:a <<<"];
        assert_eq!(find_block(&lines, "a", "REM"), Ok(Some((0, 1))));
        assert_eq!(find_block(&lines, "a", "#"), Ok(None));
    }
}
