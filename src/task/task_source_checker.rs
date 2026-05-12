use crate::config::{Config, Settings};
use crate::dirs;
use crate::file::{self, display_path};
use crate::hash;
use crate::rand::random_string;
use crate::task::Task;
use eyre::{Result, eyre};
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Check if a path is a glob pattern
pub fn is_glob_pattern(path: &str) -> bool {
    // This is the character set used for glob detection by glob
    let glob_chars = ['*', '{', '}'];
    path.chars().any(|c| glob_chars.contains(&c))
}

/// Build an [`Override`] matcher for a task's `sources` patterns.
///
/// Patterns use gitignore syntax with `!` inverted (the [`Override`] convention,
/// see `ignore::overrides`): a non-negated entry marks a file as a *source*,
/// and a `!`-prefixed entry *excludes* it. `\!` escapes a literal leading `!`,
/// and order matters — a later non-negated entry can re-include a file an
/// earlier `!` excluded.
///
/// Patterns that are absolute paths under `root` are rewritten to be relative,
/// matching the convention used by `Override` itself (see its tests) and the
/// `ignore::WalkBuilder`: matchers receive root-relative patterns and let
/// `matched()` strip the root from incoming paths automatically.
pub(crate) fn build_source_matcher(root: &Path, sources: &[String]) -> Override {
    let mut builder = OverrideBuilder::new(root);
    for s in sources {
        let normalized = relativize_pattern(root, s);
        if let Err(e) = builder.add(&normalized) {
            warn!("invalid source pattern {s:?}: {e}");
        }
    }
    builder.build().unwrap_or_else(|e| {
        warn!("failed to build source matcher: {e}");
        Override::empty()
    })
}

/// If `pattern`'s body is an absolute path under `root`, rewrite it as a
/// root-relative path so the matcher can use gitignore's relative-path
/// semantics. The `!` / `\!` prefix is preserved as-is.
fn relativize_pattern(root: &Path, pattern: &str) -> String {
    let (prefix, body) = if pattern.starts_with("\\!") {
        // `\!body` is a literal include of a path beginning with `!`. Don't
        // peek past the escape — `OverrideBuilder::add` handles it.
        return pattern.to_string();
    } else if let Some(rest) = pattern.strip_prefix('!') {
        ("!", rest)
    } else {
        ("", pattern)
    };
    let body_path = Path::new(body);
    if body_path.is_absolute()
        && let Ok(rel) = body_path.strip_prefix(root)
        && let Some(rel_str) = rel.to_str()
    {
        return format!("{prefix}{rel_str}");
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
/// in that case (matching pre-PR behavior for workspace-root paths
/// referenced from sub-package tasks, etc.).
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

/// Get the last modified time from a list of paths
pub(crate) fn last_modified_path(root: &Path, paths: &[&String]) -> Result<Option<SystemTime>> {
    let files = paths.iter().map(|p| {
        let base = Path::new(p);
        if base.is_relative() {
            Path::new(&root).join(base)
        } else {
            base.to_path_buf()
        }
    });

    last_modified_file(files)
}

/// Get the last modified time from files matching glob patterns
pub(crate) fn last_modified_glob_match(
    root: impl AsRef<Path>,
    patterns: &[&String],
) -> Result<Option<SystemTime>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let root_ref = root.as_ref();
    let files = patterns
        .iter()
        .flat_map(|pattern| {
            glob(
                root_ref
                    .join(pattern)
                    .to_str()
                    .expect("Conversion to string path failed"),
            )
            .unwrap()
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.metadata()
                .expect("Metadata call failed")
                .file_type()
                .is_file()
        });

    last_modified_file(files)
}

/// Get the last modified time from an iterator of file paths
pub(crate) fn last_modified_file(
    files: impl IntoIterator<Item = PathBuf>,
) -> Result<Option<SystemTime>> {
    Ok(files
        .into_iter()
        .unique()
        .filter(|p| p.exists())
        .map(|p| {
            p.metadata()
                .map_err(|err| eyre!("{}: {}", display_path(p), err))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|m| m.modified().map_err(|err| eyre!(err)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max())
}

/// Get the working directory for a task
pub async fn task_cwd(task: &Task, config: &Arc<Config>) -> Result<PathBuf> {
    if let Some(d) = task.dir(config).await? {
        Ok(d)
    } else {
        Ok(config
            .project_root
            .clone()
            .or_else(|| dirs::CWD.clone())
            .unwrap_or_default())
    }
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
        let root = task_cwd(task, config).await?;
        let matcher = build_source_matcher(&root, &task.sources);
        let glob_patterns = source_glob_patterns(&task.sources);
        let mut source_metadatas = get_file_metadatas(&root, &glob_patterns, &matcher)?;
        // Always include the task's own config file as a source, regardless of
        // any excludes — a stray `!mise.toml` must not silently disable invalidation.
        let config_path = if task.config_source.is_absolute() {
            task.config_source.clone()
        } else {
            root.join(&task.config_source)
        };
        if let Ok(meta) = config_path.metadata()
            && meta.is_file()
            && !source_metadatas.iter().any(|(p, _)| p == &config_path)
        {
            source_metadatas.push((config_path, meta));
        }

        // Check if sources resolved to no files (likely a config mistake)
        if source_metadatas.is_empty() {
            warn!(
                "task {} has sources defined but no matching files found",
                task.name
            );
            return Ok(false);
        }

        // Check for epoch timestamps (files extracted from tarballs without preserved timestamps)
        // These are considered stale since we can't trust the mtime
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
        if source_existing_hash(task, &root, use_content_hash).is_some_and(|h| h != source_hash) {
            debug!(
                "source {} hash mismatch in {}",
                if use_content_hash {
                    "content"
                } else {
                    "metadata"
                },
                source_hash_path.display()
            );
            file::write(&source_hash_path, &source_hash)?;
            return Ok(false);
        }
        let sources = get_last_modified_from_metadatas(&source_metadatas);
        let outputs = get_last_modified(&root, &task.outputs.paths(task, &root))?;
        file::write(&source_hash_path, &source_hash)?;
        trace!("sources: {sources:?}, outputs: {outputs:?}");
        match (sources, outputs) {
            (Some(sources), Some(outputs)) => {
                if equal_mtime_is_fresh {
                    Ok(sources <= outputs)
                } else {
                    Ok(sources < outputs)
                }
            }
            _ => Ok(false),
        }
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
    if task.outputs.is_auto() {
        let root = task_cwd(task, config).await?;
        for p in task.outputs.paths(task, &root) {
            debug!("touching auto output file: {p}");
            file::touch_file(&PathBuf::from(&p))?;
        }
    } else {
        // Check if explicitly defined outputs were generated
        // Use task_cwd to respect the task's dir setting, matching sources_are_fresh behavior
        let root = task_cwd(task, config).await?;
        for output in task.outputs.paths(task, &root) {
            let output_exists = if is_glob_pattern(&output) {
                // For glob patterns, check if any files match
                let pattern = root.join(&output);
                glob(pattern.to_str().unwrap_or_default())
                    .map(|paths| paths.flatten().next().is_some())
                    .unwrap_or(false)
            } else {
                // For regular paths, check if file exists
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
    task.config_source.hash(&mut hasher);
    root.hash(&mut hasher);
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
        let files = glob(root.join(pattern).to_str().unwrap())?;
        for file in files.flatten() {
            if let Ok(metadata) = file.metadata() {
                metadatas.insert(file, metadata);
            }
        }
    }

    for path in paths {
        let file = root.join(path);
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
/// Includes path and file size to detect changes even when mtimes are unreliable
fn file_metadatas_to_hash(metadatas: &[(PathBuf, fs::Metadata)]) -> String {
    let path_and_sizes: Vec<_> = metadatas.iter().map(|(p, m)| (p, m.len())).collect();
    hash::hash_to_str(&path_and_sizes)
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

/// Get the last modified time from a list of patterns or paths. Used for
/// task *outputs*, which do not support exclusion patterns.
fn get_last_modified(root: &Path, patterns_or_paths: &[String]) -> Result<Option<SystemTime>> {
    if patterns_or_paths.is_empty() {
        return Ok(None);
    }
    let (patterns, paths): (Vec<&String>, Vec<&String>) =
        patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

    let last_mod = std::cmp::max(
        last_modified_glob_match(root, &patterns)?,
        last_modified_path(root, &paths)?,
    );

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
        let matcher = build_source_matcher(Path::new("."), &sources);
        is_source(&matcher, Path::new(path))
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
    fn matcher_absolute_pattern_under_root() {
        // Patterns that resolve to absolute paths under the matcher root
        // (e.g. from `{{cwd}}/input` after templating) are normalized to
        // root-relative so gitignore semantics work correctly.
        // Unix-only because Windows uses `C:\...` for absolute paths and
        // `Path::is_absolute` returns false for `/proj` there.
        let root = Path::new("/proj");
        let sources = vec!["/proj/input".to_string()];
        let matcher = build_source_matcher(root, &sources);
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
        let matcher = build_source_matcher(root, &sources);
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
        let matcher = build_source_matcher(root, &sources);
        assert!(is_source(&matcher, Path::new("/elsewhere/Cargo.toml")));
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
