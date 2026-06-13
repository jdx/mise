//! `[[system.edits]]` — declarative edits to files mise doesn't own,
//! applied by `mise system install` or `mise bootstrap`.
//!
//! Where `[system.files]` manages whole files, an edit owns one small piece
//! of a file something else owns — the `mise activate` line in a shell rc,
//! an entry in /etc/hosts:
//!
//! ```toml
//! [[system.edits]]
//! path = "~/.zshrc"
//! id = "activate"                        # marker identity, default "mise"
//! block = 'eval "$(mise activate zsh)"'  # or source = "...", template = true
//!
//! [[system.edits]]
//! path = "/etc/hosts"
//! line = "127.0.0.1 dev.local"
//! ```
//!
//! A `block` is delimited by marker comments in the target file —
//! `# >>> mise:activate >>>` / `# <<< mise:activate <<<` — which double as
//! the ownership record: apply replaces only what's between them, so the
//! design stays stateless like the rest of `[system]`. A `line` ensures an
//! exact line exists, appending it if absent.
//!
//! Entries merge across the config hierarchy as a union keyed by
//! `(path, id)` for blocks and `(path, line)` for lines; a more local config
//! overrides a block with the same id.

use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::Config;
use crate::file;
use crate::path::PathExt;
use crate::system::files::FileState;
use crate::ui::prompt;

/// one `[[system.edits]]` entry as written in mise.toml. Operations stay
/// loosely typed so configs using operations from newer mise versions still
/// parse (entries with no recognized operation warn and are skipped)
#[derive(Debug, Clone, Deserialize)]
pub struct EditTomlEntry {
    pub path: String,
    /// marker identity for blocks; default "mise"
    #[serde(default)]
    pub id: Option<String>,
    /// inline block content
    #[serde(default)]
    pub block: Option<String>,
    /// block content from a file (relative to the declaring config file)
    #[serde(default)]
    pub source: Option<String>,
    /// render the block content through the mise template engine
    #[serde(default)]
    pub template: Option<bool>,
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
        id: String,
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
    pub op: EditOp,
    /// directory of the declaring config file — base dir for relative
    /// sources and template functions like `exec` and `read_file`
    pub base: PathBuf,
}

impl EditRequest {
    /// short operation label for status tables and dry-run output
    pub fn describe_op(&self) -> String {
        match &self.op {
            EditOp::Block { id, .. } => format!("block:{id}"),
            EditOp::Line { line } => format!("line:{line}"),
        }
    }
}

/// Aggregate `[[system.edits]]` across all loaded config files. Entries
/// union global -> local, keyed by `(path, id)` for blocks and
/// `(path, line)` for lines; a more local config overrides a block with the
/// same id. Malformed entries warn and are skipped.
pub fn edits_from_config(config: &Config) -> Vec<EditRequest> {
    let mut merged: IndexMap<String, EditRequest> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for (cf_path, cf) in config.config_files.iter().rev() {
        let Some(sys) = cf.system_config() else {
            continue;
        };
        let base = cf_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        for entry in sys.edits {
            match resolve_entry(entry, &base) {
                Ok((key, req)) => {
                    merged.insert(key, req);
                }
                Err(err) => warn!("[[system.edits]]: {err}"),
            }
        }
    }
    merged.into_values().collect()
}

fn resolve_entry(entry: EditTomlEntry, base: &Path) -> Result<(String, EditRequest)> {
    let path_raw = entry.path.clone();
    let path = file::replace_path(&path_raw);
    if path.is_relative() {
        bail!("path \"{path_raw}\" must be absolute or start with ~/, ignoring entry");
    }
    let is_block = entry.block.is_some() || entry.source.is_some();
    let (key, op) = match (&is_block, &entry.line) {
        (true, Some(_)) => {
            bail!("\"{path_raw}\": block/source and line are mutually exclusive, ignoring entry")
        }
        (false, None) => {
            bail!(
                "\"{path_raw}\": no recognized operation (block, source, or line), ignoring entry"
            )
        }
        (true, None) => {
            let source = match (entry.block, entry.source) {
                (Some(_), Some(_)) => {
                    bail!("\"{path_raw}\": block and source are mutually exclusive, ignoring entry")
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
            let id = entry.id.unwrap_or_else(|| "mise".to_string());
            let comment = entry
                .comment
                .unwrap_or_else(|| infer_comment(&path).to_string());
            (
                format!("{path_raw}\u{0}block:{id}"),
                EditOp::Block {
                    id,
                    source,
                    template: entry.template.unwrap_or(false),
                    comment,
                },
            )
        }
        (false, Some(line)) => (
            format!("{path_raw}\u{0}line:{line}"),
            EditOp::Line { line: line.clone() },
        ),
    };
    Ok((
        key,
        EditRequest {
            path_raw,
            path,
            op,
            base: base.to_path_buf(),
        },
    ))
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
/// preceded by at most a short comment token (`# `, `// `, `-- `, `<!-- `) —
/// content that merely *mentions* a marker (`echo ">>> mise:x >>>"`, docs)
/// must not count as one
fn is_marker_line(line: &str, pat: &str) -> bool {
    let trimmed = line.trim_start();
    match trimmed.find(pat) {
        Some(idx) => {
            let prefix = &trimmed[..idx];
            prefix.len() <= 8 && !prefix.chars().any(|c| c.is_alphanumeric())
        }
        None => false,
    }
}

/// locate an id's marker pair in the file's lines: Ok(None) = no markers,
/// Ok(Some((begin, end))) = line indexes, Err = corrupted markers
fn find_block(lines: &[&str], id: &str) -> std::result::Result<Option<(usize, usize)>, String> {
    let begin_pat = format!(">>> mise:{id} >>>");
    let end_pat = format!("<<< mise:{id} <<<");
    let begins: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_marker_line(l, &begin_pat))
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_marker_line(l, &end_pat))
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
        id,
        source,
        template,
        ..
    } = &req.op
    else {
        return Ok(None);
    };
    let raw = match source {
        BlockSource::Inline(s) => s.clone(),
        BlockSource::File(p) => file::read_to_string(p)?,
    };
    let content = if *template {
        let mut tera = crate::tera::get_tera(Some(&req.base));
        tera.render_str(&raw, &config.tera_ctx).map_err(|err| {
            eyre::eyre!(
                "[[system.edits]] \"{}\": failed to render template: {err}",
                req.path_raw
            )
        })?
    } else {
        raw
    };
    let content = content.trim_end_matches('\n').to_string();
    // a block containing its own marker lines would write a file that can't
    // be parsed back — refuse up front instead of corrupting on reapply
    for pat in [format!(">>> mise:{id} >>>"), format!("<<< mise:{id} <<<")] {
        if content.lines().any(|l| is_marker_line(l, &pat)) {
            bail!(
                "[[system.edits]] \"{}\": block content may not contain its own marker lines",
                req.path_raw
            );
        }
    }
    Ok(Some(content))
}

/// Current state of one edit on this machine.
///
/// Note: computing a template block's state requires rendering it, so this
/// runs the template engine — including `exec()` — from `mise system
/// status`. That's the same trust model as `[env]` templates; only
/// `--dry-run` promises to execute nothing and therefore skips template
/// checks entirely (see [`apply`]).
pub fn check(config: &Config, req: &EditRequest) -> Result<FileState> {
    if let EditOp::Block {
        source: BlockSource::File(p),
        ..
    } = &req.op
        && !p.exists()
    {
        return Ok(FileState::SourceMissing);
    }
    let desired = desired_content(config, req)?;
    check_resolved(req, desired.as_deref())
}

/// [`check`] with block content already resolved, so callers that go on to
/// write the file resolve and render only once
fn check_resolved(req: &EditRequest, desired: Option<&str>) -> Result<FileState> {
    // edits write through symlinks into whatever they point at (often a
    // [system.files] source) — surface that instead of silently doing it
    if req.path.is_symlink() {
        return Ok(FileState::Differs(
            "target is a symlink; edit the real file instead".into(),
        ));
    }
    if !req.path.exists() {
        return Ok(FileState::Missing);
    }
    let text = file::read_to_string(&req.path)?;
    let lines: Vec<&str> = text.lines().collect();
    match &req.op {
        EditOp::Block { id, .. } => match find_block(&lines, id) {
            Err(reason) => Ok(FileState::Differs(reason)),
            Ok(None) => Ok(FileState::Missing),
            Ok(Some((begin, end))) => {
                let current = lines[begin + 1..end].join("\n");
                if current == desired.expect("resolved block content") {
                    Ok(FileState::Applied)
                } else {
                    Ok(FileState::Differs("block content differs".into()))
                }
            }
        },
        EditOp::Line { line } => {
            if lines.contains(&line.as_str()) {
                Ok(FileState::Applied)
            } else {
                Ok(FileState::Missing)
            }
        }
    }
}

pub struct ApplyOpts {
    pub dry_run: bool,
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
        // rendering can run exec() — a dry run must not execute anything,
        // so template blocks are listed without computing their state (same
        // policy as [system.files])
        let is_template = matches!(&req.op, EditOp::Block { template: true, .. });
        if opts.dry_run && is_template {
            if req.path.is_symlink() {
                problems.push(format!(
                    "  \"{}\" ({}): target is a symlink; edit the real file instead",
                    req.path_raw,
                    req.describe_op()
                ));
            } else {
                todo.push((req, None));
            }
            continue;
        }
        let desired = desired_content(config, req)?;
        match check_resolved(req, desired.as_deref())? {
            FileState::Applied => continue,
            FileState::Differs(reason)
                if reason.contains("marker") || reason.contains("symlink") =>
            {
                problems.push(format!(
                    "  \"{}\" ({}): {reason}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
            _ => todo.push((req, desired)),
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
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    match &req.op {
        EditOp::Block { id, comment, .. } => {
            let desired = desired.expect("resolved block content");
            let mut block = vec![begin_marker(comment, id)];
            // a desired of "" means an empty block, not a blank line
            if !desired.is_empty() {
                block.extend(desired.lines().map(|l| l.to_string()));
            }
            block.push(end_marker(comment, id));
            match find_block(&lines.iter().map(|l| l.as_str()).collect::<Vec<_>>(), id) {
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
        EditOp::Line { line } => lines.push(line.clone()),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    // file::write truncates in place, preserving the file's permissions
    file::write(&req.path, &out)?;
    Ok(())
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
        assert_eq!(find_block(&lines, "a"), Ok(Some((1, 3))));
        assert_eq!(find_block(&lines, "b"), Ok(None));
        // ids are delimited — "a" must not match "ab"
        let lines = vec!["# >>> mise:ab >>>", "# <<< mise:ab <<<"];
        assert_eq!(find_block(&lines, "a"), Ok(None));
        // content that mentions a marker mid-line is not a marker
        let lines = vec![
            "# >>> mise:a >>>",
            r#"echo "keep the >>> mise:a >>> line intact""#,
            "# <<< mise:a <<<",
        ];
        assert_eq!(find_block(&lines, "a"), Ok(Some((0, 2))));
        // ...but indented comment markers still count
        let lines = vec!["  # >>> mise:a >>>", "  # <<< mise:a <<<"];
        assert_eq!(find_block(&lines, "a"), Ok(Some((0, 1))));
        let lines = vec!["<!-- >>> mise:a >>>", "<!-- <<< mise:a <<<"];
        assert_eq!(find_block(&lines, "a"), Ok(Some((0, 1))));
        let lines = vec!["# >>> mise:a >>>"];
        assert!(find_block(&lines, "a").is_err());
        let lines = vec!["# <<< mise:a <<<", "# >>> mise:a >>>"];
        assert!(find_block(&lines, "a").is_err());
    }
}
