//! `[dotfiles]` — declarative edits to files mise doesn't own,
//! applied by `mise dot apply` or `mise bootstrap`, and
//! removed by `mise dot unapply`.
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
//! exact line exists, appending it if absent by default or prepending it when
//! `position = "prepend"`.
//!
//! Entries merge across the config hierarchy as a union keyed by
//! `(path, id)` — a more local config overrides an edit with the same id,
//! exactly like whole-file entries override by target.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail};
use indexmap::IndexMap;
use mise_util::structured_merge::{self, Format};
use serde::Deserialize;

use crate::config::{Config, ConfigMap};
use crate::file;
use crate::path::PathExt;
use crate::system::files::FileState;
use crate::system::history::journal::{self, Capture};
use crate::system::resources::ResourceOrigin;
use crate::ui::prompt;

/// one `[dotfiles]` edit entry as written in mise.toml. Operations stay loosely typed so configs using operations
/// from newer mise versions still parse (entries with no recognized
/// operation warn and are skipped)
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum EditTomlEntry {
    /// `activate = 'eval "$(mise activate zsh)"'` — inline block content
    Block(String),
    /// `aliases = { source = "...", template = "tera" }` /
    /// `dev = { line = "..." }`
    Table(EditTomlTable),
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EditTomlTable {
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
    /// where to insert a missing line; `"append"` (the default) or `"prepend"`
    #[serde(default)]
    pub position: Option<String>,
    /// comment prefix for the markers; inferred from the file extension
    /// when omitted
    #[serde(default)]
    pub comment: Option<String>,
    /// `merge = true` sets the keys in `source` on a structured target file
    /// (JSON, TOML, or YAML) and leaves every other key to the application
    /// that also writes it
    ///
    /// `merge = "missing"` sets only the keys of `source` the target has no
    /// value for, so a value the application changed stays
    #[serde(default)]
    pub merge: Option<MergeSetting>,
}

/// the value of an entry's `merge` key
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum MergeSetting {
    Bool(bool),
    Mode(String),
}

/// where a block's content comes from
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BlockSource {
    Inline(String),
    /// absolute path, resolved against the declaring config file
    File(PathBuf),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EditOp {
    Block {
        source: BlockSource,
        template: bool,
        comment: String,
    },
    Line {
        line: String,
        position: LinePosition,
    },
    /// set the keys of a JSON/TOML/YAML source on the target and leave the
    /// target's other keys alone
    Merge {
        source: BlockSource,
        template: bool,
        format: Format,
        /// only set keys the target has no value for
        missing_only: bool,
    },
}

impl EditOp {
    /// the source file this edit reads, if it reads one
    pub fn source_file(&self) -> Option<&Path> {
        match self {
            Self::Block {
                source: BlockSource::File(path),
                ..
            }
            | Self::Merge {
                source: BlockSource::File(path),
                ..
            } => Some(path),
            _ => None,
        }
    }

    /// a merge that only fills in keys the target lacks
    fn fills_missing_only(&self) -> bool {
        matches!(
            self,
            Self::Merge {
                missing_only: true,
                ..
            }
        )
    }

    /// whether rendering the edit's content runs the template engine
    fn is_template(&self) -> bool {
        matches!(
            self,
            Self::Block { template: true, .. } | Self::Merge { template: true, .. }
        )
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum LinePosition {
    Prepend,
    #[default]
    Append,
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
    pub origin: ResourceOrigin,
}

impl EditRequest {
    /// short operation label for status tables and dry-run output
    pub fn describe_op(&self) -> String {
        match &self.op {
            EditOp::Block { .. } => format!("block:{}", self.id),
            EditOp::Line { .. } => format!("line:{}", self.id),
            EditOp::Merge { .. } => format!("merge:{}", self.id),
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
pub fn edits_from_config(config: &Config) -> Result<Vec<EditRequest>> {
    let mut composed: IndexMap<String, EditRequest> = IndexMap::new();
    for config_files in config.bootstrap_config_maps() {
        for request in edits_from_config_files(config_files) {
            let key = format!("{}\u{0}{}", request.path.display(), request.id);
            if let Some(existing) = composed.get(&key) {
                if edit_requests_match(config, existing, &request) {
                    continue;
                }
                bail!(
                    "conflicting dotfile edit declarations for {}/{}\n\n  first:\n    {}\n\n  second:\n    {}",
                    request.path.display(),
                    request.id,
                    existing.origin.conflict_description(),
                    request.origin.conflict_description(),
                );
            }
            composed.insert(key, request);
        }
    }
    let edits = composed.into_values().collect::<Vec<_>>();
    // every command that applies or reports edits loads them here, so the
    // contradiction is refused before either kind of entry is applied
    if !edits.is_empty() {
        crate::system::files::validate_absent_edit_targets(
            &crate::system::files::files_from_config(config)?,
            &edits,
        )?;
    }
    Ok(edits)
}

/// Returns whether sibling declarations produce the same file edit.
fn edit_requests_match(config: &Config, first: &EditRequest, second: &EditRequest) -> bool {
    first.path == second.path
        && first.id == second.id
        && first.op == second.op
        && (!first.op.is_template()
            || first.base == second.base
                && config.bootstrap_tera_ctx(&first.origin.config)
                    == config.bootstrap_tera_ctx(&second.origin.config))
}

pub(crate) fn edits_from_config_files(config_files: &ConfigMap) -> Vec<EditRequest> {
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

/// keys that only mean something on a whole-file entry
const WHOLE_FILE_KEYS: [&str; 15] = [
    "mode",
    "content",
    "permissions",
    "exclude",
    "include",
    "manifest",
    "autosave",
    "encrypt",
    "allow_plaintext",
    "variants",
    "enabled",
    "remove_empty",
    "relative",
    "dot_prefix",
    "group",
];

/// Check an incoming `merge` declaration the way the edit parser will read it,
/// so `mise dot pull` refuses one that would later be dropped. Templates are
/// not rendered.
pub(crate) fn validate_incoming_merge(
    path_and_id: &str,
    value: &toml::Value,
    config_path: &Path,
) -> Result<()> {
    if let Some(table) = value.as_table() {
        for key in WHOLE_FILE_KEYS {
            if table.contains_key(key) {
                bail!(
                    "dotfile {path_and_id}: {key} applies to whole-file entries, not merge edits"
                );
            }
        }
    }
    let entry: EditTomlEntry = value
        .clone()
        .try_into()
        .map_err(|err| eyre::eyre!("dotfile {path_and_id}: invalid merge entry: {err}"))?;
    let Some((path_raw, id)) = split_edit_key(path_and_id) else {
        bail!("dotfile {path_and_id}: edit entries must end with an id path segment");
    };
    let base = config_path.parent().unwrap_or(Path::new("."));
    resolve_entry(&path_raw, id, entry, base, config_path)
        .map(|_| ())
        .map_err(|err| eyre::eyre!("dotfile {path_and_id}: {err}"))
}

fn edit_entry_from_toml(path_and_id: &str, value: toml::Value) -> Option<EditTomlEntry> {
    match &value {
        toml::Value::Table(table) if table.contains_key("merge") => {
            // a merge edits one structured file, so every key that shapes a
            // whole-file entry is a mistake worth naming, not silently dropped
            for key in WHOLE_FILE_KEYS {
                if table.contains_key(key) {
                    warn!(
                        "[dotfiles].\"{path_and_id}\": {key} applies to whole-file entries, not merge edits, ignoring entry"
                    );
                    return None;
                }
            }
        }
        toml::Value::Table(table) => {
            let is_whole_file_table = table.is_empty()
                || table.contains_key("mode")
                || table.contains_key("remove_empty")
                || (table.contains_key("source")
                    || table.contains_key("content")
                    || table.contains_key("permissions")
                    || table.contains_key("dot_prefix"))
                    && !table.contains_key("block")
                    && !table.contains_key("line")
                    && !table.contains_key("template")
                    && !table.contains_key("comment")
                    && !table.contains_key("position");
            if is_whole_file_table {
                return None;
            }
            // an edit owns lines in a file, not the file itself; dropping
            // the key silently would leave the declared mode unapplied
            for key in ["permissions", "dot_prefix"] {
                if table.contains_key(key) {
                    warn!(
                        "[dotfiles].\"{path_and_id}\": {key} applies to whole-file entries, not block or line edits, ignoring entry"
                    );
                    return None;
                }
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
    let mut origin = ResourceOrigin {
        config: config_path.to_path_buf(),
        config_root: crate::config::config_file::config_root::config_root(config_path),
        environment: crate::config::environments_for_config_path(config_path),
        source: None,
    };
    let entry = match entry {
        EditTomlEntry::Block(inline) => EditTomlTable {
            block: Some(inline),
            source: None,
            template: None,
            line: None,
            position: None,
            comment: None,
            merge: None,
        },
        EditTomlEntry::Table(table) => table,
    };
    if let Some(merge) = &entry.merge {
        let op = merge_op(path_raw, &id, &path, &entry, merge, base, &mut origin)?;
        return Ok(EditRequest {
            path_raw: path_raw.to_string(),
            path,
            id,
            op,
            base: base.to_path_buf(),
            config_path: config_path.to_path_buf(),
            origin,
        });
    }
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
            if entry.position.is_some() {
                bail!("\"{path_raw}\".{id}: position is only valid with line, ignoring entry")
            }
            let source = match (entry.block, entry.source) {
                (Some(_), Some(_)) => {
                    bail!(
                        "\"{path_raw}\".{id}: block and source are mutually exclusive, ignoring entry"
                    )
                }
                (Some(inline), None) => BlockSource::Inline(inline),
                (None, Some(src)) => {
                    let src = file::replace_path(&src);
                    let src = if src.is_relative() {
                        base.join(src)
                    } else {
                        src
                    };
                    origin.source = Some(src.clone());
                    BlockSource::File(src)
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
            let position = match entry.position.as_deref() {
                None | Some("append") => LinePosition::Append,
                Some("prepend") => LinePosition::Prepend,
                Some(other) => {
                    bail!(
                        "\"{path_raw}\".{id}: unknown line position '{other}' (expected \"append\" or \"prepend\"), ignoring entry"
                    )
                }
            };
            EditOp::Line {
                line: line.clone(),
                position,
            }
        }
    };
    Ok(EditRequest {
        path_raw: path_raw.to_string(),
        path,
        id,
        op,
        base: base.to_path_buf(),
        config_path: config_path.to_path_buf(),
        origin,
    })
}

/// the operation for `{ source = "...", merge = true }`
fn merge_op(
    path_raw: &str,
    id: &str,
    path: &Path,
    entry: &EditTomlTable,
    merge: &MergeSetting,
    base: &Path,
    origin: &mut ResourceOrigin,
) -> Result<EditOp> {
    let missing_only = match merge {
        MergeSetting::Bool(true) => false,
        MergeSetting::Mode(mode) if mode == "missing" => true,
        _ => bail!(
            "\"{path_raw}\".{id}: merge must be true or \"missing\" when present, ignoring entry"
        ),
    };
    if entry.block.is_some() || entry.line.is_some() {
        bail!("\"{path_raw}\".{id}: merge cannot be combined with block or line, ignoring entry");
    }
    if entry.position.is_some() || entry.comment.is_some() {
        bail!("\"{path_raw}\".{id}: position and comment do not apply to merge, ignoring entry");
    }
    // like a symlink entry, an omitted source is the target's path under
    // dotfiles.root
    let source = match &entry.source {
        Some(source) => source.clone(),
        // normalized first, so `~/../x` is not taken for a target under $HOME
        None => crate::system::files::implied_source(&crate::system::files::resolve_target_arg(
            path_raw,
        ))
        .map_err(|err| eyre::eyre!("\"{path_raw}\".{id}: {err}, ignoring entry"))?
        .to_string_lossy()
        .into_owned(),
    };
    let Some(format) = Format::from_path(path) else {
        bail!(
            "\"{path_raw}\".{id}: merge needs a .json, .toml, .yaml, or .yml target, ignoring entry"
        );
    };
    let source = file::replace_path(&source);
    let source = if source.is_relative() {
        base.join(source)
    } else {
        source
    };
    origin.source = Some(source.clone());
    let template = match entry.template.as_deref() {
        None => false,
        Some("tera") => true,
        Some(other) => {
            bail!(
                "\"{path_raw}\".{id}: unknown template engine '{other}' (expected \"tera\"), ignoring entry"
            )
        }
    };
    Ok(EditOp::Merge {
        source: BlockSource::File(source),
        template,
        format,
        missing_only,
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
    let (source, template, comment) = match &req.op {
        EditOp::Block {
            source,
            template,
            comment,
        } => (source, template, Some(comment)),
        EditOp::Merge {
            source, template, ..
        } => (source, template, None),
        EditOp::Line { .. } => return Ok(None),
    };
    let id = &req.id;
    let raw = match source {
        BlockSource::Inline(s) => s.clone(),
        BlockSource::File(p) => file::read_to_string(p)?,
    };
    let content = if *template {
        let mut tera = crate::tera::get_tera(Some(&req.base));
        crate::tera::render_str(
            &mut tera,
            &raw,
            config.bootstrap_tera_ctx(&req.origin.config),
        )
        .map_err(|err| {
            eyre::eyre!(
                "[dotfiles].\"{}/{}\": failed to render template: {err}",
                req.path_raw,
                req.id
            )
        })?
    } else {
        raw
    };
    let Some(comment) = comment else {
        // the source is data, not a block: a trailing newline can be part of
        // a YAML block scalar, so keep it. Parse it now so a bad source is
        // reported before any entry is written, even when the target is
        // missing
        if let EditOp::Merge { format, .. } = &req.op {
            structured_merge::contains(*format, "", &content).wrap_err_with(|| {
                format!(
                    "[dotfiles].\"{}/{}\": invalid merge source",
                    req.path_raw, req.id
                )
            })?;
        }
        return Ok(Some(content));
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
/// from `mise dot status`. That's the same trust model as `[env]`
/// templates. Rendering only happens once every render-free outcome (symlink
/// target, missing file, absent or corrupted markers) has been ruled out,
/// and `--dry-run` skips template rendering entirely (see [`apply`]).
pub fn check(config: &Config, req: &EditRequest) -> Result<FileState> {
    let selected = if req.op.fills_missing_only() {
        edits_from_config(config)?
    } else {
        vec![]
    };
    check_selected(config, req, &selected)
}

/// Inspect an edit after the earlier merges selected for this run.
pub fn check_selected(
    config: &Config,
    req: &EditRequest,
    selected: &[EditRequest],
) -> Result<FileState> {
    if let Some(p) = req.op.source_file()
        && !p.exists()
    {
        return Ok(FileState::SourceMissing);
    }
    match precheck(req)? {
        Some(EditCheck::State(state)) => Ok(state),
        Some(EditCheck::Blocked(reason)) => Ok(FileState::Differs(reason)),
        None => {
            let desired = desired_content(config, req)?;
            if let EditOp::Merge {
                format,
                missing_only: true,
                ..
            } = &req.op
            {
                // judged the way apply runs it: after the file's other merges
                let text =
                    projected_text(config, req, &file::read_to_string(&req.path)?, selected)?;
                let desired = desired.expect("resolved merge content");
                return Ok(
                    if structured_merge::missing(*format, &text, &desired)?.is_none() {
                        FileState::Applied
                    } else {
                        FileState::Differs("keys are missing".into())
                    },
                );
            }
            block_state(req, desired.as_deref())
        }
    }
}

/// `text` as apply leaves it for a fill-only entry: enforced merges first,
/// then earlier defaults for the same file. Other entries see `text` unchanged.
fn projected_text(
    config: &Config,
    req: &EditRequest,
    text: &str,
    selected: &[EditRequest],
) -> Result<String> {
    if !req.op.fills_missing_only() {
        return Ok(text.to_string());
    }
    let mut text = text.to_string();
    let mut ordered = selected.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|other| other.op.fills_missing_only());
    for other in ordered {
        let EditOp::Merge {
            format,
            missing_only,
            ..
        } = &other.op
        else {
            continue;
        };
        if other.path == req.path && other.id == req.id {
            break;
        }
        if !same_target(&other.path, &req.path) {
            continue;
        }
        if let Some(desired) = desired_content(config, other)? {
            if *missing_only {
                text = structured_merge::fill_missing(*format, &text, &desired)?;
            } else {
                text = structured_merge::merge(*format, &text, &desired)?;
            }
        }
    }
    Ok(text)
}

const SYMLINK_REASON: &str = "target is a symlink; edit the real file instead";
const MERGE_SYMLINK_REASON: &str = "target is a symlink; replace it with a copy of the real file before merging (the merge source may be trimmed to the owned keys only afterwards)";

/// a merge target that is a link to the merge's own source, as when the entry
/// used to be a `symlink`: the file behind it holds the application's state
fn links_to_merge_source(req: &EditRequest) -> bool {
    if !matches!(req.op, EditOp::Merge { .. }) {
        return false;
    }
    let Some(source) = req.op.source_file() else {
        return false;
    };
    matches!(
        (req.path.canonicalize(), source.canonicalize()),
        (Ok(target), Ok(source)) if target == source
    )
}

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
        if links_to_merge_source(req) {
            return Ok(Some(EditCheck::State(FileState::Differs(
                "symlink to the merge source; apply replaces it with a copy".into(),
            ))));
        }
        return Ok(Some(EditCheck::Blocked(
            if matches!(req.op, EditOp::Merge { .. }) {
                MERGE_SYMLINK_REASON
            } else {
                SYMLINK_REASON
            }
            .into(),
        )));
    }
    if !req.path.exists() {
        return Ok(Some(EditCheck::State(FileState::Missing)));
    }
    let text = file::read_to_string(&req.path)?;
    match &req.op {
        EditOp::Block { comment, .. } => {
            match find_block(&text.lines().collect::<Vec<_>>(), &req.id, comment) {
                Err(reason) => Ok(Some(EditCheck::Blocked(reason))),
                Ok(None) => Ok(Some(EditCheck::State(FileState::Missing))),
                Ok(Some(_)) => Ok(None),
            }
        }
        // whether the owned keys are in place takes a parse of both sides
        EditOp::Merge { .. } => Ok(None),
        EditOp::Line { line, .. } => Ok(Some(EditCheck::State(
            if text
                .strip_prefix('\u{feff}')
                .unwrap_or(&text)
                .lines()
                .any(|candidate| candidate == line)
            {
                FileState::Applied
            } else {
                FileState::Missing
            },
        ))),
    }
}

/// content comparison for a block whose markers exist ([`precheck`]
/// returned None)
fn block_state(req: &EditRequest, desired: Option<&str>) -> Result<FileState> {
    let text = file::read_to_string(&req.path)?;
    let comment = match &req.op {
        EditOp::Block { comment, .. } => comment,
        EditOp::Merge {
            format,
            missing_only: true,
            ..
        } => {
            let desired = desired.expect("resolved merge content");
            return Ok(
                if structured_merge::missing(*format, &text, desired)?.is_none() {
                    FileState::Applied
                } else {
                    FileState::Differs("keys are missing".into())
                },
            );
        }
        EditOp::Merge { format, .. } => {
            let desired = desired.expect("resolved merge content");
            return Ok(if structured_merge::contains(*format, &text, desired)? {
                FileState::Applied
            } else {
                FileState::Differs("merged keys differ".into())
            });
        }
        EditOp::Line { .. } => unreachable!("only blocks and merges reach a content comparison"),
    };
    let id = &req.id;
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

/// Whether two edit targets are the same file under different names: equal
/// paths, existing files with the same identity (a hard link, or a case
/// variant on a case-insensitive volume), or missing files with the same name
/// in one directory reached by two spellings (a symlinked parent, say).
///
/// A missing file has no identity, so a case-only difference in its name is
/// not guessed at: whether the directory ignores case can't be learned without
/// writing to it. Once the first of two such entries has created the file, the
/// pair is compared by identity and a conflict between them is refused.
fn same_target(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let Some(same) = same_identity(a, b) {
        return same;
    }
    let (ancestor_a, tail_a) = split_existing(a);
    let (ancestor_b, tail_b) = split_existing(b);
    tail_a == tail_b
        && (ancestor_a == ancestor_b || same_identity(&ancestor_a, &ancestor_b).unwrap_or(false))
}

/// Whether two existing paths are one file, or `None` when either is missing.
/// On Unix this compares device and inode from `stat`, which opens nothing, so a
/// named pipe or another path that blocks on open cannot hang the check.
fn same_identity(a: &Path, b: &Path) -> Option<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (a, b) = (std::fs::metadata(a).ok()?, std::fs::metadata(b).ok()?);
        Some(a.dev() == b.dev() && a.ino() == b.ino())
    }
    #[cfg(not(unix))]
    {
        same_file::is_same_file(a, b).ok()
    }
}

/// The nearest existing ancestor of `path` (or `path` itself) and the
/// components below it.
fn split_existing(path: &Path) -> (PathBuf, Vec<std::ffi::OsString>) {
    let mut existing = path;
    let mut tail = vec![];
    while !existing.exists() {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            break;
        };
        tail.push(name.to_os_string());
        existing = parent;
    }
    tail.reverse();
    (existing.to_path_buf(), tail)
}

/// Two merge entries for one file that set the same key to different values
/// would each look unapplied after the other ran, so every apply would flip the
/// value back and forth. Refuse that instead of picking a winner. Every entry
/// this run applies is compared with the others it applies and with the
/// config's other entries for the file; entries this run leaves alone are not
/// compared with each other, since nothing here would write them.
fn merge_conflicts(
    applied: &[(&EditRequest, Format, String)],
    unapplied: &[(&EditRequest, Format, String)],
) -> Vec<String> {
    let mut problems = vec![];
    for (i, first) in applied.iter().enumerate() {
        for second in applied[i + 1..].iter().chain(unapplied) {
            problems.extend(merge_conflict(first, second));
        }
    }
    problems
}

fn merge_conflict(
    (first, format, first_content): &(&EditRequest, Format, String),
    (second, second_format, second_content): &(&EditRequest, Format, String),
) -> Option<String> {
    if !same_target(&first.path, &second.path) || format != second_format {
        return None;
    }
    // a fill-only entry gives way to any value another entry sets
    if first.op.fills_missing_only() || second.op.fills_missing_only() {
        return None;
    }
    // unparseable sources were already reported by desired_content
    let keys = structured_merge::conflicts(*format, first_content, second_content).ok()?;
    (!keys.is_empty()).then(|| {
        format!(
            "  \"{}\": {} and {} set different values for {}",
            first.path_raw,
            first.describe_op(),
            second.describe_op(),
            keys.join(", ")
        )
    })
}

/// Merge entries that reach one file through different extensions (hard links
/// under a .json and a .yml name) cannot be merged consistently as both
/// formats, whatever their sources hold. The format comes from the extension,
/// so this needs no rendering and holds on dry runs and for templates too.
fn format_conflicts(requests: &[EditRequest], siblings: &[EditRequest]) -> Vec<String> {
    fn merges(reqs: &[EditRequest]) -> Vec<(&EditRequest, Format)> {
        reqs.iter()
            .filter_map(|req| match &req.op {
                EditOp::Merge { format, .. } => Some((req, *format)),
                _ => None,
            })
            .collect()
    }
    let applied = merges(requests);
    let others: Vec<_> = merges(siblings)
        .into_iter()
        .filter(|(sibling, _)| {
            !requests
                .iter()
                .any(|req| req.path == sibling.path && req.id == sibling.id)
        })
        .collect();
    let mut problems = vec![];
    for (i, (first, first_format)) in applied.iter().enumerate() {
        let candidates = applied[i + 1..]
            .iter()
            .map(|entry| (entry, false))
            .chain(others.iter().map(|entry| (entry, true)));
        for ((second, second_format), is_sibling) in candidates {
            if first_format == second_format || !same_target(&first.path, &second.path) {
                continue;
            }
            // only now, for a sibling that really shares the file, look at its
            // target: a blocked one (a symlink) cannot be written by that entry,
            // and a target this run never touches is never opened
            if is_sibling && !matches!(precheck(second), Ok(None | Some(EditCheck::State(_)))) {
                continue;
            }
            problems.push(format!(
                "  \"{}\": {} and {} merge into one file as different formats",
                first.path_raw,
                first.describe_op(),
                second.describe_op(),
            ));
        }
    }
    problems
}

/// The merge entries of `siblings` that this run is not applying but that
/// target a file this run merges into, so a conflict with them is caught too.
/// A template sibling is never rendered, since that could run `exec()` for an
/// entry nobody asked to apply. A conflict involving a template is therefore
/// found only when every entry in it is applied in the same run, not when one
/// of them is applied alone through a target filter. Entries whose target is
/// blocked are skipped as well.
fn unapplied_siblings<'a>(
    config: &Config,
    requests: &[EditRequest],
    siblings: &'a [EditRequest],
    merged: &[(&EditRequest, Format, String)],
) -> Vec<(&'a EditRequest, Format, String)> {
    let mut found = vec![];
    for sibling in siblings {
        let EditOp::Merge { format, .. } = &sibling.op else {
            continue;
        };
        if requests
            .iter()
            .any(|req| req.path == sibling.path && req.id == sibling.id)
            || !merged
                .iter()
                .any(|(req, ..)| same_target(&req.path, &sibling.path))
            || sibling.op.is_template()
            || !matches!(precheck(sibling), Ok(None | Some(EditCheck::State(_))))
        {
            continue;
        }
        // a sibling that cannot be rendered is reported when it is applied
        if let Ok(Some(content)) = desired_content(config, sibling) {
            found.push((sibling, *format, content));
        }
    }
    found
}

pub struct ApplyOpts {
    pub dry_run: bool,
    pub verbose: bool,
    pub yes: bool,
    /// bootstrap part these edits belong to, for the generation journal
    pub part: &'static str,
}

/// Apply all edits that aren't already in the desired state. Edits never
/// replace files, so there is no --force here — but corrupted markers and
/// symlink targets are reported as errors rather than guessed at. Returns
/// `false` when the user declines the confirmation prompt. The paths edited
/// are appended to `written` as each entry is applied, so a caller still
/// sees what changed when a later entry fails; nothing is appended on a dry
/// run.
pub fn apply(
    config: &Config,
    requests: &[EditRequest],
    opts: &ApplyOpts,
    written: &mut Vec<PathBuf>,
) -> Result<bool> {
    let mut todo: Vec<(&EditRequest, Option<String>)> = vec![];
    // fill-only entries that look applied against the file as it is now; another
    // merge applied in this run may still make their defaults needed
    let mut deferred: Vec<(&EditRequest, Option<String>)> = vec![];
    let mut problems = vec![];
    // other merge entries in the config, so applying one entry through a target
    // filter still sees a sibling that sets the same key differently
    let siblings = if requests
        .iter()
        .any(|req| matches!(req.op, EditOp::Merge { .. }))
    {
        edits_from_config(config).unwrap_or_default()
    } else {
        vec![]
    };
    // every merge source rendered this run, for the cross-entry conflict check
    let mut merged: Vec<(&EditRequest, Format, String)> = vec![];
    for req in requests {
        if let Some(p) = req.op.source_file()
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
        if opts.dry_run && req.op.is_template() {
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
        if let (EditOp::Merge { format, .. }, Some(content)) = (&req.op, &desired) {
            merged.push((req, *format, content.clone()));
        }
        match pre {
            // markers exist: compare content to see if anything would change
            None => match block_state(req, desired.as_deref()) {
                Ok(FileState::Applied) => {
                    if req.op.fills_missing_only() {
                        deferred.push((req, desired));
                    }
                    continue;
                }
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
    // an enforced merge can replace a value with a table, or recreate a key, so
    // a fill-only entry on the same file is reconsidered after it
    for (req, desired) in deferred {
        if todo.iter().any(|(other, _)| {
            matches!(other.op, EditOp::Merge { .. })
                && !other.op.fills_missing_only()
                && same_target(&other.path, &req.path)
        }) {
            todo.push((req, desired));
        }
    }
    // defaults go in after the values other entries set
    todo.sort_by_key(|(req, _)| req.op.fills_missing_only());
    let unapplied = unapplied_siblings(config, requests, &siblings, &merged);
    problems.extend(merge_conflicts(&merged, &unapplied));
    problems.extend(format_conflicts(requests, &siblings));
    if !problems.is_empty() {
        bail!(
            "edits: cannot apply these entries, fix them manually:\n{}",
            problems.join("\n")
        );
    }
    if todo.is_empty() {
        info!("edits: all edits are applied");
        return Ok(true);
    }
    if opts.dry_run {
        for (req, desired) in &todo {
            // template state wasn't computed (no rendering on dry runs), so
            // the entry may already be converged
            let conditional = desired.is_none() && req.op.is_template();
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
        return Ok(true);
    }
    if !opts.yes && console::user_attended_stderr() {
        let list = todo
            .iter()
            .map(|(r, _)| format!("{} ({})", r.path_raw, r.describe_op()))
            .collect::<Vec<_>>()
            .join(", ");
        if !prompt::confirm(format!("edits: apply {list}?"))?.is_yes() {
            info!("edits: skipped");
            return Ok(false);
        }
    }
    for (req, desired) in &todo {
        let pending = journal::begin_changes_with(opts.part, &req.path_raw, edit_paths(&req.path))?;
        apply_one(req, desired.as_deref(), written)?;
        journal::commit_changes(pending);
    }
    let applied = todo
        .iter()
        .map(|(r, _)| format!("{} ({})", r.path_raw, r.describe_op()))
        .collect::<Vec<_>>()
        .join(", ");
    info!("edits: applied {applied}");
    Ok(true)
}

/// Print unified patches for the changes required to converge edit entries.
/// Template blocks are rendered because an exact diff requires their desired
/// content, matching the trust and execution semantics of dotfiles status.
pub fn print_diffs(config: &Config, requests: &[EditRequest]) -> Result<()> {
    let mut changed = false;
    let mut problems = vec![];
    for req in requests {
        if let Some(path) = req.op.source_file()
            && !path.exists()
        {
            miseprintln!(
                "{} ({}): source missing: {}",
                req.path.display_user(),
                req.describe_op(),
                path.display_user()
            );
            changed = true;
            continue;
        }
        let pre = match precheck(req) {
            Ok(Some(EditCheck::Blocked(reason))) => {
                problems.push(format!(
                    "  \"{}\" ({}): {reason}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
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
        if matches!(&pre, Some(EditCheck::State(FileState::Applied))) {
            continue;
        }
        let desired = match desired_content(config, req) {
            Ok(desired) => desired,
            Err(err) => {
                problems.push(format!("  {err}"));
                continue;
            }
        };
        if pre.is_none() && !req.op.fills_missing_only() {
            match block_state(req, desired.as_deref()) {
                Ok(FileState::Applied) => continue,
                Ok(_) => {}
                Err(err) => {
                    problems.push(format!(
                        "  \"{}\" ({}): {err}",
                        req.path_raw,
                        req.describe_op()
                    ));
                    continue;
                }
            }
        }
        let current = if req.path.exists() {
            match file::read_to_string(&req.path) {
                Ok(current) => current,
                Err(err) => {
                    problems.push(format!(
                        "  \"{}\" ({}): {err}",
                        req.path_raw,
                        req.describe_op()
                    ));
                    continue;
                }
            }
        } else {
            String::new()
        };
        // a fill-only entry is shown against the file after its other merges
        let current = match projected_text(config, req, &current, requests) {
            Ok(current) => current,
            Err(err) => {
                problems.push(format!(
                    "  \"{}\" ({}): {err}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
        };
        let output = match apply_to_string(req, desired.as_deref(), &current) {
            Ok(output) => output,
            Err(err) => {
                problems.push(format!(
                    "  \"{}\" ({}): {err}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
        };
        if current == output {
            continue;
        }
        changed = true;
        miseprintln!(
            "edit differs: {} ({})",
            req.path.display_user(),
            req.describe_op()
        );
        let mut opts = diffy::DiffOptions::new();
        opts.set_original_filename(format!("{} (current)", req.path.display_user()))
            .set_modified_filename(format!(
                "{} (desired: {})",
                req.path.display_user(),
                req.describe_op()
            ));
        let patch = opts.create_patch(&current, &output);
        miseprint!("{}", diffy::PatchFormatter::new().fmt_patch(&patch))?;
    }
    if !problems.is_empty() {
        bail!(
            "edits: cannot diff these entries, fix them manually:\n{}",
            problems.join("\n")
        );
    }
    if !changed {
        info!("edits: all edits are applied");
    }
    Ok(())
}

pub struct UnapplyOpts {
    pub dry_run: bool,
    pub verbose: bool,
    /// plain line edits have no ownership marker, so removing them requires
    /// explicit confirmation that the configured line should be removed
    pub force: bool,
    pub yes: bool,
}

pub struct UnapplyPlan<'a> {
    req: &'a EditRequest,
    /// Exact target contents observed during planning. Template functions run
    /// before execution, so selected edits must still have this same state.
    text: String,
}

/// Remove marker-delimited blocks and, with `--force`, exact line edits.
/// Block markers are their ownership record. A plain line may have existed
/// before apply, so stateless unapply refuses to guess without `--force`.
pub fn plan_unapply<'a>(
    requests: &'a [EditRequest],
    opts: &UnapplyOpts,
) -> Result<Vec<UnapplyPlan<'a>>> {
    let mut todo = vec![];
    let mut problems = vec![];
    let mut seen_lines = indexmap::IndexSet::new();
    for req in requests {
        // merged keys carry no ownership record and may have been changed by
        // the application since, so they stay put; the target is never read
        if matches!(req.op, EditOp::Merge { .. }) {
            continue;
        }
        if req.path.is_symlink() {
            problems.push(format!(
                "  \"{}\" ({}): {SYMLINK_REASON}",
                req.path_raw,
                req.describe_op()
            ));
            continue;
        }
        if !req.path.exists() {
            continue;
        }
        let text = match file::read_to_string(&req.path) {
            Ok(text) => text,
            Err(err) => {
                problems.push(format!(
                    "  \"{}\" ({}): {err}",
                    req.path_raw,
                    req.describe_op()
                ));
                continue;
            }
        };
        let lines = text
            .strip_prefix('\u{feff}')
            .unwrap_or(&text)
            .lines()
            .collect::<Vec<_>>();
        match &req.op {
            EditOp::Block { comment, .. } => match find_block(&lines, &req.id, comment) {
                Ok(Some(_)) => todo.push(UnapplyPlan { req, text }),
                Ok(None) => {}
                Err(reason) => problems.push(format!(
                    "  \"{}\" ({}): {reason}, fix the file manually",
                    req.path_raw,
                    req.describe_op()
                )),
            },
            EditOp::Line { line, .. } if lines.contains(&line.as_str()) => {
                if !opts.force {
                    problems.push(format!(
                        "  \"{}\" ({}): line edits have no ownership marker; use --force to remove the line",
                        req.path_raw,
                        req.describe_op()
                    ));
                } else if seen_lines.insert((req.path.clone(), line.clone())) {
                    // Two ids with the same line converge to one applied line
                    // and therefore produce one unapply action.
                    todo.push(UnapplyPlan { req, text });
                }
            }
            EditOp::Line { .. } | EditOp::Merge { .. } => {}
        }
    }
    if !problems.is_empty() {
        bail!(
            "edits: cannot unapply these entries:\n{}",
            problems.join("\n")
        );
    }
    Ok(todo)
}

/// Ensure template functions or another concurrent actor did not invalidate
/// any edit ownership checks performed during planning.
pub fn validate_unapply(todo: &[UnapplyPlan<'_>]) -> Result<()> {
    let mut checked = indexmap::IndexSet::new();
    let mut problems = vec![];
    for plan in todo {
        if !checked.insert(plan.req.path.clone()) {
            continue;
        }
        let result = if plan.req.path.is_symlink() {
            Err(eyre::eyre!("{SYMLINK_REASON}"))
        } else {
            file::read_to_string(&plan.req.path).and_then(|text| {
                if text == plan.text {
                    Ok(())
                } else {
                    bail!("target changed after unapply planning")
                }
            })
        };
        if let Err(err) = result {
            problems.push(format!(
                "  \"{}\" ({}): {err}",
                plan.req.path_raw,
                plan.req.describe_op()
            ));
        }
    }
    if !problems.is_empty() {
        bail!(
            "edits: cannot unapply these entries:\n{}",
            problems.join("\n")
        );
    }
    Ok(())
}

pub fn execute_unapply(todo: &[UnapplyPlan<'_>], opts: &UnapplyOpts) -> Result<()> {
    if todo.is_empty() {
        info!("edits: all edits are unapplied");
        return Ok(());
    }
    if opts.dry_run {
        for plan in todo {
            miseprintln!(
                "remove edit {} ({})",
                plan.req.path.display_user(),
                plan.req.describe_op()
            );
            if opts.verbose {
                miseprintln!("  preserve all content outside the managed edit");
            }
        }
        return Ok(());
    }
    if !opts.yes && console::user_attended_stderr() {
        let list = todo
            .iter()
            .map(|plan| format!("{} ({})", plan.req.path_raw, plan.req.describe_op()))
            .collect::<Vec<_>>()
            .join(", ");
        if !prompt::confirm(format!("edits: unapply {list}?"))?.is_yes() {
            info!("edits: skipped");
            return Ok(());
        }
    }
    for plan in todo {
        let pending =
            journal::begin_changes("dotfiles", &plan.req.path_raw, [plan.req.path.clone()])?;
        unapply_one(plan.req)?;
        journal::commit_changes(pending);
    }
    crate::system::history::journal::note(format!(
        "edits: unapplied {}",
        todo.iter()
            .map(|plan| format!("{} ({})", plan.req.path_raw, plan.req.describe_op()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    info!(
        "edits: unapplied {}",
        todo.iter()
            .map(|plan| format!("{} ({})", plan.req.path_raw, plan.req.describe_op()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

fn unapply_one(req: &EditRequest) -> Result<()> {
    let text = file::read_to_string(&req.path)?;
    let lines = text_lines(&text);
    let remove = match &req.op {
        EditOp::Block { comment, .. } => {
            let refs = lines.iter().map(|line| line.content).collect::<Vec<_>>();
            match find_block(&refs, &req.id, comment) {
                Ok(Some((begin, end))) => lines[begin].start..lines[end].end,
                Ok(None) => return Ok(()),
                Err(reason) => bail!(
                    "edits: \"{}\": {reason}, fix the file manually",
                    req.path_raw
                ),
            }
        }
        EditOp::Merge { .. } => return Ok(()),
        EditOp::Line { line, position } => {
            // Use the occurrence nearest the configured insertion edge as the
            // best stateless approximation of the line mise added.
            let found = match position {
                LinePosition::Prepend => lines.iter().find(|candidate| candidate.content == line),
                LinePosition::Append => lines.iter().rfind(|candidate| candidate.content == line),
            };
            if let Some(found) = found {
                found.start..found.end
            } else {
                return Ok(());
            }
        }
    };
    let mut out = text;
    out.replace_range(remove, "");
    file::write(&req.path, out)?;
    Ok(())
}

struct TextLine<'a> {
    content: &'a str,
    start: usize,
    end: usize,
}

/// Logical lines plus their exact byte spans, including CRLF/LF terminators.
/// Edit removal uses the spans so bytes outside the removed edit are preserved.
fn text_lines(text: &str) -> Vec<TextLine<'_>> {
    let mut offset = 0;
    text.split_inclusive('\n')
        .map(|raw| {
            let mut start = offset;
            offset += raw.len();
            let content = raw.strip_suffix('\n').unwrap_or(raw);
            let mut content = content.strip_suffix('\r').unwrap_or(content);
            if start == 0
                && let Some(without_bom) = content.strip_prefix('\u{feff}')
            {
                start += '\u{feff}'.len_utf8();
                content = without_bom;
            }
            TextLine {
                content,
                start,
                end: offset,
            }
        })
        .collect()
}

/// Simulate applying an edit to in-memory text for bootstrap dry-run config
/// discovery. Template edits are intentionally not rendered during dry-runs
/// because rendering may execute user commands.
pub fn apply_dry_run_to_string(
    config: &Config,
    req: &EditRequest,
    text: &str,
) -> Result<Option<String>> {
    if req.op.is_template() {
        return Ok(None);
    }
    let desired = desired_content(config, req)?;
    apply_to_string(req, desired.as_deref(), text).map(Some)
}

/// The paths an edit may create or change: the file itself, captured whole,
/// preceded by any ancestors `apply_one` will have to create.
fn edit_paths(path: &Path) -> Vec<(PathBuf, Capture)> {
    let mut paths: Vec<(PathBuf, Capture)> = crate::system::files::missing_ancestors(path)
        .into_iter()
        .map(|dir| (dir, Capture::Shallow))
        .collect();
    paths.push((path.to_path_buf(), Capture::Full));
    paths
}

/// Write one edit, appending its path to `written` at the point the file is
/// first mutated. An existing file is truncated in place (preserving its
/// permissions), so it is recorded once it has been opened for truncation:
/// an open that fails (a read-only file or filesystem) changes nothing and
/// records nothing, while a write that fails after it still leaves the file
/// changed — the journal preimage is only restored by a later recovery run.
/// A new file is recorded once it exists, even if the write that created it
/// then failed.
fn apply_one(req: &EditRequest, desired: Option<&str>, written: &mut Vec<PathBuf>) -> Result<()> {
    use std::io::Write;
    debug!("edits: {} ({})", req.path.display_user(), req.describe_op());
    if let Some(parent) = req.path.parent() {
        file::create_dir_all(parent)?;
    }
    // switching from `symlink`: keep what the link points at as a regular file
    // before merging, so trimming the source afterwards loses nothing
    let replaced_link = matches!(req.op, EditOp::Merge { .. })
        && req.path.is_symlink()
        && links_to_merge_source(req);
    if replaced_link {
        // prepare the whole copy beside the link in an exclusively created
        // temp file (std::fs::copy gives it the source's mode), then rename it
        // over the link, so a failure leaves the link in place
        let dir = req.path.parent().unwrap_or(Path::new("."));
        let tmp = tempfile::NamedTempFile::new_in(dir)
            .wrap_err_with(|| format!("failed to replace symlink: {}", req.path.display_user()))?;
        std::fs::copy(&req.path, tmp.path())
            .wrap_err_with(|| format!("failed to replace symlink: {}", req.path.display_user()))?;
        // a read-only source (a Nix store file, `chmod -w`) must not leave a
        // copy the merge cannot write
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(tmp.path())?.permissions();
            perms.set_mode(perms.mode() | 0o200);
            std::fs::set_permissions(tmp.path(), perms)?;
        }
        #[cfg(windows)]
        {
            use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
            };
            let path = std::fs::canonicalize(tmp.path())?;
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let attributes = std::fs::metadata(tmp.path())?.file_attributes();
            if unsafe { SetFileAttributesW(wide.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY) }
                == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        tmp.persist(&req.path)
            .map_err(|err| err.error)
            .wrap_err_with(|| format!("failed to replace symlink: {}", req.path.display_user()))?;
    }
    let existed = req.path.exists();
    let text = if existed {
        file::read_to_string(&req.path)?
    } else {
        String::new()
    };
    let out = apply_to_string(req, desired, &text)?;
    // a fill-only entry reconsidered after another merge may have nothing left to add
    if existed && req.op.fills_missing_only() && out == text {
        // a replaced link still changed the path, which reload commands key on
        if replaced_link {
            written.push(req.path.clone());
        }
        return Ok(());
    }
    let failed = || format!("failed write: {}", req.path.display_user());
    if existed {
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&req.path)
            .wrap_err_with(failed)?;
        written.push(req.path.clone());
        target.write_all(out.as_bytes()).wrap_err_with(failed)?;
    } else {
        crate::system::files::create_recorded(&req.path, written, || file::write(&req.path, &out))?;
    }
    Ok(())
}

fn apply_to_string(req: &EditRequest, desired: Option<&str>, text: &str) -> Result<String> {
    match &req.op {
        EditOp::Merge {
            format,
            missing_only,
            ..
        } => {
            let desired = desired.expect("resolved merge content");
            let merged = if *missing_only {
                structured_merge::fill_missing(*format, text, desired)
            } else {
                structured_merge::merge(*format, text, desired)
            };
            merged.wrap_err_with(|| format!("merge into \"{}\" failed", req.path.display_user()))
        }
        EditOp::Block { comment, .. } => {
            let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
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
            let mut out = lines.join("\n");
            out.push('\n');
            Ok(out)
        }
        EditOp::Line { line, position } => {
            // an earlier entry in the same batch may have just written an
            // identical line (two ids, same text) — stay idempotent against
            // the file's current content, not the state at plan time
            let (bom, body) = text
                .strip_prefix('\u{feff}')
                .map_or(("", text), |body| ("\u{feff}", body));
            if body.lines().any(|candidate| candidate == line) {
                return Ok(text.to_string());
            }
            let newline = body
                .find('\n')
                .filter(|&i| i.checked_sub(1).and_then(|i| body.as_bytes().get(i)) == Some(&b'\r'))
                .map_or("\n", |_| "\r\n");
            let out = match position {
                LinePosition::Prepend if body.is_empty() => format!("{bom}{line}{newline}"),
                LinePosition::Prepend => format!("{bom}{line}{newline}{body}"),
                LinePosition::Append if body.is_empty() => format!("{bom}{line}{newline}"),
                LinePosition::Append if text.ends_with('\n') => format!("{text}{line}{newline}"),
                LinePosition::Append => format!("{text}{newline}{line}{newline}"),
            };
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn apply_one_records_an_existing_file_only_once_it_is_opened_for_writing() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(".zshrc");
        file::write(&path, "before\n")?;
        let req = EditRequest {
            path_raw: path.to_string_lossy().to_string(),
            path: path.clone(),
            id: "activate".into(),
            op: EditOp::Line {
                line: "eval \"$(mise activate zsh)\"".into(),
                position: LinePosition::Append,
            },
            base: dir.path().to_path_buf(),
            config_path: dir.path().join("mise.toml"),
            origin: ResourceOrigin {
                config: dir.path().join("mise.toml"),
                config_root: dir.path().to_path_buf(),
                environment: vec![],
                source: None,
            },
        };

        // a writable file is recorded and edited
        let mut written = vec![];
        apply_one(&req, None, &mut written)?;
        assert_eq!(written, vec![path.clone()]);
        assert!(file::read_to_string(&path)?.contains("mise activate"));

        // a read-only file cannot be opened for truncation: nothing changes
        // and nothing is recorded (root can open it regardless, so the case
        // is skipped there)
        file::write(&path, "before\n")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444))?;
        if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
            return Ok(());
        }
        let mut written = vec![];
        assert!(apply_one(&req, None, &mut written).is_err());
        assert!(written.is_empty());
        assert_eq!(file::read_to_string(&path)?, "before\n");
        Ok(())
    }

    fn resolve(path: &str, entry: &str) -> Result<EditRequest> {
        let entry: EditTomlEntry = toml::from_str(entry).map_err(|e| eyre::eyre!("{e}"))?;
        resolve_entry(
            path,
            "shared".into(),
            entry,
            Path::new("/cfg"),
            Path::new("/cfg/mise.toml"),
        )
    }

    #[test]
    fn merge_entries_infer_the_format_from_the_target() {
        for (path, format) in [
            ("~/a/config.toml", Format::Toml),
            ("~/a/settings.json", Format::Json),
            ("~/a/config.yml", Format::Yaml),
            ("~/a/config.yaml", Format::Yaml),
        ] {
            let req = resolve(path, "source = \"shared.txt\"\nmerge = true").unwrap();
            assert_eq!(
                req.op,
                EditOp::Merge {
                    source: BlockSource::File(PathBuf::from("/cfg/shared.txt")),
                    template: false,
                    format,
                    missing_only: false,
                }
            );
            assert_eq!(req.describe_op(), "merge:shared");
            assert_eq!(req.origin.source, Some(PathBuf::from("/cfg/shared.txt")));
        }
    }

    #[test]
    fn merge_entries_can_be_templates() {
        let req = resolve(
            "~/a/config.toml",
            "source = \"s.toml.tera\"\nmerge = true\ntemplate = \"tera\"",
        )
        .unwrap();
        assert!(req.op.is_template());
    }

    /// an absolute path outside $HOME on every platform
    const OUTSIDE_HOME: &str = if cfg!(windows) {
        "C:/outside"
    } else {
        "/outside"
    };

    #[test]
    fn an_omitted_merge_source_needs_a_target_under_home() {
        let err = resolve(&format!("{OUTSIDE_HOME}/config.toml"), "merge = true")
            .unwrap_err()
            .to_string();
        assert!(err.contains("source is required"), "{err}");
        // `..` cannot walk a target out of $HOME and still pass for one inside it
        let err = resolve("~/../outside/config.toml", "merge = true")
            .unwrap_err()
            .to_string();
        assert!(err.contains("source is required"), "{err}");
    }

    #[test]
    fn merge_missing_fills_only_absent_keys() {
        let req = resolve("~/a/config.toml", "source = \"s\"\nmerge = \"missing\"").unwrap();
        assert!(req.op.fills_missing_only());
        let err = resolve("~/a/config.toml", "source = \"s\"\nmerge = \"other\"")
            .unwrap_err()
            .to_string();
        assert!(err.contains("true or \"missing\""), "{err}");
    }

    #[test]
    fn invalid_merge_entries_are_refused() {
        for (path, entry, reason) in [
            (
                "~/a/notes.txt",
                "source = \"s\"\nmerge = true",
                ".json, .toml",
            ),
            (
                "~/a/config.toml",
                "source = \"s\"\nmerge = false",
                "must be true",
            ),
            (
                "~/a/config.toml",
                "block = \"x\"\nmerge = true",
                "block or line",
            ),
            (
                "~/a/config.toml",
                "line = \"x\"\nmerge = true",
                "block or line",
            ),
            (
                "~/a/config.toml",
                "source = \"s\"\nmerge = true\ncomment = \"#\"",
                "do not apply",
            ),
            (
                "~/a/config.toml",
                "source = \"s\"\nmerge = true\ntemplate = \"jinja\"",
                "unknown template engine",
            ),
        ] {
            let err = resolve(path, entry).unwrap_err().to_string();
            assert!(err.contains(reason), "{entry}: {err}");
        }
    }

    #[test]
    fn incoming_merge_entries_are_checked_like_the_edit_parser_reads_them() {
        let check = |key: &str, entry: &str| {
            let value: toml::Value = toml::from_str(entry).unwrap();
            validate_incoming_merge(key, &value, Path::new("/cfg/mise.toml"))
        };
        assert!(
            check(
                "~/a/settings.json/shared",
                "source = \"s.json\"\nmerge = true"
            )
            .is_ok()
        );
        for (key, entry, reason) in [
            (
                "~/a/settings.json/shared",
                "source = \"s\"\nmerge = true\nexclude = []",
                "exclude applies to whole-file",
            ),
            (
                "~/a/notes.txt/shared",
                "source = \"s\"\nmerge = true",
                ".json, .toml",
            ),
            (
                "~/a/settings.json/shared",
                "source = \"s\"\nmerge = false",
                "must be true",
            ),
        ] {
            let err = check(key, entry).unwrap_err().to_string();
            assert!(err.contains(reason), "{entry}: {err}");
        }
        let err = check(
            &format!("{OUTSIDE_HOME}/settings.json/shared"),
            "merge = true",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("source is required"), "{err}");
    }

    #[test]
    fn a_merge_table_is_an_edit_not_a_whole_file_entry() {
        let value: toml::Value = toml::from_str("source = \"s.toml\"\nmerge = true").unwrap();
        assert!(edit_entry_from_toml("~/a/config.toml/shared", value).is_some());
        let value: toml::Value = toml::from_str("source = \"s.toml\"").unwrap();
        assert!(edit_entry_from_toml("~/a/config.toml", value).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn merge_targets_are_the_same_only_when_provably_so() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let (a, b) = (dir.path().join("a.toml"), dir.path().join("b.toml"));
        file::write(&a, "x = 1\n")?;
        file::write(&b, "x = 1\n")?;
        assert!(same_target(&a, &a));
        assert!(!same_target(&a, &b));
        // a hard link is the same file
        let link = dir.path().join("link.toml");
        std::fs::hard_link(&a, &link)?;
        assert!(same_target(&a, &link));
        // missing targets: equal paths are the same
        let missing = dir.path().join("m.toml");
        assert!(same_target(&missing, &missing));
        // two spellings of one existing directory, same missing file name
        let real = dir.path().join("real");
        file::create_dir_all(&real)?;
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias)?;
        assert!(same_target(&real.join("new.toml"), &alias.join("new.toml")));
        assert!(!same_target(
            &real.join("new.toml"),
            &alias.join("other.toml")
        ));
        Ok(())
    }

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
