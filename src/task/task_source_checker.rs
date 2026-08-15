use crate::config::{Config, Settings};
use crate::dirs;
use crate::file::{self, display_path};
use crate::hash;
use crate::rand::random_string;
use crate::task::Task;
use eyre::{Result, bail};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use glob::glob;
use ignore::overrides::{Override, OverrideBuilder};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// Remove mise's automatic output before rerunning a task so an earlier
/// success cannot make a failed attempt look fresh.
pub async fn remove_auto_output(task: &Task, config: &Arc<Config>) -> Result<()> {
    if !task.outputs.is_auto() {
        return Ok(());
    }
    let root = task_cwd(task, config).await?;
    for output in task.outputs.paths(task, &root) {
        match fs::remove_file(&output) {
            Ok(()) => debug!("removed auto output file: {output}"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

/// Check if a path is a glob pattern
pub fn is_glob_pattern(path: &str) -> bool {
    // This is the character set used for glob detection by glob
    let glob_chars = ['*', '{', '}'];
    path.chars().any(|c| glob_chars.contains(&c))
}

const MAX_BRACE_EXPANSIONS: usize = 1024;

/// Expand globset-style brace alternates before passing a pattern to `glob`.
///
/// `Override`/globset understands patterns such as `{a,b}.txt`, but the `glob`
/// crate used to enumerate task sources and outputs treats braces literally.
/// Expanding only groups with comma-separated choices lets both stages share
/// the same syntax without returning to a recursive filesystem walker. Balanced
/// braces without a comma are encoded as character classes so they keep their
/// literal meaning in both `glob` and globset.
pub(crate) fn expand_glob_braces(pattern: &str) -> Result<Vec<String>> {
    struct BraceGroup {
        start: usize,
        end: usize,
        branches: Vec<(usize, usize)>,
    }

    fn find_group(pattern: &str) -> Result<Option<BraceGroup>> {
        let mut escaped = false;
        let mut class_depth = 0;
        let mut group_start = None;
        let mut group_depth = 0;
        let mut branch_start = 0;
        let mut branches = Vec::new();

        for (idx, ch) in pattern.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            // On Windows, backslashes are path separators rather than glob
            // escapes. Literal braces can still be expressed with a character
            // class, e.g. `[{]` and `[}]`.
            if ch == '\\' && !cfg!(windows) {
                escaped = true;
                continue;
            }
            match ch {
                '[' if class_depth == 0 => class_depth = 1,
                ']' if class_depth == 1 => class_depth = 0,
                '{' if class_depth == 0 => {
                    if group_depth == 0 {
                        group_start = Some(idx);
                        branch_start = idx + ch.len_utf8();
                    }
                    group_depth += 1;
                }
                ',' if class_depth == 0 && group_depth == 1 => {
                    branches.push((branch_start, idx));
                    branch_start = idx + ch.len_utf8();
                }
                '}' if class_depth == 0 => {
                    if group_depth == 0 {
                        bail!("unopened brace alternate in glob pattern {pattern:?}");
                    }
                    group_depth -= 1;
                    if group_depth == 0 {
                        let start = group_start.unwrap();
                        if !branches.is_empty() {
                            branches.push((branch_start, idx));
                            return Ok(Some(BraceGroup {
                                start,
                                end: idx,
                                branches,
                            }));
                        }

                        // A balanced group without a top-level comma is a
                        // literal brace pair. It may still contain a nested
                        // alternate, so look inside it before continuing with
                        // the remainder of the pattern.
                        if let Some(mut nested) = find_group(&pattern[start + 1..idx])? {
                            let offset = start + 1;
                            nested.start += offset;
                            nested.end += offset;
                            for (branch_start, branch_end) in &mut nested.branches {
                                *branch_start += offset;
                                *branch_end += offset;
                            }
                            return Ok(Some(nested));
                        }
                        group_start = None;
                        branches.clear();
                    }
                }
                _ => {}
            }
        }
        if group_depth != 0 {
            bail!("unclosed brace alternate in glob pattern {pattern:?}");
        }
        Ok(None)
    }

    fn expand(pattern: &str, expanded: &mut Vec<String>) -> Result<()> {
        let Some(group) = find_group(pattern)? else {
            if expanded.len() >= MAX_BRACE_EXPANSIONS {
                bail!(
                    "glob pattern expands to more than {MAX_BRACE_EXPANSIONS} alternatives: {pattern:?}"
                );
            }
            let mut literal = String::with_capacity(pattern.len());
            let mut escaped = false;
            let mut class_depth = 0;
            for ch in pattern.chars() {
                if escaped {
                    escaped = false;
                    literal.push(ch);
                    continue;
                }
                if ch == '\\' && !cfg!(windows) {
                    escaped = true;
                    literal.push(ch);
                    continue;
                }
                match ch {
                    '[' if class_depth == 0 => {
                        class_depth = 1;
                        literal.push(ch);
                    }
                    ']' if class_depth == 1 => {
                        class_depth = 0;
                        literal.push(ch);
                    }
                    '{' if class_depth == 0 => literal.push_str("[{]"),
                    '}' if class_depth == 0 => literal.push_str("[}]"),
                    _ => literal.push(ch),
                }
            }
            expanded.push(literal);
            return Ok(());
        };

        let prefix = &pattern[..group.start];
        let suffix = &pattern[group.end + 1..];
        for (branch_start, branch_end) in group.branches {
            let branch = &pattern[branch_start..branch_end];
            // Match globset's default: empty alternatives are discarded.
            if branch.is_empty() {
                continue;
            }
            expand(&format!("{prefix}{branch}{suffix}"), expanded)?;
        }
        Ok(())
    }

    let mut expanded = Vec::new();
    expand(pattern, &mut expanded)?;
    Ok(expanded)
}

/// Build an [`Override`] matcher for a task's `sources` patterns.
///
/// `match_root` is the directory the [`Override`] is anchored at (the workspace
/// root in workspace setups, otherwise the task CWD). `task_cwd` is the
/// directory the task actually runs from. When they differ — a subproject task
/// inside a workspace — relative patterns are prefixed with the
/// `task_cwd`-relative-to-`match_root` path so they remain correctly anchored.
/// Absolute patterns are stripped of the `match_root` prefix as before.
///
/// Patterns use gitignore syntax with `!` inverted (the [`Override`] convention):
/// a non-negated entry marks a file as a *source*, `!`-prefixed excludes it,
/// `\!` escapes a literal `!`, and order matters.
pub(crate) fn build_source_matcher(
    match_root: &Path,
    task_cwd: &Path,
    sources: &[String],
) -> Override {
    let mut builder = OverrideBuilder::new(match_root);
    for s in sources {
        let normalized = normalize_pattern(match_root, task_cwd, s);
        let expanded = match expand_glob_braces(&normalized) {
            Ok(expanded) => expanded,
            Err(e) => {
                // Source matcher construction is infallible to callers. An
                // invalid pattern is skipped so freshness falls back to stale
                // when no sources can be enumerated.
                warn!("invalid source pattern {s:?}: {e}");
                continue;
            }
        };
        for normalized in expanded {
            if let Err(e) = builder.add(&normalized) {
                warn!("invalid source pattern {s:?}: {e}");
            }
        }
    }
    builder.build().unwrap_or_else(|e| {
        warn!("failed to build source matcher: {e}");
        Override::empty()
    })
}

/// Normalise `pattern` so it is always expressed relative to `match_root`.
///
/// - **Absolute** body under `match_root`: strip the prefix.
/// - **Relative** body when `task_cwd` is a subdirectory of `match_root`:
///   prefix with `task_cwd`-relative-to-`match_root` so the pattern is
///   anchored at the workspace root rather than the subproject CWD.
///   E.g. `match_root=/ws`, `task_cwd=/ws/lib/worker`, `src/**/*.go`
///   → `lib/worker/src/**/*.go`.
/// - Everything else: returned unchanged.
fn normalize_pattern(match_root: &Path, task_cwd: &Path, pattern: &str) -> String {
    let (prefix, body) = if pattern.starts_with("\\!") {
        return pattern.to_string();
    } else if let Some(rest) = pattern.strip_prefix('!') {
        ("!", rest)
    } else {
        ("", pattern)
    };
    let body_path = Path::new(body);
    if body_path.is_absolute() {
        if let Ok(rel) = body_path.strip_prefix(match_root)
            && let Some(rel_str) = rel.to_str()
        {
            let rel_str = if rel_str.starts_with('!') {
                format!("\\{rel_str}")
            } else {
                rel_str.to_string()
            };
            return format!("{prefix}{rel_str}");
        }
        return pattern.to_string();
    }
    // Relative pattern: anchor it at match_root by prepending the subproject path.
    if let Ok(cwd_rel) = task_cwd.strip_prefix(match_root)
        && let Some(cwd_rel_str) = cwd_rel.to_str()
        && !cwd_rel_str.is_empty()
    {
        return format!("{prefix}{cwd_rel_str}/{body}");
    }
    pattern.to_string()
}

/// Returns true iff `path` is selected as a source by `matcher`. With
/// [`Override`]'s inverted semantics, a non-negated user pattern produces
/// `Match::Whitelist` for matching paths.
///
/// Absolute paths that don't fall under the matcher's root are out of
/// gitignore's domain — `Override::matched` would return `Match::None` and,
/// when positive patterns are present, promote that to `Match::Ignore`,
/// silently dropping a file the glob legitimately included. Trust the glob
/// in that case.
pub(crate) fn is_source(matcher: &Override, path: &Path) -> bool {
    if path.is_absolute() && !path.starts_with(matcher.path()) {
        return true;
    }
    matcher.matched(path, false).is_whitelist()
}

/// Returns the include-side glob patterns from `sources`, suitable for file
/// enumeration via `glob`. `!`-prefixed entries are dropped (they only
/// constrain matching, not enumeration); `\!`-prefixed entries have the
/// escape removed so they can be globbed as literal `!`-prefixed paths.
pub(crate) fn source_glob_patterns(sources: &[String]) -> Vec<String> {
    sources
        .iter()
        .filter_map(|s| {
            if s.starts_with('!') {
                None
            } else if let Some(rest) = s.strip_prefix("\\!") {
                Some(format!("!{rest}"))
            } else {
                Some(s.clone())
            }
        })
        .collect()
}

/// Build an ordered matcher for task output patterns.
///
/// Output entries use the same syntax as sources: `!` excludes, `\!` escapes
/// a literal leading bang, and the last matching pattern wins. Each pattern
/// also applies to descendants because a matched output directory is handled
/// recursively by freshness checks and artifact caching.
pub(crate) fn build_output_matcher(root: &Path, outputs: &[String]) -> Result<Override> {
    let mut builder = OverrideBuilder::new(root);
    for output in outputs {
        let output = normalize_pattern(root, root, output);
        // Output callers already propagate matcher errors and conservatively
        // treat the task as stale, so keep malformed patterns visible here.
        for output in expand_glob_braces(&output)? {
            builder.add(&output)?;
            let descendant = if let Some(body) = output.strip_prefix('!') {
                format!("!{body}/**")
            } else if let Some(body) = output.strip_prefix("\\!") {
                format!("\\!{body}/**")
            } else {
                format!("{output}/**")
            };
            if !output.ends_with("/**") {
                builder.add(&descendant)?;
            }
        }
    }
    Ok(builder.build()?)
}

/// Return the include-side output patterns used to enumerate output roots.
pub(crate) fn output_glob_patterns(outputs: &[String]) -> Vec<String> {
    source_glob_patterns(outputs)
}

/// Returns true when an output path is selected by the ordered matcher.
pub(crate) fn is_output(matcher: &Override, path: &Path, is_dir: bool) -> bool {
    if path.is_absolute() && !path.starts_with(matcher.path()) {
        return true;
    }
    matcher.matched(path, is_dir).is_whitelist()
}

fn resolve_task_path(root: &Path, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn normalize_task_cwd(path: PathBuf) -> PathBuf {
    let mut normalized: PathBuf = path
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect();
    if normalized.as_os_str().is_empty() && !path.as_os_str().is_empty() {
        normalized.push(".");
    }
    normalized
}

/// Get the working directory for a task
pub async fn task_cwd(task: &Task, config: &Arc<Config>) -> Result<PathBuf> {
    if let Some(d) = task.dir(config).await? {
        Ok(normalize_task_cwd(d))
    } else {
        Ok(config
            .project_root
            .clone()
            .or_else(|| dirs::CWD.clone())
            .unwrap_or_default())
    }
}

/// Return the outermost config root that contains the task working directory.
///
/// Source patterns are anchored here so workspace-rooted patterns and task-CWD
/// relative patterns use the same namespace.
pub(crate) fn task_source_match_root(root: &Path, config: &Config) -> PathBuf {
    config
        .config_files
        .values()
        .filter_map(|cf| cf.project_root())
        .filter(|pr| root.starts_with(pr) || *pr == root)
        .min_by_key(|p| p.components().count())
        .unwrap_or_else(|| root.to_path_buf())
}

/// Collect source file metadatas for a task, anchored at the correct workspace root.
async fn collect_source_metadatas(
    task: &Task,
    config: &Arc<Config>,
) -> Result<(PathBuf, PathBuf, Vec<(PathBuf, fs::Metadata)>)> {
    let root = task_cwd(task, config).await?;
    // Anchor the Override matcher at the outermost config root that is an
    // ancestor of the task CWD (i.e. the workspace root). This allows
    // workspace-rooted patterns like `{{ config_root }}/lib/**/*` to be
    // correctly relativized so that files inside a subproject directory are
    // not silently dropped.
    //
    // config.project_root cannot be used directly: BTreeMap iterates by
    // lexicographic path order, so a subproject config may be returned
    // before the workspace root config (mise.toml) even though
    // the workspace root has a shorter path.
    let match_root_owned = task_source_match_root(&root, config);
    let match_root = match_root_owned.as_path();
    let matcher = build_source_matcher(match_root, &root, &task.sources);
    let glob_patterns = source_glob_patterns(&task.sources);
    let mut source_metadatas = get_file_metadatas(&root, &glob_patterns, &matcher)?;
    // Always include every file that contributed to the task definition,
    // regardless of excludes — a stray `!mise.toml` must not silently
    // disable invalidation.
    for config_source in task.config_sources() {
        let config_path = if config_source.is_absolute() {
            config_source.to_path_buf()
        } else {
            root.join(config_source)
        };
        if let Ok(meta) = config_path.metadata()
            && meta.is_file()
            && !source_metadatas.iter().any(|(p, _)| p == &config_path)
        {
            source_metadatas.push((config_path, meta));
        }
    }
    Ok((root, match_root_owned, source_metadatas))
}

/// Compute the current source hash for a task. Returns `(hash, hash_file_path)`
/// or `None` if the task has no sources or no matching files were found.
async fn compute_source_hash(
    task: &Task,
    config: &Arc<Config>,
) -> Result<Option<(String, PathBuf)>> {
    if task.sources.is_empty() {
        return Ok(None);
    }
    let use_content_hash = Settings::get().task.source_freshness_hash_contents;
    let (root, _, source_metadatas) = collect_source_metadatas(task, config).await?;
    if source_metadatas.is_empty() {
        return Ok(None);
    }
    let source_hash = if use_content_hash {
        let cache_path = content_hash_cache_path(task, &root);
        let mut cache = load_content_hash_cache(&cache_path);
        let h = file_contents_to_hash(&source_metadatas, &mut cache)?;
        if let Err(e) = save_content_hash_cache(&cache_path, &cache) {
            trace!("failed to save content hash cache: {e}");
        }
        h
    } else {
        file_metadatas_to_hash(&source_metadatas)
    };
    let source_hash_path = sources_hash_path(task, &root, use_content_hash);
    Ok(Some((source_hash, source_hash_path)))
}

pub struct TaskCacheInputs {
    pub source_hash: String,
    pub source_paths: Vec<PathBuf>,
    pub root_identity: PathBuf,
}

/// Compute stable paths and content hashes for artifact-cache inputs in one source scan.
pub async fn task_cache_inputs(
    task: &Task,
    config: &Arc<Config>,
    persist_content_hash_cache: bool,
) -> Result<Option<TaskCacheInputs>> {
    if task.sources.is_empty() {
        return Ok(None);
    }
    let (root, match_root, mut source_metadatas) = collect_source_metadatas(task, config).await?;
    if source_metadatas.is_empty() {
        return Ok(None);
    }
    source_metadatas.sort_by(|(a, _), (b, _)| a.cmp(b));
    let cache_path = content_hash_cache_path(task, &root);
    let mut cache = load_content_hash_cache(&cache_path);
    let mut next = ContentHashCache::new();
    let mut hasher = blake3::Hasher::new();
    let mut source_paths = Vec::with_capacity(source_metadatas.len());
    for (path, metadata) in source_metadatas {
        let identity = match path.strip_prefix(&match_root) {
            Ok(relative) => format!("workspace\0{}", relative.to_string_lossy()),
            // Retaining the absolute path deliberately disables cross-checkout reuse for
            // sources outside the workspace instead of allowing ambiguous identities.
            Err(_) => format!("external\0{}", path.to_string_lossy()),
        };
        hasher.update(&(identity.len() as u64).to_le_bytes());
        hasher.update(identity.as_bytes());
        let contents = match cache.get(&path) {
            Some(entry) if cached_entry_matches(entry, &metadata) => entry.hash.clone(),
            _ => hash::file_hash_blake3(&path, None)?,
        };
        hasher.update(contents.as_bytes());
        next.insert(path.clone(), make_cache_entry(&metadata, contents));
        source_paths.push(path.strip_prefix(&root).unwrap_or(&path).to_path_buf());
    }
    cache = next;
    if persist_content_hash_cache && let Err(e) = save_content_hash_cache(&cache_path, &cache) {
        trace!("failed to save content hash cache: {e}");
    }
    let root_identity = root
        .strip_prefix(&match_root)
        .unwrap_or(&root)
        .to_path_buf();
    Ok(Some(TaskCacheInputs {
        source_hash: hasher.finalize().to_hex().to_string(),
        source_paths,
        root_identity,
    }))
}

/// Check if task sources are up to date (fresher than outputs)
pub async fn sources_are_fresh(task: &Task, config: &Arc<Config>) -> Result<bool> {
    if task.sources.is_empty() {
        return Ok(false);
    }
    let settings = Settings::get();
    let use_content_hash = settings.task.source_freshness_hash_contents;
    let equal_mtime_is_fresh = settings.task.source_freshness_equal_mtime_is_fresh;

    let run = async || -> Result<bool> {
        let (root, _, source_metadatas) = collect_source_metadatas(task, config).await?;

        // Check if sources resolved to no files (likely a config mistake)
        if source_metadatas.is_empty() {
            warn!(
                "task {} has sources defined but no matching files found",
                task.name
            );
            return Ok(false);
        }

        // Check for epoch timestamps (files extracted from tarballs without preserved timestamps)
        // These are considered stale since we can't trust the mtime.
        // Skipped in hash mode — content is the authority there, not timestamps.
        if !use_content_hash {
            for (path, metadata) in &source_metadatas {
                if let Ok(mtime) = metadata.modified()
                    && mtime == UNIX_EPOCH
                {
                    debug!(
                        "source file {} has epoch timestamp, treating as stale",
                        display_path(path)
                    );
                    return Ok(false);
                }
            }
        }

        let source_hash = if use_content_hash {
            let cache_path = content_hash_cache_path(task, &root);
            let mut cache = load_content_hash_cache(&cache_path);
            let h = file_contents_to_hash(&source_metadatas, &mut cache)?;
            if let Err(e) = save_content_hash_cache(&cache_path, &cache) {
                trace!("failed to save content hash cache: {e}");
            }
            h
        } else {
            file_metadatas_to_hash(&source_metadatas)
        };
        let source_hash_path = sources_hash_path(task, &root, use_content_hash);
        if let Some(dir) = source_hash_path.parent() {
            file::create_dir_all(dir)?;
        }
        let existing_hash = source_existing_hash(task, &root, use_content_hash);
        if existing_hash.as_deref().is_some_and(|h| h != source_hash) {
            debug!(
                "source {} hash mismatch in {}",
                if use_content_hash {
                    "content"
                } else {
                    "metadata"
                },
                source_hash_path.display()
            );
            // Do not write the hash here — the task is about to run. If it
            // fails, the baseline must stay at the previous value so the next
            // invocation still detects the mismatch. save_checksum writes the
            // hash after a successful run.
            return Ok(false);
        }
        if use_content_hash {
            // In hash mode, content alone determines freshness — no mtime check.
            // With no stored baseline there is nothing to compare the content
            // against, so the task is stale. Falling through to the mtime
            // comparison here would let an edit whose mtime is older than the
            // output masquerade as fresh — exactly what hash mode exists to
            // prevent — on the first run after the setting is enabled.
            if existing_hash.is_none() {
                debug!("no stored content hash in {}", source_hash_path.display());
                return Ok(false);
            }
            // Compare against the stored output hash to catch partial/missing outputs.
            let current_output_hash = compute_output_hash(task, &root)?;
            let stored_output_hash = output_existing_hash(task, &root);
            let fresh = current_output_hash.is_some()
                && current_output_hash.as_deref() == stored_output_hash.as_deref();
            file::write(&source_hash_path, &source_hash)?;
            return Ok(fresh);
        }
        let sources = get_last_modified_from_metadatas(&source_metadatas);
        let outputs = get_last_modified(&root, &task.outputs.paths(task, &root))?;
        trace!("sources: {sources:?}, outputs: {outputs:?}");
        let fresh = match (sources, outputs) {
            (Some(sources), Some(outputs)) => {
                if equal_mtime_is_fresh {
                    sources <= outputs
                } else {
                    sources < outputs
                }
            }
            _ => false,
        };
        if fresh {
            // Write a snapshot of the current hash so future checks can detect
            // source changes even when mtime would appear fresh (e.g. after a
            // touch or a cache restore).
            file::write(&source_hash_path, &source_hash)?;
        }
        Ok(fresh)
    };
    Ok(run().await.unwrap_or_else(|err| {
        warn!("sources_are_fresh: {err:?}");
        false
    }))
}

/// Save a checksum file after a task completes successfully
pub async fn save_checksum(task: &Task, config: &Arc<Config>) -> Result<()> {
    if task.sources.is_empty() {
        return Ok(());
    }
    let root = task_cwd(task, config).await?;
    if task.outputs.is_auto() {
        for p in task.outputs.paths(task, &root) {
            debug!("touching auto output file: {p}");
            file::touch_file(&PathBuf::from(&p))?;
        }
    } else {
        // Warn if any explicitly declared output was not generated.
        for output in output_glob_patterns(&task.outputs.paths(task, &root)) {
            let output_exists = if is_glob_pattern(&output) {
                expand_glob_braces(&output)
                    .map(|patterns| {
                        patterns.into_iter().any(|pattern| {
                            let pattern = resolve_task_path(&root, pattern);
                            glob(pattern.to_str().unwrap_or_default())
                                .map(|paths| paths.flatten().next().is_some())
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            } else {
                let path = Path::new(&output);
                let full_path = if path.is_relative() {
                    root.join(path)
                } else {
                    path.to_path_buf()
                };
                full_path.exists()
            };
            if !output_exists {
                warn!(
                    "task {} did not generate expected output: {}",
                    task.name, output
                );
            }
        }
    }
    // Persist the source hash now that the task has succeeded. Doing this here
    // rather than in sources_are_fresh ensures a failed run never advances the
    // baseline — the next invocation will detect a mismatch and re-run.
    if let Some((hash, path)) = compute_source_hash(task, config).await? {
        if let Some(dir) = path.parent() {
            file::create_dir_all(dir)?;
        }
        file::write(&path, &hash)?;
    }
    // Persist the output hash so the next freshness check can detect missing
    // or incomplete outputs even when the source hash still matches.
    // Traversal errors (broken symlinks, unreadable files) are warned but not
    // propagated — the task itself succeeded; failing here would be misleading.
    // Without a stored output hash the next freshness check will conservatively
    // treat the task as stale.
    if Settings::get().task.source_freshness_hash_contents {
        let out_path = outputs_hash_path(task, &root);
        match compute_output_hash(task, &root) {
            Ok(Some(h)) => {
                if let Some(dir) = out_path.parent() {
                    file::create_dir_all(dir)?;
                }
                file::write(&out_path, &h)?;
            }
            Ok(None) => {} // no outputs defined — nothing to save
            Err(e) => {
                // Remove the stale baseline so the next run is not skipped
                // against an obsolete output snapshot.
                let _ = std::fs::remove_file(&out_path);
                warn!(
                    "task {} output hashing failed; next run will not be skipped: {e}",
                    task.name
                );
            }
        }
    }
    Ok(())
}

/// Identity hash for a task in a given working directory. Used as the
/// filename stem for any per-task state we write under `STATE/task-sources/`,
/// so that changes to the task definition (sources, cmd, etc.), the config
/// file it came from, or the working directory all invalidate state in
/// lock-step.
fn task_state_key(task: &Task, root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    task.hash(&mut hasher);
    task.config_sources().hash(&mut hasher);
    root.hash(&mut hasher);
    task.run.hash(&mut hasher);
    task.sources.hash(&mut hasher);
    task.outputs.patterns().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Get the path to store source hashes for a task
fn sources_hash_path(task: &Task, root: &Path, content_hash: bool) -> PathBuf {
    let suffix = if content_hash { "-content" } else { "" };
    dirs::STATE
        .join("task-sources")
        .join(format!("{}{suffix}", task_state_key(task, root)))
}

/// Get the existing source hash for a task, if it exists
fn source_existing_hash(task: &Task, root: &Path, content_hash: bool) -> Option<String> {
    let path = sources_hash_path(task, root, content_hash);
    if path.exists() {
        Some(file::read_to_string(&path).unwrap_or_default())
    } else {
        None
    }
}

/// Path to the stored output hash for a task.
fn outputs_hash_path(task: &Task, root: &Path) -> PathBuf {
    dirs::STATE
        .join("task-sources")
        .join(format!("{}-outputs", task_state_key(task, root)))
}

/// Read the previously stored output hash, if any.
fn output_existing_hash(task: &Task, root: &Path) -> Option<String> {
    let path = outputs_hash_path(task, root);
    if path.exists() {
        Some(file::read_to_string(&path).unwrap_or_default())
    } else {
        None
    }
}

/// Compute a content-integrity hash for all current output files.
///
/// Returns `None` when any statically-named output is missing (incomplete
/// outputs), when a glob pattern expands to zero matching filesystem objects,
/// or when the task declares no outputs. A `Some` value encodes the sorted
/// `(path, blake3_content_hash)` of every resolved output file — two identical
/// sets of fully-present, content-identical outputs produce the same hash.
///
/// Content hashing (blake3) catches same-size modifications inside directory
/// outputs that `(path, size)` or `(path, size, mtime)` would miss.
/// Directory outputs (static or glob-matched) are walked recursively.
fn compute_output_hash(task: &Task, root: &Path) -> Result<Option<String>> {
    let raw_patterns = task.outputs.paths(task, root);
    let matcher = build_output_matcher(root, &raw_patterns)?;
    let patterns_or_paths = output_glob_patterns(&raw_patterns);
    if patterns_or_paths.is_empty() {
        return Ok(None);
    }

    let (glob_pats, static_paths): (Vec<&String>, Vec<&String>) =
        patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

    // (path, blake3_hex) — full content hash for correctness.
    let mut entries: Vec<(PathBuf, String)> = Vec::new();

    fn hash_file(path: &Path) -> Result<(PathBuf, String)> {
        Ok((path.to_path_buf(), hash::file_hash_blake3(path, None)?))
    }

    /// Walk a directory and push entries for all descendants.
    /// Files get their blake3 content hash; subdirectories get a "dir" sentinel
    /// so that additions/deletions of empty nested directories are detected.
    /// Symlinked directories are followed so content changes inside them are
    /// caught. Returns `true` when at least one entry was found.
    fn push_dir_entries(
        dir: &Path,
        entries: &mut Vec<(PathBuf, String)>,
        matcher: &Override,
    ) -> Result<bool> {
        let mut found_any = false;
        for entry in WalkDir::new(dir).follow_links(true).into_iter() {
            let entry = entry?;
            let path = entry.path();
            if path == dir {
                continue; // skip the root directory itself
            }
            if !is_output(matcher, path, entry.file_type().is_dir()) {
                continue;
            }
            if entry.file_type().is_file() {
                entries.push(hash_file(path)?);
                found_any = true;
            } else if entry.file_type().is_dir() {
                entries.push((path.to_path_buf(), "dir".to_string()));
                found_any = true;
            }
        }
        Ok(found_any)
    }

    for path_str in static_paths {
        let path = {
            let p = Path::new(path_str.as_str());
            if p.is_relative() {
                root.join(p)
            } else {
                p.to_path_buf()
            }
        };
        match path.metadata() {
            Ok(m) if m.is_file() => {
                if is_output(&matcher, &path, false) {
                    entries.push(hash_file(&path)?);
                } else {
                    continue;
                }
            }
            Ok(m) if m.is_dir() => {
                if !push_dir_entries(&path, &mut entries, &matcher)?
                    && is_output(&matcher, &path, true)
                {
                    // Empty directory — sentinel so its deletion is detected.
                    entries.push((path, "empty-dir".to_string()));
                }
            }
            Ok(_) => {
                if is_output(&matcher, &path, false) {
                    entries.push((path, "other".to_string()));
                }
            }
            Err(_) => {
                if is_output(&matcher, &path, false) || is_output(&matcher, &path, true) {
                    return Ok(None); // selected and missing → outputs incomplete
                }
            }
        }
    }

    for pattern_str in glob_pats {
        let mut glob_matched = false;
        for expanded in expand_glob_braces(pattern_str)? {
            let full = resolve_task_path(root, expanded);
            for entry in glob(full.to_str().unwrap_or_default())? {
                // Propagate glob resolution errors (OS errors during directory
                // reads) rather than silently skipping them — a partial result
                // could produce the same hash as a complete one.
                let path = entry?;
                glob_matched = true;
                let metadata = match path.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        if !is_output(&matcher, &path, false) && !is_output(&matcher, &path, true) {
                            continue;
                        }
                        return Ok(None); // selected and unreadable → outputs incomplete
                    }
                };
                if !is_output(&matcher, &path, metadata.is_dir()) {
                    continue;
                }
                match metadata {
                    m if m.is_file() => {
                        entries.push(hash_file(&path)?);
                    }
                    m if m.is_dir() => {
                        let found = push_dir_entries(&path, &mut entries, &matcher)?;
                        if !found {
                            entries.push((path, "empty-dir".to_string()));
                        }
                    }
                    _ => {
                        entries.push((path, "other".to_string()));
                    }
                }
            }
        }
        // A glob that matches nothing means expected outputs are missing.
        if !glob_matched {
            return Ok(None);
        }
    }

    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(Some(hash::hash_to_str(&entries)))
}

/// Get file metadata for a list of include-side patterns or paths, retaining
/// only files that `matcher` selects as a source.
fn get_file_metadatas(
    root: &Path,
    patterns_or_paths: &[String],
    matcher: &Override,
) -> Result<Vec<(PathBuf, fs::Metadata)>> {
    if patterns_or_paths.is_empty() {
        return Ok(vec![]);
    }
    let (patterns, paths): (Vec<&String>, Vec<&String>) =
        patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

    let mut metadatas = BTreeMap::new();
    for pattern in patterns {
        for expanded in expand_glob_braces(pattern)? {
            let pattern = resolve_task_path(root, expanded);
            let files = glob(pattern.to_str().unwrap())?;
            for file in files.flatten() {
                if let Ok(metadata) = file.metadata() {
                    metadatas.insert(file, metadata);
                }
            }
        }
    }

    for path in paths {
        let file = resolve_task_path(root, path);
        if let Ok(metadata) = file.metadata() {
            metadatas.insert(file, metadata);
        }
    }

    let metadatas = metadatas
        .into_iter()
        .filter(|(_, m)| m.is_file())
        .filter(|(p, _)| is_source(matcher, p))
        .collect_vec();

    Ok(metadatas)
}

/// Convert file metadata to a hash string for comparison
///
/// Includes path, file size and mtime. Without the mtime, a change that keeps the file size — a
/// version bump from `1.2.3` to `1.2.4`, say — and lands an mtime no newer than the output falls
/// through to the mtime comparison below, which then reports the task as fresh and leaves a stale
/// output in place. That is what tar, unzip, `rsync -a` and `cp -p` do when they restore an older
/// tree; `git checkout` is unaffected because it always writes with the current time.
///
/// The [`SystemTime`] is hashed as it is rather than as a duration since the epoch: converting
/// would fold every pre-epoch mtime into the same value as "this filesystem reports no mtime",
/// making two distinct timestamps indistinguishable. `None` therefore means only that the mtime is
/// unavailable.
fn file_metadatas_to_hash(metadatas: &[(PathBuf, fs::Metadata)]) -> String {
    let stat_info: Vec<_> = metadatas
        .iter()
        .map(|(p, m)| (p, m.len(), m.modified().ok()))
        .collect();
    hash::hash_to_str(&stat_info)
}

/// Per-file content hash cache entry. The `(size, mtime_secs, mtime_nanos)`
/// tuple is the cache key (in the git-style "stat-info" sense): when those
/// three match, we reuse `hash` without re-reading the file.
#[derive(Debug, Serialize, Deserialize)]
struct CachedFileHash {
    mtime_secs: i64,
    mtime_nanos: u32,
    size: u64,
    hash: String,
}

type ContentHashCache = BTreeMap<PathBuf, CachedFileHash>;

/// Path to the per-task content-hash cache file. Shares `task_state_key`
/// with `sources_hash_path` so changes to the task definition invalidate
/// both in lock-step.
fn content_hash_cache_path(task: &Task, root: &Path) -> PathBuf {
    dirs::STATE
        .join("task-sources")
        .join(format!("{}-content-cache", task_state_key(task, root)))
}

fn load_content_hash_cache(path: &Path) -> ContentHashCache {
    (|| -> Result<ContentHashCache> {
        let mut zlib = ZlibDecoder::new(File::open(path)?);
        let mut bytes = Vec::new();
        zlib.read_to_end(&mut bytes)?;
        Ok(rmp_serde::from_slice(&bytes)?)
    })()
    .unwrap_or_default()
}

fn save_content_hash_cache(path: &Path, cache: &ContentHashCache) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    let partial = path.with_extension(format!("part-{}", random_string(8)));
    {
        let mut zlib = ZlibEncoder::new(File::create(&partial)?, Compression::fast());
        zlib.write_all(&rmp_serde::to_vec_named(cache)?)?;
        // Propagate finalization errors explicitly — ZlibEncoder's Drop impl
        // would silently discard them, leaving a truncated partial file that
        // we'd then rename into place as a poisoned cache.
        zlib.finish()?;
    }
    file::rename(&partial, path)?;
    Ok(())
}

fn cached_entry_matches(entry: &CachedFileHash, metadata: &fs::Metadata) -> bool {
    let Ok(mtime) = metadata.modified() else {
        return false;
    };
    let Ok(dur) = mtime.duration_since(UNIX_EPOCH) else {
        return false;
    };
    entry.size == metadata.len()
        && entry.mtime_secs == dur.as_secs() as i64
        && entry.mtime_nanos == dur.subsec_nanos()
}

fn make_cache_entry(metadata: &fs::Metadata, hash: String) -> CachedFileHash {
    let dur = metadata
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok());
    CachedFileHash {
        mtime_secs: dur.map(|d| d.as_secs() as i64).unwrap_or(0),
        mtime_nanos: dur.map(|d| d.subsec_nanos()).unwrap_or(0),
        size: metadata.len(),
        hash,
    }
}

/// Convert file contents to a hash string for comparison using blake3.
///
/// More accurate than metadata hashing but slower since it reads all file
/// contents. `cache` is consulted first: if a file's `(size, mtime_secs,
/// mtime_nanos)` match the cached entry, the stored hash is reused and the
/// file is not re-read. On return, `cache` is rebuilt from scratch with one
/// entry per current source file — entries for files no longer in `sources`
/// are pruned so the cache file size stays bounded.
fn file_contents_to_hash(
    metadatas: &[(PathBuf, fs::Metadata)],
    cache: &mut ContentHashCache,
) -> Result<String> {
    let mut content_hashes: Vec<(&PathBuf, String)> = Vec::new();
    let mut next: ContentHashCache = BTreeMap::new();
    for (path, metadata) in metadatas {
        let hash = match cache.get(path) {
            Some(entry) if cached_entry_matches(entry, metadata) => entry.hash.clone(),
            _ => hash::file_hash_blake3(path, None)?,
        };
        next.insert(path.clone(), make_cache_entry(metadata, hash.clone()));
        content_hashes.push((path, hash));
    }
    *cache = next;
    Ok(hash::hash_to_str(&content_hashes))
}

/// Get the last modified time from file metadata
fn get_last_modified_from_metadatas(metadatas: &[(PathBuf, fs::Metadata)]) -> Option<SystemTime> {
    metadatas.iter().flat_map(|(_, m)| m.modified()).max()
}

/// Get the last modified time from selected task outputs.
fn get_last_modified(root: &Path, patterns_or_paths: &[String]) -> Result<Option<SystemTime>> {
    if patterns_or_paths.is_empty() {
        return Ok(None);
    }
    let matcher = build_output_matcher(root, patterns_or_paths)?;
    let mut file_modified = Vec::new();
    let mut directory_modified = Vec::new();
    for pattern in output_glob_patterns(patterns_or_paths) {
        let is_glob = is_glob_pattern(&pattern);
        let candidates = if is_glob {
            let mut candidates = Vec::new();
            for expanded in expand_glob_braces(&pattern)? {
                let expanded = resolve_task_path(root, expanded);
                candidates.extend(
                    glob(expanded.to_str().unwrap_or_default())?.collect::<Result<Vec<_>, _>>()?,
                );
            }
            candidates
        } else {
            vec![resolve_task_path(root, &pattern)]
        };
        let mut found_candidate = false;
        for candidate in candidates {
            if fs::symlink_metadata(&candidate).is_err() {
                continue;
            }
            found_candidate = true;
            for entry in WalkDir::new(candidate).follow_links(true) {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if is_output(&matcher, entry.path(), metadata.is_dir()) {
                    if metadata.is_dir() {
                        directory_modified.push(metadata.modified()?);
                    } else {
                        file_modified.push(metadata.modified()?);
                    }
                }
            }
        }
        // Every positive output pattern represents a required artifact root.
        // Excluded static paths are the exception; the ordered matcher makes
        // those optional even though they remain in the enumeration list.
        if !found_candidate
            && (is_glob || {
                let path = resolve_task_path(root, &pattern);
                is_output(&matcher, &path, false) || is_output(&matcher, &path, true)
            })
        {
            return Ok(None);
        }
    }
    let last_mod = file_modified.into_iter().chain(directory_modified).max();

    trace!(
        "last_modified of {}: {last_mod:?}",
        patterns_or_paths.iter().join(" ")
    );
    Ok(last_mod)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(sources: &[&str], path: &str) -> bool {
        let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
        let root = Path::new(".");
        let matcher = build_source_matcher(root, root, &sources);
        is_source(&matcher, Path::new(path))
    }

    #[test]
    fn output_matcher_excludes_and_reincludes_descendants() {
        let root = Path::new("/project");
        let patterns = vec![
            "dist".to_string(),
            "!dist/**/*.map".to_string(),
            "dist/keep.map".to_string(),
        ];
        let matcher = build_output_matcher(root, &patterns).unwrap();

        assert!(is_output(&matcher, &root.join("dist/app.js"), false));
        assert!(!is_output(&matcher, &root.join("dist/app.map"), false));
        assert!(is_output(&matcher, &root.join("dist/nested/app.js"), false));
        assert!(is_output(&matcher, &root.join("dist/keep.map"), false));
    }

    #[test]
    fn output_matcher_normalizes_absolute_patterns_under_root() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("dist/result.txt");
        let patterns = vec![format!("{}/dist/**/*", root.path().display())];
        let matcher = build_output_matcher(root.path(), &patterns).unwrap();

        assert!(is_output(&matcher, &output, false));
    }

    #[test]
    fn metadata_hash_notices_a_same_size_change_with_an_older_mtime() {
        // https://github.com/jdx/mise/discussions/4209 — restoring an older tree with tar,
        // `rsync -a` or `cp -p` keeps the recorded mtime, so a same-size edit is invisible to a
        // hash built from the path and size alone and the mtime comparison then calls it fresh.
        let root = tempfile::tempdir().unwrap();
        let p = root.path().join("pin.txt");
        fs::write(&p, "1.2.3").unwrap();
        let before = file_metadatas_to_hash(&[(p.clone(), fs::metadata(&p).unwrap())]);

        fs::write(&p, "1.2.4").unwrap();
        let restored = filetime::FileTime::from_unix_time(1_000_000, 0);
        filetime::set_file_times(&p, restored, restored).unwrap();
        let after = file_metadatas_to_hash(&[(p.clone(), fs::metadata(&p).unwrap())]);

        assert_eq!(
            fs::metadata(&p).unwrap().len(),
            5,
            "the fixture only exercises the bug while both versions are the same size"
        );
        assert_ne!(before, after, "the mtime change should be part of the hash");
    }

    #[test]
    fn metadata_hash_separates_pre_epoch_mtimes() {
        // mtimes from before 1970 must stay distinct from each other, and from "the filesystem
        // reports no mtime" — folding them together would hide a change the same way the missing
        // mtime did.
        let root = tempfile::tempdir().unwrap();
        let p = root.path().join("ancient.txt");
        fs::write(&p, "x").unwrap();

        let stamp = |secs: i64| {
            let t = filetime::FileTime::from_unix_time(secs, 0);
            // a filesystem that cannot store a pre-epoch timestamp is not what this pins down
            filetime::set_file_times(&p, t, t).ok()?;
            let metadata = fs::metadata(&p).unwrap();
            let mtime = metadata.modified().ok()?;
            Some((mtime, file_metadatas_to_hash(&[(p.clone(), metadata)])))
        };
        let (Some((first_mtime, first)), Some((second_mtime, second))) =
            (stamp(-2_000_000), stamp(-1_000_000))
        else {
            return;
        };
        if first_mtime == second_mtime {
            return;
        }

        assert_ne!(
            first, second,
            "two different pre-epoch mtimes should hash differently"
        );
    }

    #[test]
    fn output_globs_ignore_excludes_and_unescape_literal_bangs() {
        assert_eq!(
            output_glob_patterns(&[
                "dist".to_string(),
                "!dist/**/*.map".to_string(),
                "\\!important".to_string(),
            ]),
            ["dist", "!important"]
        );
    }

    #[test]
    fn glob_braces_expand_nested_and_multiple_alternates() {
        assert_eq!(
            expand_glob_braces("src/{a,{b,c}}/{one,two}.txt").unwrap(),
            [
                "src/a/one.txt",
                "src/a/two.txt",
                "src/b/one.txt",
                "src/b/two.txt",
                "src/c/one.txt",
                "src/c/two.txt",
            ]
        );
        assert_eq!(expand_glob_braces("{,a}.txt").unwrap(), ["a.txt"]);
        assert!(expand_glob_braces("src/{a,b.txt").is_err());
        assert!(expand_glob_braces(&"{a,b}".repeat(11)).is_err());
    }

    #[test]
    fn glob_braces_preserve_literal_singleton_groups() {
        assert_eq!(
            expand_glob_braces("{generated}.txt").unwrap(),
            ["[{]generated[}].txt"]
        );
        assert_eq!(
            expand_glob_braces("{generated}/{a,b}.txt").unwrap(),
            ["[{]generated[}]/a.txt", "[{]generated[}]/b.txt"]
        );
        assert_eq!(
            expand_glob_braces("{generated}.{txt,out}").unwrap(),
            ["[{]generated[}].txt", "[{]generated[}].out"]
        );
        assert_eq!(
            expand_glob_braces("{prefix-{a,b}}.txt").unwrap(),
            ["[{]prefix-a[}].txt", "[{]prefix-b[}].txt"]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn glob_braces_preserve_escaped_unix_braces() {
        assert_eq!(
            expand_glob_braces(r"src/\{literal\}/[{}].txt").unwrap(),
            [r"src/\{literal\}/[{}].txt"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn glob_braces_treat_windows_backslashes_as_separators() {
        assert_eq!(
            expand_glob_braces(r"C:\build\{debug,release}\*.exe").unwrap(),
            [r"C:\build\debug\*.exe", r"C:\build\release\*.exe"]
        );
    }

    #[test]
    fn source_and_output_matchers_support_ordered_brace_globs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source_matcher = build_source_matcher(
            root,
            root,
            &[
                "{Cargo.toml,README.md}".to_string(),
                "!README.md".to_string(),
                "README.md".to_string(),
            ],
        );
        let output_matcher = build_output_matcher(
            root,
            &[
                "{Cargo.toml,README.md}".to_string(),
                "!README.md".to_string(),
            ],
        )
        .unwrap();

        assert!(is_source(&source_matcher, &root.join("Cargo.toml")));
        assert!(is_source(&source_matcher, &root.join("README.md")));
        assert!(is_output(&output_matcher, &root.join("Cargo.toml"), false));
        assert!(!is_output(&output_matcher, &root.join("README.md"), false));
    }

    #[test]
    fn output_hash_supports_brace_globs() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.out"), "a").unwrap();
        fs::write(root.path().join("b.out"), "b").unwrap();
        let task = Task {
            outputs: crate::task::task_sources::TaskOutputs::Files(vec!["{a,b}.out".to_string()]),
            ..Default::default()
        };

        assert!(compute_output_hash(&task, root.path()).unwrap().is_some());
    }

    #[test]
    fn output_mtime_includes_selected_directories() {
        let root = tempfile::tempdir().unwrap();
        let dist = root.path().join("dist");
        let output = dist.join("result.txt");
        fs::create_dir(&dist).unwrap();
        fs::write(&output, "result").unwrap();
        let file_mtime = filetime::FileTime::from_unix_time(100, 0);
        let directory_mtime = filetime::FileTime::from_unix_time(200, 0);
        filetime::set_file_mtime(&output, file_mtime).unwrap();
        filetime::set_file_mtime(&dist, directory_mtime).unwrap();

        let modified = get_last_modified(root.path(), &["dist".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(modified, SystemTime::from(directory_mtime));
    }

    #[test]
    fn output_mtime_requires_all_selected_static_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("present.txt"), "present").unwrap();

        let modified = get_last_modified(
            root.path(),
            &["present.txt".to_string(), "missing.txt".to_string()],
        )
        .unwrap();

        assert!(modified.is_none());
    }

    #[test]
    fn output_mtime_requires_each_positive_glob_to_match() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("present.txt"), "present").unwrap();

        let modified = get_last_modified(
            root.path(),
            &["present.txt".to_string(), "*.generated".to_string()],
        )
        .unwrap();

        assert!(modified.is_none());
    }

    #[test]
    fn output_mtime_allows_missing_excluded_static_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("present.txt"), "present").unwrap();

        let modified = get_last_modified(
            root.path(),
            &[
                "present.txt".to_string(),
                "missing.txt".to_string(),
                "!missing.txt".to_string(),
            ],
        )
        .unwrap();

        assert!(modified.is_some());
    }

    #[test]
    fn output_mtime_brace_alternatives_require_any_match() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.out"), "a").unwrap();

        let modified = get_last_modified(root.path(), &["{a,b}.out".to_string()]).unwrap();

        assert!(modified.is_some());
    }

    #[test]
    fn output_mtime_allows_glob_matches_that_are_all_excluded() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("present.txt"), "present").unwrap();
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/vendor.js"), "vendor").unwrap();

        let modified = get_last_modified(
            root.path(),
            &[
                "present.txt".to_string(),
                "dist/*.js".to_string(),
                "!dist/vendor.js".to_string(),
            ],
        )
        .unwrap();

        assert!(modified.is_some());
    }

    #[test]
    fn output_hash_allows_missing_excluded_static_paths() {
        let root = tempfile::tempdir().unwrap();
        let task = Task {
            outputs: crate::task::task_sources::TaskOutputs::Files(vec![
                "missing.txt".to_string(),
                "!missing.txt".to_string(),
            ]),
            ..Default::default()
        };

        assert!(compute_output_hash(&task, root.path()).unwrap().is_some());
    }

    #[test]
    fn output_hash_allows_glob_matches_that_are_all_excluded() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/vendor.js"), "vendor").unwrap();
        let task = Task {
            outputs: crate::task::task_sources::TaskOutputs::Files(vec![
                "dist/*.js".to_string(),
                "!dist/vendor.js".to_string(),
            ]),
            ..Default::default()
        };

        assert!(compute_output_hash(&task, root.path()).unwrap().is_some());
    }

    #[test]
    fn task_state_key_includes_all_definition_sources() {
        let root = Path::new("/project");
        let mut task = Task {
            name: "build".to_string(),
            config_source: PathBuf::from(".mise/tasks/build"),
            ..Default::default()
        };
        let primary_key = task_state_key(&task, root);

        task.additional_config_sources
            .push(PathBuf::from("mise.toml"));

        assert_ne!(primary_key, task_state_key(&task, root));
    }

    #[test]
    fn task_state_key_changes_when_run_changes() {
        use crate::task::RunEntry;
        let root = Path::new("/project");
        let mut task = Task {
            name: "build".to_string(),
            config_source: PathBuf::from("mise.toml"),
            run: vec![RunEntry::Script("echo v1".to_string())],
            ..Default::default()
        };
        let key_v1 = task_state_key(&task, root);
        task.run = vec![RunEntry::Script("echo v2".to_string())];
        assert_ne!(key_v1, task_state_key(&task, root));
    }

    #[test]
    fn task_state_key_changes_when_sources_change() {
        let root = Path::new("/project");
        let mut task = Task {
            name: "build".to_string(),
            config_source: PathBuf::from("mise.toml"),
            sources: vec!["src.txt".to_string()],
            ..Default::default()
        };
        let key_v1 = task_state_key(&task, root);
        task.sources = vec!["other.txt".to_string()];
        assert_ne!(key_v1, task_state_key(&task, root));
    }

    #[test]
    fn glob_patterns_drops_excludes_and_unescapes() {
        let inputs = vec![
            "src/**/*.ts".to_string(),
            "!src/**/*.test.ts".to_string(),
            "\\!literal.txt".to_string(),
            "tsconfig.json".to_string(),
        ];
        assert_eq!(
            source_glob_patterns(&inputs),
            vec!["src/**/*.ts", "!literal.txt", "tsconfig.json"],
        );
    }

    #[test]
    fn matcher_includes_plain_pattern() {
        assert!(matches(&["src/**/*.ts"], "src/foo.ts"));
        assert!(matches(&["src/**/*.ts"], "src/sub/foo.ts"));
        assert!(!matches(&["src/**/*.ts"], "lib/foo.ts"));
    }

    #[test]
    fn matcher_negation_excludes() {
        let pats = &["src/**/*.ts", "!src/**/*.test.ts"];
        assert!(matches(pats, "src/foo.ts"));
        assert!(!matches(pats, "src/foo.test.ts"));
    }

    #[test]
    fn matcher_reincludes_after_negation() {
        // Re-inclusion semantics: a later non-negated entry wins over an
        // earlier `!`-negation, just like a gitignore whitelist.
        let pats = &["src/**/*.ts", "!src/**/*.test.ts", "src/keep.test.ts"];
        assert!(matches(pats, "src/foo.ts"));
        assert!(!matches(pats, "src/foo.test.ts"));
        assert!(matches(pats, "src/keep.test.ts"));
    }

    #[test]
    fn matcher_escaped_literal_bang() {
        let pats = &["\\!important.txt", "!ignored.txt"];
        assert!(matches(pats, "!important.txt"));
        assert!(!matches(pats, "ignored.txt"));
    }

    #[test]
    #[cfg(unix)]
    fn matcher_absolute_literal_bang_under_root() {
        let root = Path::new("/project");
        let sources = vec!["/project/!important.txt".to_string()];
        let matcher = build_source_matcher(root, root, &sources);
        assert!(is_source(&matcher, Path::new("/project/!important.txt")));
        assert!(!is_source(&matcher, Path::new("/project/other.txt")));
    }

    #[test]
    #[cfg(unix)]
    fn matcher_absolute_pattern_under_root() {
        // Patterns that resolve to absolute paths under the matcher root
        // (e.g. from `{{cwd}}/input` after templating) are normalized to
        // root-relative so gitignore semantics work correctly.
        // Unix-only because Windows uses `C:\...` for absolute paths and
        // `Path::is_absolute` returns false for `/proj` there.
        let root = Path::new("/proj");
        let sources = vec!["/proj/input".to_string()];
        let matcher = build_source_matcher(root, root, &sources);
        assert!(is_source(&matcher, Path::new("/proj/input")));
        assert!(!is_source(&matcher, Path::new("/proj/other")));
    }

    #[test]
    #[cfg(unix)]
    fn matcher_absolute_negation_under_root() {
        let root = Path::new("/proj");
        let sources = vec![
            "/proj/src/**/*.ts".to_string(),
            "!/proj/src/**/*.test.ts".to_string(),
        ];
        let matcher = build_source_matcher(root, root, &sources);
        assert!(is_source(&matcher, Path::new("/proj/src/foo.ts")));
        assert!(!is_source(&matcher, Path::new("/proj/src/foo.test.ts")));
    }

    /// Regression: an absolute path outside the matcher's root must not be
    /// silently dropped. `Override::matched` returns `Match::None` for such
    /// paths and (with positive patterns present) promotes them to
    /// `Match::Ignore`, which would silently exclude legitimate sources
    /// (e.g. a workspace-root file referenced from a sub-package task).
    #[test]
    #[cfg(unix)]
    fn matcher_absolute_path_outside_root_passes_through() {
        let root = Path::new("/proj");
        let sources = vec!["/elsewhere/Cargo.toml".to_string()];
        let matcher = build_source_matcher(root, root, &sources);
        assert!(is_source(&matcher, Path::new("/elsewhere/Cargo.toml")));
    }

    /// Workspace-rooted absolute pattern in a subproject task must match files
    /// both inside and outside the subproject CWD.
    #[test]
    #[cfg(unix)]
    fn matcher_subproject_absolute_workspace_pattern() {
        let match_root = Path::new("/workspace");
        let task_cwd = Path::new("/workspace/lib/worker");
        let sources = vec!["/workspace/lib/**/*".to_string()];
        let matcher = build_source_matcher(match_root, task_cwd, &sources);
        assert!(is_source(
            &matcher,
            Path::new("/workspace/lib/worker/worker.go")
        ));
        assert!(is_source(&matcher, Path::new("/workspace/lib/shared.go")));
        assert!(!is_source(&matcher, Path::new("/workspace/other/file.go")));
    }

    #[test]
    fn absolute_source_patterns_are_enumerated_from_a_subproject() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path();
        let task_cwd = workspace.join("packages/app");
        let source = task_cwd.join("src/input.txt");
        let global = workspace.join("workspace.txt");
        fs::create_dir_all(source.parent().unwrap())?;
        fs::write(&source, "source")?;
        fs::write(&global, "global")?;

        let sources = vec![
            format!("{}/packages/app/src/**/*", workspace.display()),
            global.to_string_lossy().to_string(),
        ];
        let matcher = build_source_matcher(workspace, &task_cwd, &sources);
        let metadatas = get_file_metadatas(&task_cwd, &source_glob_patterns(&sources), &matcher)?;
        let paths = metadatas
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>();

        assert!(paths.contains(&source), "{paths:?}");
        assert!(paths.contains(&global), "{paths:?}");
        Ok(())
    }

    /// Relative pattern in a subproject task must be anchored at the task CWD,
    /// not the workspace root.
    #[test]
    #[cfg(unix)]
    fn matcher_subproject_relative_pattern() {
        let match_root = Path::new("/workspace");
        let task_cwd = Path::new("/workspace/lib/worker");
        let sources = vec!["src/**/*.go".to_string()];
        let matcher = build_source_matcher(match_root, task_cwd, &sources);
        assert!(is_source(
            &matcher,
            Path::new("/workspace/lib/worker/src/main.go")
        ));
        assert!(!is_source(&matcher, Path::new("/workspace/src/other.go")));
    }

    #[test]
    fn relative_sources_match_when_task_dir_starts_with_dot() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path();
        let task_cwd = normalize_task_cwd(workspace.join("./sub"));
        let source = workspace.join("sub/input.txt");
        fs::create_dir_all(source.parent().unwrap())?;
        fs::write(&source, "source")?;

        let sources = vec!["input.txt".to_string()];
        let matcher = build_source_matcher(workspace, &task_cwd, &sources);
        let metadatas = get_file_metadatas(&task_cwd, &sources, &matcher)?;

        assert_eq!(
            metadatas.into_iter().map(|(path, _)| path).collect_vec(),
            [source]
        );
        Ok(())
    }

    #[test]
    fn content_hash_cache_reuses_unchanged_files() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        std::fs::write(&a, "hello").unwrap();
        std::fs::write(&b, "world").unwrap();
        let metadatas = vec![
            (a.clone(), a.metadata().unwrap()),
            (b.clone(), b.metadata().unwrap()),
        ];

        let mut cache = ContentHashCache::new();
        let first = file_contents_to_hash(&metadatas, &mut cache).unwrap();
        assert_eq!(cache.len(), 2);
        let a_hash_v1 = cache.get(&a).unwrap().hash.clone();

        // Re-run with same files: hashes should be reused, aggregate unchanged.
        let second = file_contents_to_hash(&metadatas, &mut cache).unwrap();
        assert_eq!(first, second);
        assert_eq!(cache.get(&a).unwrap().hash, a_hash_v1);

        // Mutate `a` so size differs; aggregate hash must change.
        std::fs::write(&a, "hello world").unwrap();
        let metadatas = vec![
            (a.clone(), a.metadata().unwrap()),
            (b.clone(), b.metadata().unwrap()),
        ];
        let third = file_contents_to_hash(&metadatas, &mut cache).unwrap();
        assert_ne!(second, third);
        assert_ne!(cache.get(&a).unwrap().hash, a_hash_v1);
    }

    #[test]
    fn content_hash_cache_prunes_dropped_files() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        std::fs::write(&a, "hello").unwrap();
        std::fs::write(&b, "world").unwrap();

        let mut cache = ContentHashCache::new();
        let metadatas = vec![
            (a.clone(), a.metadata().unwrap()),
            (b.clone(), b.metadata().unwrap()),
        ];
        file_contents_to_hash(&metadatas, &mut cache).unwrap();
        assert_eq!(cache.len(), 2);

        // Only `a` is a source this run — `b` should drop out of the cache.
        let metadatas = vec![(a.clone(), a.metadata().unwrap())];
        file_contents_to_hash(&metadatas, &mut cache).unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&a));
        assert!(!cache.contains_key(&b));
    }

    #[test]
    fn content_hash_cache_round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        std::fs::write(&a, "hello").unwrap();

        let mut cache = ContentHashCache::new();
        let metadatas = vec![(a.clone(), a.metadata().unwrap())];
        file_contents_to_hash(&metadatas, &mut cache).unwrap();

        let cache_path = tmp.path().join("cache.bin");
        save_content_hash_cache(&cache_path, &cache).unwrap();
        let loaded = load_content_hash_cache(&cache_path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get(&a).unwrap().hash, cache.get(&a).unwrap().hash,);

        // Corrupt the file: loader must silently fall back to empty.
        std::fs::write(&cache_path, b"not a valid msgpack stream").unwrap();
        let loaded = load_content_hash_cache(&cache_path);
        assert!(loaded.is_empty());
    }
}
