//! `[dotfiles]` — declarative config files (dotfiles) applied by
//! `mise dotfiles apply` or `mise bootstrap`.
//!
//! Entries are keyed by target path and point at a source file or directory,
//! resolved relative to the config file that declares them:
//!
//! ```toml
//! [dotfiles]
//! "~/.zshrc" = {}                                        # implied source
//! "~/.gitconfig" = "dotfiles/gitconfig"                  # explicit source
//! "~/.config/foo.toml" = { mode = "copy" }               # implied source
//! "~/.ssh/config" = { source = "ssh.tmpl", mode = "template" }
//! "~/.config/nvim" = "dotfiles/nvim"                     # symlink the dir itself
//! "~/.local/bin" = { source = "bin", mode = "symlink-each" }
//! ```
//!
//! Like `[bootstrap.packages]`, entries merge across the config hierarchy
//! (global -> local, local overrides by target key) and are only ever
//! applied by an explicit command, never implicitly.

use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use indexmap::IndexMap;
use itertools::Itertools;
use regex::Regex;
use serde::Deserialize;

use crate::config::{Config, ConfigMap, Settings};
use crate::dirs;
use crate::file;
use crate::path::PathExt;
use crate::ui::prompt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    /// symlink the target to the source — a file or the directory itself
    Symlink,
    /// source is a directory: recreate its directory structure under the
    /// target and symlink each file individually, so the target directory
    /// can also hold files mise doesn't manage
    SymlinkEach,
    /// copy the source file (or directory, recursively)
    Copy,
    /// render the source through the mise template engine and write the
    /// result (permissions are taken from the source file)
    Template,
}

impl FileMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "symlink" => Some(Self::Symlink),
            "symlink-each" => Some(Self::SymlinkEach),
            "copy" => Some(Self::Copy),
            "template" => Some(Self::Template),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Symlink => "symlink",
            Self::SymlinkEach => "symlink-each",
            Self::Copy => "copy",
            Self::Template => "template",
        }
    }
}

/// one `[dotfiles]` whole-file entry as written in mise.toml
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FileTomlEntry {
    /// `"~/.gitconfig" = "dotfiles/gitconfig"`
    Source(String),
    /// `"~/.gitconfig" = { source = "...", mode = "..." }` — both fields are
    /// optional so `{}` can mean implied source/default mode. Mode stays a
    /// string here so configs using modes from newer mise versions still
    /// parse (they warn and are skipped, like unknown package managers)
    Table {
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        mode: Option<String>,
    },
}

/// one file entry, resolved against the config file that declared it
#[derive(Debug, Clone)]
pub struct FileRequest {
    /// target path as written in config (display/merge key)
    pub target_raw: String,
    /// absolute target path (`~` expanded)
    pub target: PathBuf,
    /// absolute source path (relative sources resolve against the config
    /// file's directory; omitted sources resolve under dotfiles.root)
    pub source: PathBuf,
    pub mode: FileMode,
    /// directory of the declaring config file — base dir for template
    /// functions like `exec` and `read_file`
    pub base: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FileState {
    Applied,
    Missing,
    /// target exists but doesn't match — the reason is human-readable
    Differs(String),
    SourceMissing,
}

/// Aggregate whole-file `[dotfiles]` entries across all loaded config files.
/// Keys union global -> local; a more local config overrides an entry for the
/// same target. Malformed entries and unknown modes warn and are skipped.
pub fn files_from_config(config: &Config) -> Vec<FileRequest> {
    files_from_config_files(&config.config_files)
}

/// Aggregate `[dotfiles]` across a specific set of config files. This is
/// used by OCI builds, which intentionally scope config to project files by
/// default instead of blindly inheriting global dotfiles.
pub fn files_from_config_files(config_files: &ConfigMap) -> Vec<FileRequest> {
    // keyed by the *expanded* target so "~/.gitconfig" in one config and
    // its absolute spelling in another are one entry, not two
    let mut merged: IndexMap<PathBuf, FileRequest> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for (path, cf) in config_files.iter().rev() {
        let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let Some(dotfiles) = cf.dotfiles_config() else {
            continue;
        };
        for (target_raw, value) in dotfiles.0 {
            let Some(entry) = file_entry_from_toml(&target_raw, value) else {
                continue;
            };
            merge_file_entry(target_raw, entry, &base, &mut merged);
        }
    }
    merged.into_values().collect()
}

fn file_entry_from_toml(target_raw: &str, value: toml::Value) -> Option<FileTomlEntry> {
    match &value {
        toml::Value::String(_) => {}
        toml::Value::Table(table)
            if table.is_empty()
                || table.contains_key("mode")
                || (table.contains_key("source")
                    && !table.contains_key("block")
                    && !table.contains_key("line")
                    && !table.contains_key("template")
                    && !table.contains_key("comment")) => {}
        toml::Value::Table(_) => return None,
        _ => {
            warn!("[dotfiles].\"{target_raw}\": expected string or table entry, ignoring entry");
            return None;
        }
    }
    match value.try_into() {
        Ok(entry) => Some(entry),
        Err(err) => {
            warn!("[dotfiles].\"{target_raw}\": invalid file entry: {err}");
            None
        }
    }
}

fn merge_file_entry(
    target_raw: String,
    entry: FileTomlEntry,
    base: &Path,
    merged: &mut IndexMap<PathBuf, FileRequest>,
) {
    let (source, mode) = match entry {
        FileTomlEntry::Source(source) => (Some(source), None),
        FileTomlEntry::Table { source, mode } => (source, mode),
    };
    let mode = match mode.as_deref() {
        None => default_mode(),
        Some(m) => match FileMode::parse(m) {
            Some(m) => m,
            None => {
                warn!("[dotfiles].\"{target_raw}\": unknown mode '{m}', ignoring entry");
                return;
            }
        },
    };
    let target = file::replace_path(&target_raw);
    if target.is_relative() {
        warn!(
            "[dotfiles].\"{target_raw}\": target must be absolute or start with ~/, ignoring entry"
        );
        return;
    }
    let source = match source {
        Some(source) => {
            let source = file::replace_path(&source);
            if source.is_relative() {
                base.join(source)
            } else {
                source
            }
        }
        None => match implied_source(&target) {
            Ok(source) => source,
            Err(err) => {
                warn!("[dotfiles].\"{target_raw}\": {err}, ignoring entry");
                return;
            }
        },
    };
    for req in expand_request(target_raw, target, source, mode, base.to_path_buf()) {
        merged.insert(req.target.clone(), req);
    }
}

pub fn default_mode() -> FileMode {
    let settings = Settings::get();
    let mode = settings.dotfiles.default_mode.as_str();
    match FileMode::parse(mode) {
        Some(mode) => mode,
        None => {
            warn!("dotfiles.default_mode: unknown mode '{mode}', using symlink");
            FileMode::Symlink
        }
    }
}

pub fn dotfiles_root() -> PathBuf {
    file::replace_path(&Settings::get().dotfiles.root)
}

pub fn implied_source(target: &Path) -> Result<PathBuf> {
    let home: &Path = &dirs::HOME;
    let rel = target.strip_prefix(home).map_err(|_| {
        eyre::eyre!(
            "source is required for targets outside $HOME: {}",
            target.display_user()
        )
    })?;
    if rel.as_os_str().is_empty() {
        bail!("source is required for the home directory itself");
    }
    Ok(dotfiles_root().join(rel))
}

pub fn source_is_implied(req: &FileRequest) -> bool {
    match implied_source(&req.target) {
        Ok(source) => source == req.source,
        Err(_) => false,
    }
}

pub fn resolve_target_arg(target: &str) -> PathBuf {
    file::replace_path(target)
}

pub fn matches_target(req_target: &Path, req_raw: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == req_raw || {
                let resolved = resolve_target_arg(filter);
                resolved == req_target
            }
        })
}

pub fn copy_path(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    if source.is_dir() {
        if target.exists() && !target.is_dir() {
            remove_existing(target)?;
        }
        file::create_dir_all(target)?;
        file::copy_dir_all(source, target)?;
    } else {
        if target.is_symlink() {
            file::remove_file(target)?;
        }
        file::copy(source, target)?;
    }
    Ok(())
}

fn expand_request(
    target_raw: String,
    target: PathBuf,
    source: PathBuf,
    mode: FileMode,
    base: PathBuf,
) -> Vec<FileRequest> {
    if !is_glob_pattern(&source) {
        return vec![FileRequest {
            target_raw,
            target,
            source,
            mode,
            base,
        }];
    }

    let source_pattern = source.to_string_lossy().to_string();
    let matches = match glob::glob(&source_pattern) {
        Ok(paths) => paths
            .filter_map(|path| match path {
                Ok(path) => Some(path),
                Err(err) => {
                    warn!(
                        "[dotfiles].\"{target_raw}\": error reading source pattern {source_pattern}: {err}"
                    );
                    None
                }
            })
            .sorted()
            .collect_vec(),
        Err(err) => {
            warn!("[dotfiles].\"{target_raw}\": invalid source pattern: {err}");
            return vec![];
        }
    };
    if matches.is_empty() {
        warn!("[dotfiles].\"{target_raw}\": source pattern matched no files, ignoring entry");
        return vec![];
    }

    let target_pattern = target.to_string_lossy().to_string();
    if !is_glob_pattern(&target) {
        if matches.len() > 1 {
            warn!(
                "[dotfiles].\"{target_raw}\": source pattern matched multiple paths but target has no wildcard, ignoring entry"
            );
            return vec![];
        }
        return vec![FileRequest {
            target_raw,
            target,
            source: matches[0].clone(),
            mode,
            base,
        }];
    }

    matches
        .into_iter()
        .filter_map(|matched_source| {
            let captures = match wildcard_captures(&source_pattern, &matched_source) {
                Ok(captures) => captures,
                Err(err) => {
                    warn!("[dotfiles].\"{target_raw}\": {err}");
                    return None;
                }
            };
            let Some(target_path) = expand_target_pattern(&target_pattern, &captures) else {
                warn!(
                    "[dotfiles].\"{target_raw}\": target wildcard count does not match source pattern, ignoring {}",
                    matched_source.display_user()
                );
                return None;
            };
            Some(FileRequest {
                target_raw: target_path.display_user().to_string(),
                target: target_path,
                source: matched_source,
                mode,
                base: base.clone(),
            })
        })
        .collect()
}

fn is_glob_pattern(path: &Path) -> bool {
    path.to_string_lossy()
        .chars()
        .any(|c| matches!(c, '*' | '?' | '['))
}

fn wildcard_captures(pattern: &str, path: &Path) -> Result<Vec<String>> {
    let path = normalize_path_separators(&path.to_string_lossy());
    let re = wildcard_regex(pattern)?;
    let Some(captures) = re.captures(&path) else {
        bail!("source pattern did not match {path}");
    };
    Ok((1..captures.len())
        .map(|i| {
            captures
                .get(i)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        })
        .collect())
}

fn wildcard_regex(pattern: &str) -> Result<Regex> {
    let mut re = String::from("^");
    let pattern = normalize_path_separators(pattern);
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    // Glob `**/` can match zero directories. Capture the
                    // directory body without the trailing slash so target
                    // expansion can omit the slash when the capture is empty.
                    re.push_str("(?:(.*)/)?");
                } else {
                    re.push_str("(.*)");
                }
            }
            '*' => re.push_str("([^/]*)"),
            '?' => re.push_str("([^/])"),
            '[' => {
                let Some(class) = read_glob_class(&mut chars) else {
                    re.push_str("\\[");
                    continue;
                };
                re.push('(');
                re.push_str(&class);
                re.push(')');
            }
            _ => re.push_str(&regex::escape(&ch.to_string())),
        }
    }
    re.push('$');
    Ok(Regex::new(&re)?)
}

fn expand_target_pattern(pattern: &str, captures: &[String]) -> Option<PathBuf> {
    let mut out = String::new();
    let pattern = normalize_path_separators(pattern);
    let mut chars = pattern.chars().peekable();
    let mut captures = captures.iter();
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                let capture = normalize_path_separators(captures.next()?);
                if chars.peek() == Some(&'/') {
                    chars.next();
                    if !capture.is_empty() {
                        out.push_str(&capture);
                        out.push('/');
                    }
                } else {
                    out.push_str(&capture);
                }
            }
            '*' => out.push_str(&normalize_path_separators(captures.next()?)),
            '?' => out.push_str(&normalize_path_separators(captures.next()?)),
            '[' => {
                read_glob_class(&mut chars)?;
                out.push_str(&normalize_path_separators(captures.next()?));
            }
            _ => out.push(ch),
        }
    }
    if captures.next().is_some() {
        return None;
    }
    Some(PathBuf::from(native_path_separators(&out)))
}

fn normalize_path_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn native_path_separators(path: &str) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        path.to_string()
    } else {
        path.replace('/', std::path::MAIN_SEPARATOR_STR)
    }
}

fn read_glob_class<I>(chars: &mut std::iter::Peekable<I>) -> Option<String>
where
    I: Iterator<Item = char>,
{
    let mut class = String::from("[");
    if chars.peek() == Some(&'!') {
        chars.next();
        class.push('^');
    }
    for ch in chars.by_ref() {
        class.push(ch);
        if ch == ']' {
            return Some(class);
        }
    }
    None
}

/// Current state of one entry on this machine.
///
/// Note: computing a template entry's state requires rendering it, so this
/// runs the template engine — including `exec()` — from `mise dotfiles
/// status`. That's the same trust model as `[env]` templates (which run on
/// every command in a trusted config); only `--dry-run` promises to execute
/// nothing and therefore skips template checks entirely.
pub fn check(config: &Config, req: &FileRequest) -> Result<FileState> {
    if !req.source.exists() {
        return Ok(FileState::SourceMissing);
    }
    // render at most once per call — templates may use exec()
    let rendered = match req.mode {
        FileMode::Template => Some(render_template(config, req)?),
        _ => None,
    };
    check_rendered(req, rendered.as_deref())
}

/// [`check`] with template output already rendered, so callers that go on to
/// write the file render only once (templates may use `exec()`, which must
/// not run more often than necessary)
fn check_rendered(req: &FileRequest, rendered: Option<&str>) -> Result<FileState> {
    match req.mode {
        FileMode::Symlink => check_symlink(&req.source, &req.target),
        FileMode::SymlinkEach => check_symlink_each(req),
        FileMode::Copy => check_copy(&req.source, &req.target),
        FileMode::Template => {
            let state = check_content(
                &req.target,
                rendered.expect("rendered template content").as_bytes(),
            )?;
            // templates promise the source file's permissions — repair
            // drift (e.g. a later chmod), not just content
            #[cfg(unix)]
            if state == FileState::Applied {
                use std::os::unix::fs::PermissionsExt;
                let mode_of =
                    |p: &Path| -> Result<u32> { Ok(p.metadata()?.permissions().mode() & 0o7777) };
                if mode_of(&req.source)? != mode_of(&req.target)? {
                    return Ok(FileState::Differs("permissions differ".into()));
                }
            }
            Ok(state)
        }
    }
}

fn check_symlink(source: &Path, target: &Path) -> Result<FileState> {
    // on Windows file "symlinks" are copies (see `link_file`)
    if cfg!(windows) && source.is_file() {
        return check_copy(source, target);
    }
    if target.is_symlink() {
        let dest = std::fs::read_link(target)?;
        if dest == *source || points_at_same_file(target, source) {
            Ok(FileState::Applied)
        } else {
            Ok(FileState::Differs(format!(
                "symlink points to {}",
                dest.display_user()
            )))
        }
    } else if target.exists() {
        Ok(FileState::Differs("exists but is not a symlink".into()))
    } else {
        Ok(FileState::Missing)
    }
}

fn points_at_same_file(target: &Path, source: &Path) -> bool {
    match (target.canonicalize(), source.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn check_symlink_each(req: &FileRequest) -> Result<FileState> {
    if !req.source.is_dir() {
        // callers add the entry's context (status row / apply error list)
        bail!(
            "mode symlink-each requires the source to be a directory: {}",
            req.source.display_user()
        );
    }
    let files = walk_source_files(req)?;
    // with no files to link the desired state is just the target directory —
    // a blocking non-directory must still surface (and be --force-able)
    if files.is_empty() {
        return if req.target.is_dir() {
            Ok(FileState::Applied)
        } else if req.target.exists() || req.target.is_symlink() {
            Ok(FileState::Differs("exists but is not a directory".into()))
        } else {
            Ok(FileState::Missing)
        };
    }
    let mut applied = 0;
    let mut missing = 0;
    let mut differs: Option<String> = None;
    for (source, target) in files {
        match check_symlink(&source, &target)? {
            FileState::Applied => applied += 1,
            FileState::Missing => missing += 1,
            FileState::Differs(reason) => {
                differs.get_or_insert(format!("{}: {reason}", target.display_user()));
            }
            FileState::SourceMissing => unreachable!("walked from source"),
        }
    }
    if let Some(reason) = differs {
        Ok(FileState::Differs(reason))
    } else if missing == 0 {
        Ok(FileState::Applied)
    } else if applied == 0 {
        Ok(FileState::Missing)
    } else {
        Ok(FileState::Differs(format!(
            "{applied} file(s) linked, {missing} missing"
        )))
    }
}

fn check_copy(source: &Path, target: &Path) -> Result<FileState> {
    if source.is_dir() {
        if !target.exists() {
            return Ok(FileState::Missing);
        }
        if !target.is_dir() {
            return Ok(FileState::Differs("exists but is not a directory".into()));
        }
        for entry in walkdir::WalkDir::new(source) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(source)?;
            match check_content(&target.join(rel), &file::read(entry.path())?)? {
                FileState::Applied => {}
                _ => return Ok(FileState::Differs(format!("{} differs", rel.display()))),
            }
        }
        Ok(FileState::Applied)
    } else {
        check_content(target, &file::read(source)?)
    }
}

fn check_content(target: &Path, expected: &[u8]) -> Result<FileState> {
    // copy/template targets must be real files; a symlink — dangling or
    // live — gets replaced (re-pointing/replacing symlinks needs no --force)
    if target.is_symlink() {
        return Ok(FileState::Differs("exists but is a symlink".into()));
    }
    if !target.exists() {
        return Ok(FileState::Missing);
    }
    if target.is_dir() {
        return Ok(FileState::Differs("exists but is a directory".into()));
    }
    if file::read(target)? == expected {
        Ok(FileState::Applied)
    } else {
        Ok(FileState::Differs("content differs".into()))
    }
}

pub fn render_template(config: &Config, req: &FileRequest) -> Result<String> {
    let raw = file::read_to_string(&req.source)?;
    let mut tera = crate::tera::get_tera(Some(&req.base));
    let rendered = tera.render_str(&raw, &config.tera_ctx).map_err(|err| {
        eyre::eyre!(
            "[dotfiles].\"{}\": failed to render template {}: {err}",
            req.target_raw,
            req.source.display_user()
        )
    })?;
    Ok(rendered)
}

/// directories a symlink-each entry needs: the target itself plus every
/// intermediate directory for nested source files
fn needed_dirs(req: &FileRequest) -> Result<Vec<PathBuf>> {
    let mut out = indexmap::IndexSet::new();
    out.insert(req.target.clone());
    for (_, target) in walk_source_files(req)? {
        let mut dir = target.parent();
        while let Some(d) = dir {
            if d == req.target {
                break;
            }
            out.insert(d.to_path_buf());
            dir = d.parent();
        }
    }
    Ok(out.into_iter().collect())
}

/// every (source file, target path) pair of a symlink-each entry
fn walk_source_files(req: &FileRequest) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut out = vec![];
    for entry in walkdir::WalkDir::new(&req.source).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        let rel = entry.path().strip_prefix(&req.source)?;
        out.push((entry.path().to_path_buf(), req.target.join(rel)));
    }
    Ok(out)
}

pub struct ApplyOpts {
    pub dry_run: bool,
    pub verbose: bool,
    /// replace conflicting targets (existing real files where a symlink
    /// should go, or type mismatches) instead of erroring
    pub force: bool,
    pub force_hint: &'static str,
    pub yes: bool,
}

/// Apply all entries that aren't already in the desired state. Conflicting
/// targets (a real file where a symlink should go, a directory where a file
/// should go) are an error unless `force` is set — content updates for
/// copy/template entries are not conflicts, overwriting is their job.
pub fn apply(config: &Config, requests: &[FileRequest], opts: &ApplyOpts) -> Result<()> {
    // pre-rendered template output rides along so it's written as compared,
    // and exec() in templates runs once per apply
    let mut todo: Vec<(&FileRequest, Option<String>)> = vec![];
    let mut missing_sources = vec![];
    let mut broken = vec![];
    let mut conflicts = vec![];
    for req in requests {
        // report every problem in one pass instead of fix-and-retry — a
        // render or check failure on one entry must not hide the rest
        if !req.source.exists() {
            missing_sources.push(format!(
                "  [dotfiles].\"{}\": {}",
                req.target_raw,
                req.source.display_user()
            ));
            continue;
        }
        // rendering can run exec() — a dry run must not execute anything,
        // so list template entries without computing their current state
        if opts.dry_run && req.mode == FileMode::Template {
            conflicts.extend(find_conflicts(req)?);
            todo.push((req, None));
            continue;
        }
        let rendered = match req.mode {
            FileMode::Template => match render_template(config, req) {
                Ok(rendered) => Some(rendered),
                // already carries the entry's context
                Err(err) => {
                    broken.push(format!("  {err}"));
                    continue;
                }
            },
            _ => None,
        };
        match check_rendered(req, rendered.as_deref()) {
            Ok(FileState::Applied) => continue,
            Ok(_) => {}
            Err(err) => {
                broken.push(format!("  [dotfiles].\"{}\": {err}", req.target_raw));
                continue;
            }
        }
        conflicts.extend(find_conflicts(req)?);
        todo.push((req, rendered));
    }
    let mut problems = vec![];
    if !missing_sources.is_empty() {
        problems.push(format!(
            "sources do not exist:\n{}",
            missing_sources.join("\n")
        ));
    }
    if !broken.is_empty() {
        problems.push(format!("entries with errors:\n{}", broken.join("\n")));
    }
    if !conflicts.is_empty() && !opts.force {
        problems.push(format!(
            "refusing to overwrite existing files ({}):\n{}",
            opts.force_hint,
            conflicts
                .iter()
                .map(|p| format!("  {}", p.display_user()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !problems.is_empty() {
        bail!("files: {}", problems.join("\nfiles: "));
    }
    if todo.is_empty() {
        info!("files: all files are applied");
        return Ok(());
    }
    if opts.dry_run {
        for (req, rendered) in &todo {
            // template state wasn't computed (no rendering on dry runs), so
            // the entry may already be converged
            let conditional = req.mode == FileMode::Template && rendered.is_none();
            let suffix = if conditional { " (if changed)" } else { "" };
            miseprintln!("{}{suffix}", describe(req)?);
            if opts.verbose && !conditional {
                print_diff(req, rendered.as_deref())?;
            }
        }
        return Ok(());
    }
    if !opts.yes && console::user_attended_stderr() {
        let list = todo
            .iter()
            .map(|(r, _)| r.target_raw.clone())
            .collect::<Vec<_>>()
            .join(", ");
        if !prompt::confirm(format!("files: apply {list}?"))? {
            info!("files: skipped");
            return Ok(());
        }
    }
    for (req, rendered) in &todo {
        apply_one(req, rendered.as_deref())?;
    }
    info!(
        "files: applied {}",
        todo.iter()
            .map(|(r, _)| r.target_raw.clone())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

/// existing paths this entry would have to delete or replace — not counting
/// content overwrites by copy/template (those are the declared intent) or
/// re-pointing symlinks (always mise-owned territory)
fn find_conflicts(req: &FileRequest) -> Result<Vec<PathBuf>> {
    let regular_files_have_same_content = |source: &Path, target: &Path| -> Result<bool> {
        if !source.is_file() || !target.is_file() {
            return Ok(false);
        }
        if source.metadata()?.len() != target.metadata()?.len() {
            return Ok(false);
        }
        Ok(file::read(source)? == file::read(target)?)
    };

    // on Windows, file "symlinks" are applied as copies (see `link_path`),
    // so existing regular-file targets are routine content updates there,
    // not conflicts — only a type mismatch blocks
    let file_link_conflicts = |source: &Path, target: &Path| -> Result<bool> {
        if cfg!(windows) && source.is_file() {
            Ok(target.exists() && target.is_dir())
        } else {
            if !target.exists() || target.is_symlink() {
                return Ok(false);
            }
            // If this equality check cannot read either side, keep the
            // conservative conflict path so --force can still handle it.
            Ok(!regular_files_have_same_content(source, target).unwrap_or(false))
        }
    };
    let mut out = vec![];
    match req.mode {
        FileMode::Symlink => {
            if file_link_conflicts(&req.source, &req.target)? {
                out.push(req.target.clone());
            }
        }
        FileMode::SymlinkEach => {
            // a regular file where the target directory (or a nested one)
            // should go blocks the whole entry
            for dir in needed_dirs(req)? {
                if dir.exists() && !dir.is_dir() {
                    out.push(dir);
                }
            }
            for (source, target) in walk_source_files(req)? {
                if file_link_conflicts(&source, &target)? {
                    out.push(target);
                }
            }
        }
        FileMode::Copy | FileMode::Template => {
            // a dir where a file should go (or vice versa) must be removed;
            // file-over-file is an ordinary overwrite
            if req.target.exists() && req.target.is_dir() != req.source.is_dir() {
                out.push(req.target.clone());
            }
        }
    }
    Ok(out)
}

fn describe(req: &FileRequest) -> Result<String> {
    let src = req.source.display_user();
    let tgt = req.target.display_user();
    Ok(match req.mode {
        FileMode::Symlink => format!("ln -sf {src} {tgt}"),
        FileMode::SymlinkEach => {
            format!(
                "ln -sf {src}/* into {tgt}/ ({} files)",
                walk_source_files(req)?.len()
            )
        }
        FileMode::Copy if req.source.is_dir() => format!("cp -r {src} {tgt}"),
        FileMode::Copy => format!("cp {src} {tgt}"),
        FileMode::Template => format!("render {src} -> {tgt}"),
    })
}

fn print_diff(req: &FileRequest, rendered: Option<&str>) -> Result<()> {
    match req.mode {
        FileMode::Symlink => {
            if req.target.is_symlink() {
                let dest = std::fs::read_link(&req.target)?;
                miseprintln!(
                    "  current symlink: {} -> {}",
                    req.target.display_user(),
                    dest.display_user()
                );
            } else if req.target.exists() {
                miseprintln!("  current: {} exists", req.target.display_user());
            } else {
                miseprintln!("  current: {} missing", req.target.display_user());
            }
            miseprintln!(
                "  desired symlink: {} -> {}",
                req.target.display_user(),
                req.source.display_user()
            );
        }
        FileMode::SymlinkEach => {
            miseprintln!(
                "  desired symlink-each: {} files from {}",
                walk_source_files(req)?.len(),
                req.source.display_user()
            );
        }
        FileMode::Copy | FileMode::Template if req.source.is_file() => {
            let desired = match req.mode {
                FileMode::Template => rendered.unwrap_or_default().as_bytes().to_vec(),
                _ => file::read(&req.source)?,
            };
            let current = if req.target.exists() && req.target.is_file() {
                file::read(&req.target)?
            } else {
                vec![]
            };
            if current != desired {
                miseprintln!(
                    "  content differs: {} -> {}",
                    req.source.display_user(),
                    req.target.display_user()
                );
            }
        }
        FileMode::Copy | FileMode::Template => {
            miseprintln!(
                "  desired directory contents: {} -> {}",
                req.source.display_user(),
                req.target.display_user()
            );
        }
    }
    Ok(())
}

fn apply_one(req: &FileRequest, rendered: Option<&str>) -> Result<()> {
    debug!("files: {}", describe(req)?);
    if let Some(parent) = req.target.parent() {
        file::create_dir_all(parent)?;
    }
    match req.mode {
        FileMode::Symlink => {
            remove_existing(&req.target)?;
            link_path(&req.source, &req.target)?;
        }
        FileMode::SymlinkEach => {
            // conflicts were vetted (or --force given): clear anything
            // blocking a directory we need
            for dir in needed_dirs(req)? {
                if dir.exists() && !dir.is_dir() {
                    remove_existing(&dir)?;
                }
            }
            // even an empty source dir must produce the target dir, or the
            // entry would never converge
            file::create_dir_all(&req.target)?;
            for (source, target) in walk_source_files(req)? {
                if check_symlink(&source, &target)? == FileState::Applied {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    file::create_dir_all(parent)?;
                }
                remove_existing(&target)?;
                link_path(&source, &target)?;
            }
        }
        FileMode::Copy => {
            if req.source.is_dir() {
                // additive: overwrite matching files, leave files mise
                // doesn't manage in place — only a type mismatch (vetted
                // as a conflict) removes the target
                if req.target.exists() && !req.target.is_dir() {
                    remove_existing(&req.target)?;
                }
                // even an empty source dir must produce the target dir,
                // or the entry would never converge
                file::create_dir_all(&req.target)?;
                // per-file instead of copy_dir_all so a symlink at a
                // destination is replaced, not written through
                for (source, target) in walk_source_files(req)? {
                    if let Some(parent) = target.parent() {
                        file::create_dir_all(parent)?;
                    }
                    if target.is_symlink() {
                        file::remove_file(&target)?;
                    }
                    file::copy(&source, &target)?;
                }
            } else {
                remove_existing(&req.target)?;
                file::copy(&req.source, &req.target)?;
            }
        }
        FileMode::Template => {
            let rendered = rendered.expect("rendered template content");
            remove_existing(&req.target)?;
            file::write(&req.target, rendered)?;
            #[cfg(unix)]
            std::fs::set_permissions(&req.target, req.source.metadata()?.permissions())?;
        }
    }
    Ok(())
}

/// remove whatever sits at `path` so it can be replaced — conflicts have
/// already been vetted (or --force given) by the time this runs
fn remove_existing(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        file::remove_file(path)?;
    } else if path.is_dir() {
        file::remove_all(path)?;
    }
    Ok(())
}

fn link_path(source: &Path, target: &Path) -> Result<()> {
    if cfg!(windows) && source.is_file() {
        // Windows file symlinks require elevation; junctions only work for
        // directories — fall back to a copy like make_symlink_or_copy does
        file::copy(source, target)?;
    } else {
        file::make_symlink(source, target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_mode_parse() {
        assert_eq!(FileMode::parse("symlink"), Some(FileMode::Symlink));
        assert_eq!(FileMode::parse("symlink-each"), Some(FileMode::SymlinkEach));
        assert_eq!(FileMode::parse("copy"), Some(FileMode::Copy));
        assert_eq!(FileMode::parse("template"), Some(FileMode::Template));
        assert_eq!(FileMode::parse("hardlink"), None);
    }

    #[test]
    fn test_wildcard_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/*.toml",
            Path::new("/repo/dotfiles/config/starship.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/*.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/starship.toml"));
    }

    #[test]
    fn test_recursive_wildcard_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/**/*.toml",
            Path::new("/repo/dotfiles/config/a/b/tool.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/**/*.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/a/b/tool.toml"));
    }

    #[test]
    fn test_recursive_wildcard_matches_zero_directories() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/**/*.toml",
            Path::new("/repo/dotfiles/config/tool.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/**/*.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/tool.toml"));
    }

    #[test]
    fn test_question_mark_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/app?.toml",
            Path::new("/repo/dotfiles/config/app1.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/app?.toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/app1.toml"));
    }

    #[test]
    fn test_character_class_target_expansion() {
        let captures = wildcard_captures(
            "/repo/dotfiles/config/theme-[ab].toml",
            Path::new("/repo/dotfiles/config/theme-a.toml"),
        )
        .unwrap();
        let target = expand_target_pattern("/home/me/.config/theme-[ab].toml", &captures).unwrap();
        assert_eq!(target, PathBuf::from("/home/me/.config/theme-a.toml"));
    }

    #[test]
    fn test_windows_separator_wildcard_expansion() {
        let captures = wildcard_captures(
            r"C:\repo\dotfiles\config\*.toml",
            Path::new(r"C:\repo\dotfiles\config\starship.toml"),
        )
        .unwrap();
        let target = expand_target_pattern(r"C:\Users\me\.config\*.toml", &captures).unwrap();
        assert_eq!(
            target,
            PathBuf::from(native_path_separators("C:/Users/me/.config/starship.toml"))
        );
    }

    #[test]
    fn test_windows_separator_recursive_wildcard_expansion() {
        let captures = wildcard_captures(
            r"C:\repo\dotfiles\config\**\*.toml",
            Path::new(r"C:\repo\dotfiles\config\tool.toml"),
        )
        .unwrap();
        let target = expand_target_pattern(r"C:\Users\me\.config\**\*.toml", &captures).unwrap();
        assert_eq!(
            target,
            PathBuf::from(native_path_separators("C:/Users/me/.config/tool.toml"))
        );
    }
}
