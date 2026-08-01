use crate::config::{Config, Settings};
use crate::dirs;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::hash;
use crate::task::task_source_checker::{
    TaskCacheInputs, build_output_matcher, expand_glob_braces, is_output, output_glob_patterns,
    task_cache_inputs, task_cwd,
};
use crate::task::{RunEntry, Task};
use crate::toolset::Toolset;
use eyre::{Context, Report, Result, bail, eyre};
use glob::glob;
use ignore::overrides::Override;
use jdx_tar::{Builder, EntryType, Header};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const CACHE_FORMAT_VERSION: u8 = 2;
const CACHE_DIR_VERSION: &str = "v2";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskCacheConfig {
    pub enabled: bool,
    /// Ambient environment variables whose resolved values affect the cache key.
    pub env: Vec<String>,
    /// Commands whose stdout and stderr affect the cache key.
    pub command_inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum TaskCacheMode {
    /// Read cached results and write new results.
    #[default]
    ReadWrite,
    /// Read cached results without writing new results.
    ReadOnly,
    /// Write new results without reading cached results.
    WriteOnly,
    /// Disable task output caching for this run.
    Off,
    /// Read and write only the local cache.
    LocalOnly,
}

impl TaskCacheMode {
    pub(crate) fn from_env() -> Result<Self> {
        let Some(value) = std::env::var_os("MISE_TASK_CACHE") else {
            return Ok(Self::default());
        };
        let value = value
            .into_string()
            .map_err(|_| eyre!("MISE_TASK_CACHE must be valid UTF-8"))?;
        <Self as clap::ValueEnum>::from_str(&value, false).map_err(|_| {
            eyre!(
                "invalid MISE_TASK_CACHE value {value:?}; expected read-write, read-only, \
                 write-only, off, or local-only"
            )
        })
    }

    pub(crate) fn enabled(self) -> bool {
        self != Self::Off
    }

    pub(crate) fn reads(self) -> bool {
        matches!(self, Self::ReadWrite | Self::ReadOnly | Self::LocalOnly)
    }

    pub(crate) fn writes(self) -> bool {
        matches!(self, Self::ReadWrite | Self::WriteOnly | Self::LocalOnly)
    }
}

#[derive(Debug, Serialize)]
struct CacheKeyMaterial<'a> {
    format: u8,
    task: &'a str,
    run: &'a [RunEntry],
    args: &'a [String],
    shell: &'a Option<String>,
    outputs: Vec<String>,
    root: PathBuf,
    source_hash: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependency_keys: Vec<String>,
    environment: BTreeMap<String, Option<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    command_inputs: Vec<CommandInput>,
    vars: BTreeMap<String, String>,
    tools: Vec<String>,
    os: &'static str,
    arch: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct CommandInput {
    pub(crate) command: String,
    pub(crate) stdout_hash: String,
    pub(crate) stderr_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheManifest {
    format: u8,
    key: String,
    roots: Vec<PathBuf>,
    output: Vec<TaskCacheOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "stream", content = "line")]
pub(crate) enum TaskCacheOutput {
    Stdout(String),
    Stderr(String),
}

pub(crate) enum TaskCacheRestore {
    Hit(Vec<TaskCacheOutput>),
    Miss(TaskCacheMissReason),
}

pub(crate) enum TaskCacheMissReason {
    CorruptEntry,
    DependencyWithoutKey,
    EntryNotFound,
    Forced,
    ReadDisabled,
}

impl fmt::Display for TaskCacheMissReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CorruptEntry => "cache entry was corrupt",
            Self::DependencyWithoutKey => "dependency completed without a cache key",
            Self::EntryNotFound => "no matching cache entry",
            Self::Forced => "forced execution",
            Self::ReadDisabled => "cache reads are disabled",
        })
    }
}

pub struct TaskArtifactCache {
    root: PathBuf,
    cache_dir: PathBuf,
    key: String,
    explanation: Option<TaskCacheKeyExplanation>,
    state_path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct TaskCacheKeyExplanation {
    format: u8,
    source_paths: Vec<PathBuf>,
    output_patterns: Vec<String>,
    output_paths: Vec<PathBuf>,
    dependency_count: usize,
    environment: BTreeMap<String, bool>,
    command_input_count: usize,
    vars: Vec<String>,
    tool_count: usize,
    os: &'static str,
    arch: &'static str,
}

pub(crate) struct TaskArtifactCacheBuilder {
    root: PathBuf,
    inputs: TaskCacheInputs,
    output_roots: Vec<PathBuf>,
}

pub(crate) struct TaskCacheContext<'a> {
    pub(crate) task: &'a Task,
    pub(crate) config: &'a Arc<Config>,
    pub(crate) toolset: &'a Toolset,
    pub(crate) resolved_env: &'a BTreeMap<String, String>,
    pub(crate) declared_env: &'a [(String, String)],
    pub(crate) dependency_keys: &'a [String],
    pub(crate) command_inputs: Vec<CommandInput>,
    pub(crate) explain: bool,
}

impl TaskArtifactCache {
    pub(crate) async fn prepare(
        task: &Task,
        config: &Arc<Config>,
        dry_run: bool,
    ) -> Result<Option<TaskArtifactCacheBuilder>> {
        Settings::get().ensure_experimental("task artifact caching")?;
        let root = task_cwd(task, config).await?;
        validate_config(task, &root)?;
        let output_roots = resolve_output_roots(task, &root, false)?;
        for output in &output_roots {
            ensure_no_symlink_ancestors(&root, output)?;
        }
        let Some(inputs) = task_cache_inputs(task, config, !dry_run).await? else {
            warn!(
                "task {} has sources defined but no matching files found; artifact caching disabled",
                task.name
            );
            return Ok(None);
        };
        for source in &inputs.source_paths {
            if output_roots
                .iter()
                .any(|output| source == output || source.starts_with(output))
            {
                bail!(
                    "task {} cache outputs must not contain source {}",
                    task.name,
                    source.display()
                );
            }
        }
        Ok(Some(TaskArtifactCacheBuilder {
            root,
            inputs,
            output_roots,
        }))
    }
}

impl TaskArtifactCacheBuilder {
    /// Finishes cache-key construction after task tools, environment, and
    /// dependency artifacts have been resolved.
    pub(crate) async fn finish(self, ctx: TaskCacheContext<'_>) -> Result<TaskArtifactCache> {
        let TaskCacheContext {
            task,
            config,
            toolset,
            resolved_env,
            declared_env,
            dependency_keys,
            command_inputs,
            explain,
        } = ctx;
        let Self {
            root,
            inputs,
            output_roots,
        } = self;
        let mut environment = declared_env
            .iter()
            .map(|(key, _)| (key.clone(), resolved_env.get(key).cloned()))
            .collect::<BTreeMap<_, _>>();
        let cache_config = task.cache.as_ref().expect("cache must be configured");
        for key in &cache_config.env {
            environment.insert(key.clone(), resolved_env.get(key).cloned());
        }
        let vars = task
            .tera_ctx(config)
            .await?
            .get("vars")
            .map(|value| serde::Deserialize::deserialize(value.clone()))
            .transpose()?
            .unwrap_or_default();
        let mut tools = toolset
            .list_current_versions()
            .into_iter()
            .map(|(_, tv)| tv.to_string())
            .collect::<Vec<_>>();
        tools.sort();
        let source_paths = if explain {
            inputs.source_paths.clone()
        } else {
            Vec::new()
        };
        let material = CacheKeyMaterial {
            format: CACHE_FORMAT_VERSION,
            task: &task.name,
            run: task.run(),
            args: &task.args,
            shell: &task.shell,
            outputs: task.outputs.patterns(),
            root: inputs.root_identity,
            source_hash: inputs.source_hash,
            dependency_keys: dependency_keys.to_vec(),
            environment,
            command_inputs,
            vars,
            tools,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        };
        let encoded = serde_json::to_vec(&material)?;
        let key = hash::hash_blake3_to_str(std::str::from_utf8(&encoded)?);
        let explanation = if explain {
            Some(TaskCacheKeyExplanation {
                format: material.format,
                source_paths,
                output_patterns: material.outputs.clone(),
                output_paths: remove_nested_roots(output_roots),
                dependency_count: material.dependency_keys.len(),
                environment: material
                    .environment
                    .iter()
                    .map(|(name, value)| (name.clone(), value.is_some()))
                    .collect(),
                command_input_count: material.command_inputs.len(),
                vars: material.vars.keys().cloned().collect(),
                tool_count: material.tools.len(),
                os: material.os,
                arch: material.arch,
            })
        } else {
            None
        };
        let state_identity = hash::hash_blake3_to_str(&format!(
            "{}\0{}\0{}",
            root.display(),
            task.name,
            task.config_source.display()
        ));
        let state_path = dirs::STATE
            .join("task-artifacts")
            .join(format!("{state_identity}.key"));
        Ok(TaskArtifactCache {
            root,
            cache_dir: task_cache_dir(),
            key,
            explanation,
            state_path,
        })
    }
}

impl TaskArtifactCache {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn explanation(&self) -> Option<&TaskCacheKeyExplanation> {
        self.explanation.as_ref()
    }

    pub(crate) fn current_output(&self) -> Option<Vec<TaskCacheOutput>> {
        let (archive_path, manifest_path) = self.paths();
        if !manifest_path.is_file()
            || !file::read_to_string(&self.state_path).is_ok_and(|key| key.trim() == self.key)
        {
            return None;
        }
        let manifest = self.read_manifest().ok()?;
        if !manifest.roots.is_empty() && !archive_path.is_file() {
            return None;
        }
        Some(manifest.output)
    }

    pub fn mark_current(&self) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write(&self.state_path, &self.key)
    }

    pub(crate) fn restore(&self, task: &Task) -> Result<TaskCacheRestore> {
        let (archive_path, manifest_path) = self.paths();
        if !manifest_path.is_file() {
            return Ok(TaskCacheRestore::Miss(TaskCacheMissReason::EntryNotFound));
        }
        let restore = || -> Result<Vec<TaskCacheOutput>> {
            let manifest = self.read_manifest()?;
            for root in &manifest.roots {
                ensure_safe_relative(root)?;
            }
            if remove_nested_roots(manifest.roots.clone()) != manifest.roots {
                bail!("task cache manifest contains duplicate or nested roots");
            }
            if manifest.roots.is_empty() {
                if let Err(err) = file::touch_file(&manifest_path) {
                    warn!("failed to update task cache manifest access time: {err}");
                }
                return Ok(manifest.output);
            }
            // Serialize restores targeting the same working directory across
            // mise processes so validation and renames form one cooperative
            // critical section. Result-only entries never mutate the task root.
            let _output_lock = crate::lock_file::LockFile::new(
                &self.root.join(".mise-task-artifact-cache-output"),
            )
            .lock()?;
            if !archive_path.is_file() {
                bail!("task cache archive is missing");
            }

            let staging = tempfile::Builder::new()
                .prefix(".mise-task-cache-")
                .tempdir_in(&self.root)?;
            file::untar(
                &archive_path,
                staging.path(),
                ExtractionFormat::TarZst,
                &ExtractOptions {
                    preserve_mtime: false,
                    ..Default::default()
                },
            )?;
            for rel in &manifest.roots {
                let restored = staging.path().join(rel);
                if !restored.exists() && fs::symlink_metadata(&restored).is_err() {
                    bail!("task cache archive is missing {}", rel.display());
                }
                ensure_no_symlink_ancestors(staging.path(), rel)?;
                ensure_no_symlink_ancestors(&self.root, rel)?;
            }

            let mut remove = resolve_output_roots(task, &self.root, false)?;
            remove.extend(manifest.roots.iter().cloned());
            let remove = remove_nested_roots(remove);
            for rel in &remove {
                ensure_no_symlink_ancestors(&self.root, rel)?;
            }
            let output_matcher = build_output_matcher(&self.root, &task.outputs.patterns())?;
            install_transactionally(
                &self.root,
                staging.path(),
                &manifest.roots,
                &remove,
                Some(&output_matcher),
            )?;
            if let Err(err) = file::touch_file(&archive_path) {
                warn!("failed to update task cache archive access time: {err}");
            }
            if let Err(err) = file::touch_file(&manifest_path) {
                warn!("failed to update task cache manifest access time: {err}");
            }
            Ok(manifest.output)
        };

        match restore() {
            Ok(output) => Ok(TaskCacheRestore::Hit(output)),
            Err(err) => {
                warn!("ignoring corrupt task cache entry {}: {err}", self.key);
                let _ = file::remove_file(&archive_path);
                let _ = file::remove_file(&manifest_path);
                Ok(TaskCacheRestore::Miss(TaskCacheMissReason::CorruptEntry))
            }
        }
    }

    /// Stores a successful task's declared outputs and captured logs.
    pub(crate) fn store(&self, task: &Task, output: &[TaskCacheOutput]) -> Result<()> {
        let roots = resolve_output_roots(task, &self.root, true)?;
        let roots = remove_nested_roots(roots);
        for root in &roots {
            ensure_no_symlink_ancestors(&self.root, root)?;
        }

        file::create_dir_all(&self.cache_dir)?;
        let (archive_path, manifest_path) = self.paths();
        let nonce = crate::rand::random_string(8);
        let archive_partial = self
            .cache_dir
            .join(format!("{}.part-{nonce}.tar.zst", self.key));
        let manifest_partial = self
            .cache_dir
            .join(format!("{}.part-{nonce}.json", self.key));

        let manifest = CacheManifest {
            format: CACHE_FORMAT_VERSION,
            key: self.key.clone(),
            roots,
            output: output.to_vec(),
        };
        if !manifest.roots.is_empty() {
            let output_matcher = build_output_matcher(&self.root, &task.outputs.patterns())?;
            write_archive(
                &archive_partial,
                &self.root,
                &manifest.roots,
                &output_matcher,
            )?;
        }
        fs::write(&manifest_partial, serde_json::to_vec(&manifest)?)?;
        if !manifest.roots.is_empty() {
            file::rename(&archive_partial, &archive_path)?;
        } else {
            let _ = file::remove_file(&archive_path);
        }
        file::rename(&manifest_partial, &manifest_path)?;
        Ok(())
    }

    /// Returns this cache entry's archive and manifest paths.
    fn paths(&self) -> (PathBuf, PathBuf) {
        (
            self.cache_dir.join(format!("{}.tar.zst", self.key)),
            self.cache_dir.join(format!("{}.json", self.key)),
        )
    }

    fn read_manifest(&self) -> Result<CacheManifest> {
        let (_, manifest_path) = self.paths();
        let manifest: CacheManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
        if manifest.format != CACHE_FORMAT_VERSION || manifest.key != self.key {
            bail!("task cache manifest does not match cache key");
        }
        Ok(manifest)
    }
}

impl TaskCacheKeyExplanation {
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            "cache key inputs:".to_string(),
            format!("  format: {}", self.format),
            "  task definition: included".to_string(),
            format!("  sources: {} files", self.source_paths.len()),
        ];
        lines.extend(
            self.source_paths
                .iter()
                .map(|path| format!("    source: {}", display_cache_path(path))),
        );
        lines.push(format!("  output patterns: {}", self.output_patterns.len()));
        lines.extend(
            self.output_patterns
                .iter()
                .map(|pattern| format!("    pattern: {}", display_cache_text(pattern))),
        );
        lines.push(format!("  resolved outputs: {}", self.output_paths.len()));
        lines.extend(
            self.output_paths
                .iter()
                .map(|path| format!("    output: {}", display_cache_path(path))),
        );
        lines.push(format!(
            "  dependencies: {} artifact keys",
            self.dependency_count
        ));
        lines.extend(self.environment.iter().map(|(name, is_set)| {
            let state = if *is_set { "set" } else { "unset" };
            format!("  environment {name}: {state}")
        }));
        lines.push(format!("  command inputs: {}", self.command_input_count));
        lines.extend(self.vars.iter().map(|name| format!("  variable: {name}")));
        lines.push(format!("  tools: {} resolved versions", self.tool_count));
        lines.push(format!("  platform: {}-{}", self.os, self.arch));
        lines
    }
}

fn display_cache_path(path: &Path) -> String {
    display_cache_text(&crate::file::display_path(path))
}

fn display_cache_text(text: &str) -> String {
    text.escape_debug().to_string()
}

/// Returns the versioned directory containing task artifact cache entries.
pub(crate) fn task_cache_dir() -> PathBuf {
    Settings::get()
        .task
        .cache_dir
        .clone()
        .unwrap_or_else(|| dirs::CACHE.join("task-artifacts"))
        .join(CACHE_DIR_VERSION)
}

pub(crate) fn validate_config(task: &Task, root: &Path) -> Result<()> {
    if task.sources.is_empty() {
        bail!("task {} cache requires at least one source", task.name);
    }
    if task.outputs.is_auto() {
        bail!(
            "task {} cache requires explicit outputs or outputs = []",
            task.name
        );
    }
    if let Some(command) = task.cache.as_ref().and_then(|cache| {
        cache
            .command_inputs
            .iter()
            .find(|command| command.trim().is_empty())
    }) {
        bail!(
            "task {} cache command input must not be empty: {command:?}",
            task.name
        );
    }
    let patterns = task.outputs.patterns();
    if !patterns.is_empty() && output_glob_patterns(&patterns).is_empty() {
        bail!(
            "task {} cache outputs require at least one non-excluded pattern",
            task.name
        );
    }
    for output in &patterns {
        let path = if let Some(body) = output.strip_prefix('!') {
            PathBuf::from(body)
        } else if let Some(body) = output.strip_prefix("\\!") {
            PathBuf::from(format!("!{body}"))
        } else {
            PathBuf::from(&output)
        };
        ensure_safe_relative(&path).wrap_err_with(|| {
            format!(
                "task {} cache output must stay within {}: {output}",
                task.name,
                root.display()
            )
        })?;
    }
    build_output_matcher(root, &patterns)
        .wrap_err_with(|| format!("task {} has an invalid cache output pattern", task.name))?;
    Ok(())
}

fn ensure_safe_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("path must be a non-empty relative path");
    }
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("path must not escape the task directory");
    }
    if !path.components().any(|c| matches!(c, Component::Normal(_))) {
        bail!("path must identify an output beneath the task directory");
    }
    Ok(())
}

fn resolve_output_roots(task: &Task, root: &Path, require_matches: bool) -> Result<Vec<PathBuf>> {
    let mut resolved = BTreeSet::new();
    let patterns = task.outputs.patterns();
    let matcher = build_output_matcher(root, &patterns)?;
    for output in output_glob_patterns(&patterns) {
        ensure_safe_relative(Path::new(&output))?;
        if crate::task::task_source_checker::is_glob_pattern(&output) {
            let mut glob_matched = false;
            for expanded in expand_glob_braces(&output)? {
                ensure_safe_relative(Path::new(&expanded))?;
                for entry in glob(root.join(expanded).to_str().unwrap_or_default())? {
                    let path = entry?;
                    glob_matched = true;
                    let rel = path.strip_prefix(root)?.to_path_buf();
                    ensure_safe_relative(&rel)?;
                    let is_dir = fs::symlink_metadata(&path)?.is_dir();
                    if is_output(&matcher, &path, is_dir) {
                        resolved.insert(rel);
                    }
                }
            }
            if require_matches && !glob_matched {
                bail!("output pattern {output:?} matched no files");
            }
        } else {
            let rel = PathBuf::from(&output);
            let abs = root.join(&rel);
            let is_dir = fs::symlink_metadata(&abs)
                .map(|metadata| metadata.is_dir())
                .unwrap_or(false);
            if !is_output(&matcher, &abs, is_dir) {
                continue;
            }
            if require_matches && !abs.exists() && fs::symlink_metadata(&abs).is_err() {
                bail!("output {} does not exist", rel.display());
            }
            resolved.insert(rel);
        }
    }
    Ok(resolved.into_iter().collect())
}

fn remove_nested_roots(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort_by_key(|path| path.components().count());
    let mut result = Vec::<PathBuf>::new();
    for root in roots {
        if !result.iter().any(|parent| root.starts_with(parent)) {
            result.push(root);
        }
    }
    result.sort();
    result
}

fn ensure_no_symlink_ancestors(root: &Path, rel: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    let component_count = rel.components().count();
    for component in rel.components().take(component_count.saturating_sub(1)) {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "output path {} traverses symlink ancestor {}",
                    rel.display(),
                    current.display()
                );
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn install_transactionally(
    root: &Path,
    staging: &Path,
    install_roots: &[PathBuf],
    remove_roots: &[PathBuf],
    output_matcher: Option<&Override>,
) -> Result<()> {
    let backup = tempfile::Builder::new()
        .prefix(".mise-task-cache-backup-")
        .tempdir_in(root)?;
    let mut backed_up = Vec::new();
    for rel in remove_roots {
        let from = root.join(rel);
        match fs::symlink_metadata(&from) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                let err = eyre!(
                    "failed to inspect existing output {}: {err}",
                    from.display()
                );
                return rollback_after_error(backup, root, &[], &backed_up, err);
            }
        }
        let to = backup.path().join(rel);
        let backup_output = (|| -> Result<()> {
            if let Some(parent) = to.parent() {
                file::create_dir_all(parent)?;
            }
            fs::rename(&from, &to).wrap_err_with(|| format!("failed to back up {}", rel.display()))
        })();
        if let Err(err) = backup_output {
            return rollback_after_error(backup, root, &[], &backed_up, err);
        }
        backed_up.push(rel.clone());
    }

    if let Some(matcher) = output_matcher
        && let Err(err) = copy_excluded_outputs(backup.path(), staging, &backed_up, matcher)
    {
        return rollback_after_error(backup, root, &[], &backed_up, err);
    }

    let mut installed = Vec::new();
    for rel in install_roots {
        let from = staging.join(rel);
        let to = root.join(rel);
        let install = (|| -> Result<()> {
            ensure_no_symlink_ancestors(root, rel)?;
            if let Some(parent) = to.parent() {
                file::create_dir_all(parent)?;
            }
            fs::rename(&from, &to)
                .wrap_err_with(|| format!("failed to install cached output {}", rel.display()))
        })();
        if let Err(err) = install {
            return rollback_after_error(backup, root, &installed, &backed_up, err);
        }
        installed.push(rel.clone());
    }
    Ok(())
}

/// Merge files excluded from the cache back into the staged artifact before
/// replacing output roots. This keeps local-only output files intact on a
/// cache hit while selected files still come entirely from the artifact.
fn copy_excluded_outputs(
    backup: &Path,
    staging: &Path,
    roots: &[PathBuf],
    matcher: &Override,
) -> Result<()> {
    let mut directories = Vec::new();
    for root in roots {
        let backup_root = backup.join(root);
        for entry in WalkDir::new(&backup_root).follow_links(false) {
            let entry = entry?;
            let from = entry.path();
            let rel = from.strip_prefix(backup)?;
            let metadata = fs::symlink_metadata(from)?;
            if is_output(matcher, &matcher.path().join(rel), metadata.is_dir()) {
                continue;
            }
            let to = staging.join(rel);
            ensure_no_symlink_ancestors(staging, rel)?;
            if metadata.is_dir() {
                file::create_dir_all(&to)?;
                directories.push((
                    to,
                    metadata.permissions(),
                    filetime::FileTime::from_last_access_time(&metadata),
                    filetime::FileTime::from_last_modification_time(&metadata),
                ));
            } else if metadata.file_type().is_symlink() {
                if let Some(parent) = to.parent() {
                    file::create_dir_all(parent)?;
                }
                if fs::symlink_metadata(&to).is_err() {
                    file::make_symlink(&fs::read_link(from)?, &to)?;
                }
            } else if metadata.is_file() {
                if let Some(parent) = to.parent() {
                    file::create_dir_all(parent)?;
                }
                fs::copy(from, &to)?;
                fs::set_permissions(&to, metadata.permissions())?;
                filetime::set_file_times(
                    &to,
                    filetime::FileTime::from_last_access_time(&metadata),
                    filetime::FileTime::from_last_modification_time(&metadata),
                )?;
            }
        }
    }
    for (path, permissions, accessed, modified) in directories.into_iter().rev() {
        fs::set_permissions(&path, permissions)?;
        filetime::set_file_times(&path, accessed, modified)?;
    }
    Ok(())
}

fn rollback_after_error(
    backup: tempfile::TempDir,
    root: &Path,
    installed: &[PathBuf],
    backed_up: &[PathBuf],
    err: Report,
) -> Result<()> {
    if rollback_install(root, backup.path(), installed, backed_up) {
        Err(err)
    } else {
        let backup = backup.keep();
        Err(err).wrap_err_with(|| {
            format!(
                "cache restore rollback was incomplete; original outputs were preserved at {}",
                backup.display()
            )
        })
    }
}

fn rollback_install(
    root: &Path,
    backup: &Path,
    installed: &[PathBuf],
    backed_up: &[PathBuf],
) -> bool {
    let mut complete = true;
    for rel in installed.iter().rev() {
        if let Err(err) = file::remove_all(root.join(rel)) {
            complete = false;
            warn!(
                "failed to remove partial cache restore {}: {err}",
                rel.display()
            );
        }
    }
    for rel in backed_up.iter().rev() {
        let from = backup.join(rel);
        let to = root.join(rel);
        if let Some(parent) = to.parent()
            && let Err(err) = file::create_dir_all(parent)
        {
            complete = false;
            warn!(
                "failed to prepare cache restore rollback {}: {err}",
                rel.display()
            );
            continue;
        }
        if let Err(err) = fs::rename(&from, &to) {
            complete = false;
            warn!("failed to roll back cached output {}: {err}", rel.display());
        }
    }
    complete
}

fn write_archive(
    path: &Path,
    root: &Path,
    roots: &[PathBuf],
    output_matcher: &Override,
) -> Result<()> {
    let file = File::create(path)?;
    let encoder = zstd::Encoder::new(file, 0)?;
    let mut archive = Builder::new(encoder);
    let mut entries = BTreeMap::<PathBuf, PathBuf>::new();
    for rel_root in roots {
        let abs_root = root.join(rel_root);
        for entry in WalkDir::new(&abs_root).follow_links(false) {
            let entry = entry?;
            let abs = entry.path().to_path_buf();
            let rel = abs.strip_prefix(root)?.to_path_buf();
            ensure_safe_relative(&rel)?;
            if is_output(output_matcher, &abs, entry.file_type().is_dir()) {
                entries.insert(rel, abs);
            }
        }
    }

    for (rel, abs) in entries {
        let metadata = fs::symlink_metadata(&abs)?;
        let mut header = Header::new_gnu(if metadata.file_type().is_symlink() {
            EntryType::Symlink
        } else if metadata.is_dir() {
            EntryType::Directory
        } else {
            EntryType::File
        });
        header.set_mode(metadata_mode(&metadata));
        header.set_mtime(
            metadata
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
        if metadata.file_type().is_symlink() {
            header.set_size(0);
            archive.append_link(&mut header, &rel, fs::read_link(&abs)?)?;
        } else if metadata.is_dir() {
            header.set_size(0);
            archive.append_data(&mut header, &rel, std::io::empty())?;
        } else if metadata.is_file() {
            header.set_size(metadata.len());
            archive.append_data(&mut header, &rel, File::open(&abs)?)?;
        } else {
            bail!("unsupported output file type: {}", rel.display());
        }
    }
    let encoder = archive.into_inner()?;
    encoder.finish()?;
    Ok(())
}

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    if metadata.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_deserializes_and_rejects_unknown_fields() {
        let config: TaskCacheConfig = toml::from_str(
            "enabled = true\nenv = ['PROFILE']\ncommand_inputs = ['node --version']",
        )
        .unwrap();
        assert!(config.enabled);
        assert_eq!(config.env, ["PROFILE"]);
        assert_eq!(config.command_inputs, ["node --version"]);
        assert!(toml::from_str::<TaskCacheConfig>("remote = true").is_err());
    }

    #[test]
    fn cache_explanation_omits_environment_and_variable_values() {
        let explanation = TaskCacheKeyExplanation {
            format: 2,
            source_paths: vec![
                PathBuf::from("input.txt"),
                PathBuf::from("src/\x1b[2J\nfile.rs"),
            ],
            output_patterns: vec!["dist".into(), "!dist/private/**".into()],
            output_paths: vec![PathBuf::from("dist")],
            dependency_count: 1,
            environment: BTreeMap::from([("MISSING".into(), false), ("TOKEN".into(), true)]),
            command_input_count: 2,
            vars: vec!["password".into()],
            tool_count: 4,
            os: "linux",
            arch: "x86_64",
        };

        let output = explanation.lines().join("\n");
        assert!(output.contains("environment TOKEN: set"));
        assert!(output.contains("environment MISSING: unset"));
        assert!(output.contains("variable: password"));
        assert!(output.contains("sources: 2 files"));
        assert!(output.contains("source: input.txt"));
        assert!(output.contains(r"source: src/\u{1b}[2J\nfile.rs"));
        assert!(output.contains("pattern: !dist/private/**"));
        assert!(output.contains("output: dist"));
        assert!(output.contains("dependencies: 1 artifact keys"));
        assert!(output.contains("command inputs: 2"));
        assert!(output.contains("tools: 4 resolved versions"));
        assert!(!output.contains("secret"));
        assert!(!output.contains("hunter2"));
    }

    #[test]
    fn cache_miss_reasons_are_human_readable() {
        assert_eq!(
            TaskCacheMissReason::CorruptEntry.to_string(),
            "cache entry was corrupt"
        );
        assert_eq!(
            TaskCacheMissReason::DependencyWithoutKey.to_string(),
            "dependency completed without a cache key"
        );
        assert_eq!(
            TaskCacheMissReason::EntryNotFound.to_string(),
            "no matching cache entry"
        );
        assert_eq!(TaskCacheMissReason::Forced.to_string(), "forced execution");
        assert_eq!(
            TaskCacheMissReason::ReadDisabled.to_string(),
            "cache reads are disabled"
        );
    }

    #[test]
    fn safe_relative_paths_reject_escapes() {
        assert!(ensure_safe_relative(Path::new("dist/app")).is_ok());
        assert!(ensure_safe_relative(Path::new("../dist")).is_err());
        assert!(ensure_safe_relative(Path::new("/tmp/dist")).is_err());
        assert!(ensure_safe_relative(Path::new("")).is_err());
        assert!(ensure_safe_relative(Path::new(".")).is_err());
    }

    #[test]
    fn nested_output_roots_are_collapsed() {
        assert_eq!(
            remove_nested_roots(vec![
                PathBuf::from("dist/a.js"),
                PathBuf::from("dist"),
                PathBuf::from("coverage"),
            ]),
            vec![PathBuf::from("coverage"), PathBuf::from("dist")]
        );
    }

    #[test]
    fn output_roots_allow_glob_matches_that_are_all_excluded() {
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

        assert_eq!(
            resolve_output_roots(&task, root.path(), true).unwrap(),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn output_roots_support_brace_globs() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dist/client")).unwrap();
        fs::create_dir_all(root.path().join("dist/server")).unwrap();
        fs::write(root.path().join("dist/client/app.js"), "client").unwrap();
        fs::write(root.path().join("dist/server/app.js"), "server").unwrap();
        let task = Task {
            outputs: crate::task::task_sources::TaskOutputs::Files(vec![
                "dist/{client,server}/**/*.js".to_string(),
            ]),
            ..Default::default()
        };

        assert_eq!(
            resolve_output_roots(&task, root.path(), true).unwrap(),
            [
                PathBuf::from("dist/client/app.js"),
                PathBuf::from("dist/server/app.js"),
            ]
        );
    }

    #[test]
    fn output_roots_reject_unsafe_brace_expansions() {
        let root = tempfile::tempdir().unwrap();
        let task = Task {
            outputs: crate::task::task_sources::TaskOutputs::Files(vec![
                "{..,ok}/secret.txt".to_string(),
            ]),
            ..Default::default()
        };

        assert!(resolve_output_roots(&task, root.path(), false).is_err());
    }

    #[test]
    fn failed_install_restores_previous_outputs() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/result.txt"), "old").unwrap();
        fs::create_dir(staging.path().join("dist")).unwrap();
        fs::write(staging.path().join("dist/result.txt"), "new").unwrap();

        assert!(
            install_transactionally(
                root.path(),
                staging.path(),
                &[PathBuf::from("dist"), PathBuf::from("missing")],
                &[PathBuf::from("dist")],
                None,
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("dist/result.txt")).unwrap(),
            "old"
        );
    }

    #[test]
    fn install_preserves_outputs_excluded_from_cache() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        fs::create_dir_all(root.path().join("dist/private")).unwrap();
        fs::write(root.path().join("dist/result.txt"), "old").unwrap();
        fs::write(root.path().join("dist/result.map"), "local map").unwrap();
        fs::write(root.path().join("dist/private/token"), "local token").unwrap();
        fs::create_dir(staging.path().join("dist")).unwrap();
        fs::write(staging.path().join("dist/result.txt"), "cached").unwrap();
        let matcher = build_output_matcher(
            root.path(),
            &[
                "dist".to_string(),
                "!dist/**/*.map".to_string(),
                "!dist/private/**".to_string(),
            ],
        )
        .unwrap();

        install_transactionally(
            root.path(),
            staging.path(),
            &[PathBuf::from("dist")],
            &[PathBuf::from("dist")],
            Some(&matcher),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.path().join("dist/result.txt")).unwrap(),
            "cached"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("dist/result.map")).unwrap(),
            "local map"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("dist/private/token")).unwrap(),
            "local token"
        );
    }

    #[test]
    fn incomplete_rollback_preserves_backup_directory() {
        let root = tempfile::tempdir().unwrap();
        let backup = tempfile::tempdir_in(root.path()).unwrap();
        let backup_path = backup.path().to_path_buf();
        fs::create_dir(backup.path().join("blocked")).unwrap();
        fs::write(backup.path().join("blocked/output"), "old").unwrap();
        fs::write(root.path().join("blocked"), "not a directory").unwrap();

        let err = rollback_after_error(
            backup,
            root.path(),
            &[],
            &[PathBuf::from("blocked/output")],
            eyre!("install failed"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("original outputs were preserved"));
        assert_eq!(
            fs::read_to_string(backup_path.join("blocked/output")).unwrap(),
            "old"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("build")).unwrap();

        assert!(ensure_no_symlink_ancestors(root.path(), Path::new("build/dist")).is_err());
        assert!(ensure_no_symlink_ancestors(root.path(), Path::new("build")).is_ok());
    }
}
