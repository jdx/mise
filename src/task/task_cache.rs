use crate::config::{Config, Settings};
use crate::dirs;
use crate::duration;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::hash;
use crate::task::task_cache_store::{
    LocalTaskCacheStore, RemoteTaskCacheConfig, TASK_CACHE_STORE_VERSION, TaskCacheStore,
    compose_task_cache_stores,
};
use crate::task::task_source_checker::{
    TaskCacheInputs, build_output_matcher, expand_enumeration_patterns, is_output,
    output_glob_patterns, task_cache_inputs, task_cwd,
};
use crate::task::{RunEntry, Task};
use crate::toolset::Toolset;
use bytesize::ByteSize;
use eyre::{Context, Report, Result, bail, eyre};
use glob::glob;
use ignore::overrides::Override;
use jdx_tar::{Builder, EntryType, Header};
use mise_cache_core::RemoteCacheConfig;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub(crate) const CACHE_FORMAT_VERSION: u8 = 2;
const CACHE_DIR_VERSION: &str = "v2";
const ARTIFACT_CHECKSUM_FORMAT: u8 = 1;
const TASK_ACTION_VERSION: u8 = 1;

static CLEANED_PARTIAL_CACHE_DIRS: LazyLock<Mutex<BTreeSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskCacheConfig {
    pub enabled: bool,
    /// Report project files read or written outside the declared cache contract.
    pub audit: bool,
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
    #[serde(rename = "version")]
    format: u8,
    kind: &'static str,
    task: &'a str,
    phase: crate::task::TaskRunPhase,
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
pub(crate) struct CacheManifest {
    pub(crate) format: u8,
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) task_identity: String,
    #[serde(default)]
    pub(crate) artifact_checksum: Option<String>,
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) output: Vec<TaskCacheOutput>,
    #[serde(default)]
    pub(crate) restored_bytes: u64,
    #[serde(default)]
    pub(crate) execution_duration_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "stream", content = "line")]
pub(crate) enum TaskCacheOutput {
    Stdout(String),
    Stderr(String),
}

pub(crate) enum TaskCacheRestore {
    Hit(TaskCacheHit),
    Miss(TaskCacheMissReason),
}

pub(crate) struct TaskCacheHit {
    pub(crate) output: Vec<TaskCacheOutput>,
    pub(crate) restored_bytes: u64,
    pub(crate) saved_duration: std::time::Duration,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskCacheEntry {
    pub(crate) key: String,
    #[serde(skip)]
    pub(crate) identity_verified: bool,
    pub(crate) artifact_checksum: Option<String>,
    pub(crate) current: bool,
    pub(crate) size_bytes: u64,
    pub(crate) restored_bytes: u64,
    pub(crate) execution_duration_ns: u64,
    pub(crate) last_accessed: u64,
    pub(crate) outputs: Vec<PathBuf>,
}

pub(crate) struct TaskCacheClearResult {
    pub(crate) entries: usize,
    pub(crate) size_bytes: u64,
}

pub(crate) enum TaskCacheMissReason {
    CorruptEntry,
    DependencyWithoutKey,
    EntryNotFound,
    Expired,
    Forced,
    ReadDisabled,
}

impl fmt::Display for TaskCacheMissReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CorruptEntry => "cache entry was corrupt",
            Self::DependencyWithoutKey => "dependency completed without a cache key",
            Self::EntryNotFound => "no matching cache entry",
            Self::Expired => "cache entry exceeded its age limit",
            Self::Forced => "forced execution",
            Self::ReadDisabled => "cache reads are disabled",
        })
    }
}

pub struct TaskArtifactCache {
    root: PathBuf,
    cache_dir: PathBuf,
    store: Arc<dyn TaskCacheStore>,
    key: String,
    action: Vec<u8>,
    explanation: Option<TaskCacheKeyExplanation>,
    state_path: PathBuf,
    limits: TaskCacheLimits,
}

#[derive(Clone, Copy, Default)]
struct TaskCacheLimits {
    max_size: Option<u64>,
    max_age: Option<Duration>,
}

impl TaskCacheLimits {
    fn configured(self) -> bool {
        self.max_size.is_some() || self.max_age.is_some()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskCacheKeyExplanation {
    format: u8,
    action_version: u8,
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
    pub(crate) mode: TaskCacheMode,
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
            mode,
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
            format: TASK_ACTION_VERSION,
            kind: "task",
            task: &task.name,
            phase: task.run_phase,
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
        let encoded = canonical_json(&serde_json::to_value(&material)?)?;
        let key = hash::hash_blake3_to_str(std::str::from_utf8(&encoded)?);
        let explanation = if explain {
            Some(TaskCacheKeyExplanation {
                format: CACHE_FORMAT_VERSION,
                action_version: material.format,
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
        let state_path = task_cache_state_path(task, &root);
        let cache_dir = task_cache_dir();
        let limits = task_cache_limits()?;
        cleanup_abandoned_partial_writes_once(&cache_dir);
        let local: Arc<dyn TaskCacheStore> = Arc::new(LocalTaskCacheStore::new(cache_dir.clone()));
        let settings = Settings::get();
        let remote = if mode == TaskCacheMode::LocalOnly {
            None
        } else if let Some(base_url) = settings.task.cache.remote_url.clone() {
            let namespace = settings
                .task
                .cache
                .remote_namespace
                .as_deref()
                .map(str::trim)
                .filter(|namespace| !namespace.is_empty())
                .ok_or_else(|| {
                    eyre!(
                        "task.cache.remote_namespace is required when task.cache.remote_url is set"
                    )
                })?
                .to_string();
            let mode = crate::cache::effective_remote_cache_mode(settings.task.cache.remote_mode);
            let base_url = base_url.parse().wrap_err("invalid task.cache.remote_url")?;
            mode.map(|mode| RemoteTaskCacheConfig {
                remote: RemoteCacheConfig {
                    base_url,
                    namespace,
                    token: settings.task.cache.remote_token.clone(),
                    token_file: settings.task.cache.remote_token_file.clone(),
                    oidc_audience: settings.task.cache.remote_oidc_audience.clone(),
                    connect_timeout: settings.http_timeout(),
                    read_timeout: settings.http_timeout(),
                    download_timeout: settings.http_download_timeout(),
                    retries: settings.http_retries(),
                },
                staging_dir: cache_dir.join("remote"),
                mode,
            })
        } else {
            None
        };
        let store = compose_task_cache_stores(local, remote)?;
        if store.version() != TASK_CACHE_STORE_VERSION {
            bail!(
                "unsupported task cache store version {}; expected {}",
                store.version(),
                TASK_CACHE_STORE_VERSION
            );
        }
        Ok(TaskArtifactCache {
            root,
            cache_dir,
            store,
            key,
            action: encoded,
            explanation,
            state_path,
            limits,
        })
    }
}

pub(super) fn canonical_json(value: &serde_json::Value) -> Result<Vec<u8>> {
    fn write(value: &serde_json::Value, output: &mut Vec<u8>) -> Result<()> {
        match value {
            serde_json::Value::Null => output.extend_from_slice(b"null"),
            serde_json::Value::Bool(value) => {
                output.extend_from_slice(if *value { b"true" } else { b"false" })
            }
            serde_json::Value::Number(value) => {
                output.extend_from_slice(value.to_string().as_bytes())
            }
            serde_json::Value::String(value) => serde_json::to_writer(output, value)?,
            serde_json::Value::Array(values) => {
                output.push(b'[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    write(value, output)?;
                }
                output.push(b']');
            }
            serde_json::Value::Object(values) => {
                output.push(b'{');
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key)?;
                    output.push(b':');
                    write(&values[key], output)?;
                }
                output.push(b'}');
            }
        }
        Ok(())
    }

    let mut output = Vec::new();
    write(value, &mut output)?;
    Ok(output)
}

impl TaskArtifactCache {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn explanation(&self) -> Option<&TaskCacheKeyExplanation> {
        self.explanation.as_ref()
    }

    pub(crate) async fn current_output(&self) -> Option<Vec<TaskCacheOutput>> {
        let _entry_lock = self.entry_lock().ok()?;
        if !file::read_to_string(&self.state_path).is_ok_and(|key| key.trim() == self.key)
            || self.exceeded_max_age().ok()?
        {
            return None;
        }
        let entry = self
            .store
            .get(&self.key, self.action.len() as u64)
            .await
            .ok()??;
        let manifest = self.read_manifest(&entry.manifest).ok()?;
        if !manifest.roots.is_empty() && entry.artifact.is_none() {
            return None;
        }
        verify_artifact_checksum(
            &manifest,
            entry.artifact.as_ref().map(|artifact| artifact.path()),
        )
        .ok()?;
        self.store.touch(&self.key);
        Some(manifest.output)
    }

    pub fn mark_current(&self) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write(&self.state_path, &self.key)
    }

    pub(crate) async fn restore(&self, task: &Task) -> Result<TaskCacheRestore> {
        let _entry_lock = self.entry_lock()?;
        let entry = match self.store.get(&self.key, self.action.len() as u64).await {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                return Ok(TaskCacheRestore::Miss(TaskCacheMissReason::EntryNotFound));
            }
            Err(err) => {
                warn!("ignoring unreadable task cache entry {}: {err}", self.key);
                let _ = self.store.remove(&self.key).await;
                return Ok(TaskCacheRestore::Miss(TaskCacheMissReason::CorruptEntry));
            }
        };
        if self.exceeded_max_age()? {
            if let Err(err) = self
                .store
                .remove_local(&self.key, self.action.len() as u64)
                .await
            {
                warn!(
                    "failed to remove expired local task cache entry {}: {err}",
                    self.key
                );
            }
            return Ok(TaskCacheRestore::Miss(TaskCacheMissReason::Expired));
        }
        let restore = || -> Result<TaskCacheHit> {
            let manifest = self.read_manifest(&entry.manifest)?;
            for root in &manifest.roots {
                ensure_safe_relative(root)?;
            }
            if remove_nested_roots(manifest.roots.clone()) != manifest.roots {
                bail!("task cache manifest contains duplicate or nested roots");
            }
            verify_artifact_checksum(
                &manifest,
                entry.artifact.as_ref().map(|artifact| artifact.path()),
            )?;
            if manifest.roots.is_empty() {
                self.store.touch(&self.key);
                return Ok(TaskCacheHit {
                    output: manifest.output,
                    restored_bytes: manifest.restored_bytes,
                    saved_duration: std::time::Duration::from_nanos(manifest.execution_duration_ns),
                });
            }
            // Serialize restores targeting the same working directory across
            // mise processes so validation and renames form one cooperative
            // critical section. Result-only entries never mutate the task root.
            let _output_lock = crate::lock_file::LockFile::new(
                &self.root.join(".mise-task-artifact-cache-output"),
            )
            .lock()?;
            let archive_path = entry
                .artifact
                .as_ref()
                .map(|artifact| artifact.path())
                .ok_or_else(|| eyre!("task cache archive is missing"))?;

            let staging = tempfile::Builder::new()
                .prefix(".mise-task-cache-")
                .tempdir_in(&self.root)?;
            file::untar(
                archive_path,
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
            self.store.touch(&self.key);
            Ok(TaskCacheHit {
                output: manifest.output,
                restored_bytes: manifest.restored_bytes,
                saved_duration: std::time::Duration::from_nanos(manifest.execution_duration_ns),
            })
        };

        match restore() {
            Ok(output) => Ok(TaskCacheRestore::Hit(output)),
            Err(err) => {
                warn!("ignoring corrupt task cache entry {}: {err}", self.key);
                let _ = self.store.remove(&self.key).await;
                Ok(TaskCacheRestore::Miss(TaskCacheMissReason::CorruptEntry))
            }
        }
    }

    fn exceeded_max_age(&self) -> Result<bool> {
        let Some(max_age) = self.limits.max_age else {
            return Ok(false);
        };
        Ok(task_cache_limit_entry(&self.cache_dir, &self.key)?
            .is_some_and(|entry| entry.last_accessed.elapsed().unwrap_or_default() > max_age))
    }

    /// Stores a successful task's declared outputs and captured logs.
    pub(crate) async fn store(
        &self,
        task: &Task,
        output: &[TaskCacheOutput],
        execution_duration: std::time::Duration,
    ) -> Result<()> {
        let roots = resolve_output_roots(task, &self.root, true)?;
        let roots = remove_nested_roots(roots);
        for root in &roots {
            ensure_no_symlink_ancestors(&self.root, root)?;
        }

        let entry_lock = self.entry_lock()?;
        let write = self.store.begin_write(&self.key)?;

        let output_bytes = output.iter().fold(0_u64, |total, line| {
            let bytes = match line {
                TaskCacheOutput::Stdout(line) | TaskCacheOutput::Stderr(line) => line.len() as u64,
            };
            total.saturating_add(bytes)
        });
        let archive_bytes = if !roots.is_empty() {
            let output_matcher = build_output_matcher(&self.root, &task.outputs.patterns())?;
            write_archive(write.artifact_path(), &self.root, &roots, &output_matcher)?
        } else {
            0
        };
        let mut manifest = CacheManifest {
            format: CACHE_FORMAT_VERSION,
            key: self.key.clone(),
            task_identity: task_cache_identity(task, &self.root),
            artifact_checksum: None,
            roots,
            output: output.to_vec(),
            restored_bytes: archive_bytes.saturating_add(output_bytes),
            execution_duration_ns: execution_duration.as_nanos().min(u64::MAX as u128) as u64,
        };
        manifest.artifact_checksum = Some(calculate_artifact_checksum(
            &manifest,
            (!manifest.roots.is_empty()).then_some(write.artifact_path()),
        )?);
        self.store
            .commit(
                &self.key,
                &self.action,
                &write,
                &serde_json::to_vec(&manifest)?,
                !manifest.roots.is_empty(),
            )
            .await?;
        drop(entry_lock);
        if self.limits.configured()
            && let Err(err) = enforce_task_cache_limits(&self.cache_dir, self.limits)
        {
            warn!(
                "failed to enforce task cache limits in {}: {err}",
                self.cache_dir.display()
            );
        }
        Ok(())
    }

    fn entry_lock(&self) -> Result<fslock::LockFile> {
        task_cache_entry_lock(&self.cache_dir, &self.key)
    }

    fn read_manifest(&self, contents: &[u8]) -> Result<CacheManifest> {
        let manifest: CacheManifest = serde_json::from_slice(contents)?;
        if manifest.format != CACHE_FORMAT_VERSION || manifest.key != self.key {
            bail!("task cache manifest does not match cache key");
        }
        Ok(manifest)
    }
}

fn cleanup_abandoned_partial_writes_once(cache_dir: &Path) {
    if !cache_dir.is_dir() {
        return;
    }
    let should_clean = CLEANED_PARTIAL_CACHE_DIRS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(cache_dir.to_path_buf());
    if should_clean && let Err(err) = cleanup_abandoned_partial_writes(cache_dir) {
        CLEANED_PARTIAL_CACHE_DIRS
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(cache_dir);
        warn!(
            "failed to clean abandoned task cache writes in {}: {err}",
            cache_dir.display()
        );
    }
}

fn cleanup_abandoned_partial_writes(cache_dir: &Path) -> Result<()> {
    let entries = match fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let mut candidates = BTreeMap::<String, Vec<PathBuf>>::new();
    for entry in entries {
        let path = entry?.path();
        if let Some(key) = partial_cache_key(&path) {
            candidates.entry(key.to_string()).or_default().push(path);
        }
    }
    for (key, paths) in candidates {
        let _entry_lock = task_cache_entry_lock(cache_dir, &key)?;
        for path in paths {
            if path.exists() {
                remove_cache_file(&path)?;
            }
        }
    }
    Ok(())
}

fn partial_cache_key(path: &Path) -> Option<&str> {
    let filename = path.file_name()?.to_str()?;
    let stem = filename
        .strip_suffix(".tar.zst")
        .or_else(|| filename.strip_suffix(".json"))?;
    let (key, nonce) = stem.split_once(".part-")?;
    (!key.is_empty() && !nonce.is_empty()).then_some(key)
}

fn task_cache_limits() -> Result<TaskCacheLimits> {
    let settings = Settings::get();
    let max_age = settings
        .task
        .cache_max_age
        .as_deref()
        .map(duration::parse_duration)
        .transpose()
        .wrap_err("invalid task.cache_max_age")?
        .filter(|age| !age.is_zero());
    let max_size = settings
        .task
        .cache_max_size
        .as_deref()
        .map(|size| {
            size.parse::<ByteSize>()
                .map(|size| size.as_u64())
                .map_err(|err| eyre!(err))
        })
        .transpose()
        .wrap_err("invalid task.cache_max_size")?
        .filter(|size| *size > 0);
    Ok(TaskCacheLimits { max_size, max_age })
}

struct TaskCacheLimitEntry {
    key: String,
    size: u64,
    last_accessed: SystemTime,
}

fn enforce_task_cache_limits(cache_dir: &Path, limits: TaskCacheLimits) -> Result<()> {
    let mut entries = task_cache_limit_entries(cache_dir)?;
    entries.sort_by(|a, b| {
        a.last_accessed
            .cmp(&b.last_accessed)
            .then_with(|| a.key.cmp(&b.key))
    });
    let mut total_size = entries
        .iter()
        .fold(0_u64, |total, entry| total.saturating_add(entry.size));
    let mut remove = BTreeSet::new();
    if let Some(max_age) = limits.max_age {
        for entry in &entries {
            if entry.last_accessed.elapsed().unwrap_or_default() > max_age {
                remove.insert(entry.key.clone());
                total_size = total_size.saturating_sub(entry.size);
            }
        }
    }
    if let Some(max_size) = limits.max_size {
        for entry in &entries {
            if total_size <= max_size {
                break;
            }
            if remove.insert(entry.key.clone()) {
                total_size = total_size.saturating_sub(entry.size);
            }
        }
    }
    for entry in entries.iter().filter(|entry| remove.contains(&entry.key)) {
        let _entry_lock = task_cache_entry_lock(cache_dir, &entry.key)?;
        let Some(current) = task_cache_limit_entry(cache_dir, &entry.key)? else {
            continue;
        };
        if current.last_accessed > entry.last_accessed {
            continue;
        }
        remove_cache_file(&cache_dir.join(format!("{}.tar.zst", entry.key)))?;
        remove_cache_file(&cache_dir.join(format!("{}.json", entry.key)))?;
    }
    Ok(())
}

fn task_cache_limit_entries(cache_dir: &Path) -> Result<Vec<TaskCacheLimitEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(cache_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json")
            || partial_cache_key(&path).is_some()
        {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if let Some(entry) = task_cache_limit_entry(cache_dir, key)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn task_cache_limit_entry(cache_dir: &Path, key: &str) -> Result<Option<TaskCacheLimitEntry>> {
    let manifest_path = cache_dir.join(format!("{key}.json"));
    let manifest = match fs::metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let archive = fs::metadata(cache_dir.join(format!("{key}.tar.zst"))).ok();
    let last_accessed = archive
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .into_iter()
        .chain(manifest.modified().ok())
        .max()
        .unwrap_or(UNIX_EPOCH);
    Ok(Some(TaskCacheLimitEntry {
        key: key.to_string(),
        size: manifest
            .len()
            .saturating_add(archive.map_or(0, |metadata| metadata.len())),
        last_accessed,
    }))
}

pub(crate) fn calculate_artifact_checksum(
    manifest: &CacheManifest,
    archive_path: Option<&Path>,
) -> Result<String> {
    #[derive(Serialize)]
    struct ArtifactChecksumMaterial<'a> {
        format: u8,
        roots: &'a [PathBuf],
        output: &'a [TaskCacheOutput],
        restored_bytes: u64,
        execution_duration_ns: u64,
        archive_checksum: Option<String>,
    }

    let archive_checksum = archive_path
        .map(|path| hash::file_hash_blake3(path, None))
        .transpose()?;
    let material = ArtifactChecksumMaterial {
        format: ARTIFACT_CHECKSUM_FORMAT,
        roots: &manifest.roots,
        output: &manifest.output,
        restored_bytes: manifest.restored_bytes,
        execution_duration_ns: manifest.execution_duration_ns,
        archive_checksum,
    };
    let encoded = serde_json::to_string(&material)?;
    Ok(format!("blake3:{}", hash::hash_blake3_to_str(&encoded)))
}

fn verify_artifact_checksum(manifest: &CacheManifest, archive_path: Option<&Path>) -> Result<()> {
    let Some(expected) = &manifest.artifact_checksum else {
        return Ok(());
    };
    let archive_path = if manifest.roots.is_empty() {
        None
    } else {
        Some(archive_path.ok_or_else(|| eyre!("task cache archive is missing"))?)
    };
    let actual = calculate_artifact_checksum(manifest, archive_path)?;
    if actual != *expected {
        bail!("task cache artifact checksum mismatch");
    }
    Ok(())
}

pub(crate) fn task_cache_entries(task: &Task, root: &Path) -> Result<Vec<TaskCacheEntry>> {
    Settings::get().ensure_experimental("task artifact caching")?;
    let cache_dir = task_cache_dir();
    if !cache_dir.is_dir() {
        return Ok(Vec::new());
    }
    let identity = task_cache_identity(task, root);
    let current_key = file::read_to_string(task_cache_state_path(task, root))
        .ok()
        .map(|key| key.trim().to_string());
    let mut entries = Vec::new();
    for entry in fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let manifest_path = entry.path();
        if manifest_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if manifest_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.contains(".part-"))
        {
            continue;
        }
        let Some(key) = manifest_path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let _entry_lock = task_cache_entry_lock(&cache_dir, key)?;
        let manifest: CacheManifest = match fs::read(&manifest_path)
            .map_err(Report::from)
            .and_then(|contents| serde_json::from_slice(&contents).map_err(Report::from))
        {
            Ok(manifest) => manifest,
            Err(err) => {
                warn!(
                    "ignoring unreadable task cache manifest {}: {err}",
                    manifest_path.display()
                );
                continue;
            }
        };
        let matches_identity = manifest.task_identity == identity;
        let matches_legacy_current = manifest.task_identity.is_empty()
            && current_key.as_deref() == Some(manifest.key.as_str());
        if !matches_identity && !matches_legacy_current {
            continue;
        }
        let archive_path = cache_dir.join(format!("{}.tar.zst", manifest.key));
        let Ok(manifest_metadata) = fs::metadata(&manifest_path) else {
            continue;
        };
        let archive_metadata = fs::metadata(&archive_path).ok();
        let last_accessed = archive_metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .into_iter()
            .chain(manifest_metadata.modified().ok())
            .filter_map(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .max()
            .unwrap_or_default();
        entries.push(TaskCacheEntry {
            current: current_key.as_deref() == Some(manifest.key.as_str()),
            key: manifest.key,
            identity_verified: matches_identity,
            artifact_checksum: manifest.artifact_checksum,
            size_bytes: manifest_metadata
                .len()
                .saturating_add(archive_metadata.map_or(0, |metadata| metadata.len())),
            restored_bytes: manifest.restored_bytes,
            execution_duration_ns: manifest.execution_duration_ns,
            last_accessed,
            outputs: manifest.roots,
        });
    }
    entries.sort_by(|a, b| {
        b.last_accessed
            .cmp(&a.last_accessed)
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok(entries)
}

pub(crate) fn clear_task_cache(task: &Task, root: &Path) -> Result<TaskCacheClearResult> {
    let entries = task_cache_entries(task, root)?;
    let cache_dir = task_cache_dir();
    let identified_entries = entries.iter().filter(|entry| entry.identity_verified);
    let mut size_bytes = identified_entries
        .clone()
        .fold(0_u64, |total, entry| total.saturating_add(entry.size_bytes));
    for entry in identified_entries.clone() {
        let _entry_lock = task_cache_entry_lock(&cache_dir, &entry.key)?;
        remove_cache_file(&cache_dir.join(format!("{}.tar.zst", entry.key)))?;
        remove_cache_file(&cache_dir.join(format!("{}.json", entry.key)))?;
    }
    let legacy_entries = entries
        .len()
        .saturating_sub(identified_entries.clone().count());
    if legacy_entries > 0 {
        warn!(
            "skipping {legacy_entries} legacy task cache entries because ownership cannot be verified; use `mise cache clear` to remove them"
        );
    }
    let identity = task_cache_identity(task, root);
    let mut entry_count = identified_entries.count();
    let partial_entries = match fs::read_dir(&cache_dir) {
        Ok(entries) => Some(entries),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    for entry in partial_entries.into_iter().flatten() {
        let entry = entry?;
        let manifest_path = entry.path();
        let Some(stem) = manifest_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| stem.contains(".part-"))
        else {
            continue;
        };
        if manifest_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(key) = stem.split_once(".part-").map(|(key, _)| key) else {
            continue;
        };
        let _entry_lock = task_cache_entry_lock(&cache_dir, key)?;
        let Ok(contents) = fs::read(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<CacheManifest>(&contents) else {
            continue;
        };
        if manifest.task_identity != identity {
            continue;
        }
        let archive_path = cache_dir.join(format!("{stem}.tar.zst"));
        size_bytes = size_bytes
            .saturating_add(fs::metadata(&manifest_path).map_or(0, |metadata| metadata.len()))
            .saturating_add(fs::metadata(&archive_path).map_or(0, |metadata| metadata.len()));
        entry_count += 1;
        remove_cache_file(&manifest_path)?;
        remove_cache_file(&archive_path)?;
    }
    remove_cache_file(&task_cache_state_path(task, root))?;
    Ok(TaskCacheClearResult {
        entries: entry_count,
        size_bytes,
    })
}

fn remove_cache_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn task_cache_entry_lock(cache_dir: &Path, key: &str) -> Result<fslock::LockFile> {
    crate::lock_file::LockFile::new(&cache_dir.join(format!("{key}.json")))
        .with_callback(|path| debug!("waiting for task cache entry lock {}", path.display()))
        .lock()
}

fn task_cache_identity(task: &Task, root: &Path) -> String {
    hash::hash_blake3_to_str(&format!(
        "{}\0{}\0{:?}\0{}",
        root.display(),
        task.name,
        task.run_phase,
        task.config_source.display()
    ))
}

fn task_cache_state_path(task: &Task, root: &Path) -> PathBuf {
    dirs::STATE
        .join("task-artifacts")
        .join(format!("{}.key", task_cache_identity(task, root)))
}

impl TaskCacheKeyExplanation {
    pub(crate) fn to_json(&self, task: &str, cache_key: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Output<'a> {
            task: &'a str,
            cache_key: &'a str,
            #[serde(flatten)]
            explanation: &'a TaskCacheKeyExplanation,
        }

        Ok(serde_json::to_string(&Output {
            task,
            cache_key,
            explanation: self,
        })?)
    }

    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            "cache key inputs:".to_string(),
            format!("  cache format: {}", self.format),
            format!("  action version: {}", self.action_version),
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
            for expanded in expand_enumeration_patterns(&output)? {
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
) -> Result<u64> {
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

    let mut restored_bytes = 0_u64;
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
            restored_bytes = restored_bytes.saturating_add(metadata.len());
        } else {
            bail!("unsupported output file type: {}", rel.display());
        }
    }
    let encoder = archive.into_inner()?;
    encoder.finish()?;
    Ok(restored_bytes)
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
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn config_deserializes_and_rejects_unknown_fields() {
        let config: TaskCacheConfig = toml::from_str(
            "enabled = true\naudit = true\nenv = ['PROFILE']\ncommand_inputs = ['node --version']",
        )
        .unwrap();
        assert!(config.enabled);
        assert!(config.audit);
        assert_eq!(config.env, ["PROFILE"]);
        assert_eq!(config.command_inputs, ["node --version"]);
        assert!(toml::from_str::<TaskCacheConfig>("remote = true").is_err());
    }

    #[test]
    fn entry_locks_serialize_same_key_without_blocking_other_keys() {
        let cache_dir = tempfile::tempdir().unwrap();
        let held = task_cache_entry_lock(cache_dir.path(), "shared").unwrap();
        let _other = task_cache_entry_lock(cache_dir.path(), "independent").unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let cache_path = cache_dir.path().to_path_buf();

        let waiter = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _lock = task_cache_entry_lock(&cache_path, "shared").unwrap();
            acquired_tx.send(()).unwrap();
        });

        started_rx.recv().unwrap();
        assert!(acquired_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(held);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn abandoned_partial_writes_are_removed_without_touching_complete_entries() {
        let cache_dir = tempfile::tempdir().unwrap();
        let abandoned_manifest = cache_dir.path().join("abandoned.part-deadbeef.json");
        let abandoned_archive = cache_dir.path().join("abandoned.part-deadbeef.tar.zst");
        let complete_manifest = cache_dir.path().join("complete.json");
        let unrelated = cache_dir.path().join("abandoned.part-deadbeef.txt");
        fs::write(&abandoned_manifest, "partial manifest").unwrap();
        fs::write(&abandoned_archive, "partial archive").unwrap();
        fs::write(&complete_manifest, "complete manifest").unwrap();
        fs::write(&unrelated, "unrelated").unwrap();

        cleanup_abandoned_partial_writes(cache_dir.path()).unwrap();

        assert!(!abandoned_manifest.exists());
        assert!(!abandoned_archive.exists());
        assert!(complete_manifest.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn active_partial_writes_are_not_removed_while_locked() {
        let cache_dir = tempfile::tempdir().unwrap();
        let partial_manifest = cache_dir.path().join("active.part-deadbeef.json");
        let partial_archive = cache_dir.path().join("active.part-deadbeef.tar.zst");
        fs::write(&partial_manifest, "partial manifest").unwrap();
        fs::write(&partial_archive, "partial archive").unwrap();
        let held = task_cache_entry_lock(cache_dir.path(), "active").unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let cache_path = cache_dir.path().to_path_buf();

        let cleanup = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            cleanup_abandoned_partial_writes(&cache_path).unwrap();
            finished_tx.send(()).unwrap();
        });

        started_rx.recv().unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(partial_manifest.exists());
        assert!(partial_archive.exists());
        drop(held);
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        cleanup.join().unwrap();
        assert!(!partial_manifest.exists());
        assert!(!partial_archive.exists());
    }

    #[test]
    fn missing_cache_directory_remains_eligible_for_partial_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let cache_dir = root.path().join("later-cache");
        cleanup_abandoned_partial_writes_once(&cache_dir);

        fs::create_dir(&cache_dir).unwrap();
        let abandoned = cache_dir.join("abandoned.part-deadbeef.json");
        fs::write(&abandoned, "partial manifest").unwrap();
        cleanup_abandoned_partial_writes_once(&cache_dir);

        assert!(!abandoned.exists());
    }

    #[test]
    fn cache_limits_remove_oldest_entries_by_size_and_age() {
        let cache_dir = tempfile::tempdir().unwrap();
        let write_entry = |key: &str, modified: filetime::FileTime| {
            let manifest = cache_dir.path().join(format!("{key}.json"));
            let archive = cache_dir.path().join(format!("{key}.tar.zst"));
            fs::write(&manifest, [0_u8; 10]).unwrap();
            fs::write(&archive, [0_u8; 5]).unwrap();
            filetime::set_file_mtime(&manifest, modified).unwrap();
            filetime::set_file_mtime(&archive, modified).unwrap();
        };
        write_entry("oldest", filetime::FileTime::from_unix_time(100, 0));
        write_entry("middle", filetime::FileTime::from_unix_time(200, 0));
        write_entry("newest", filetime::FileTime::now());

        enforce_task_cache_limits(
            cache_dir.path(),
            TaskCacheLimits {
                max_size: Some(30),
                max_age: None,
            },
        )
        .unwrap();

        assert!(!cache_dir.path().join("oldest.json").exists());
        assert!(cache_dir.path().join("middle.json").exists());
        assert!(cache_dir.path().join("newest.json").exists());

        enforce_task_cache_limits(
            cache_dir.path(),
            TaskCacheLimits {
                max_size: None,
                max_age: Some(Duration::from_secs(24 * 60 * 60)),
            },
        )
        .unwrap();

        assert!(!cache_dir.path().join("middle.json").exists());
        assert!(cache_dir.path().join("newest.json").exists());
    }

    #[test]
    fn older_manifests_default_stats_metadata() {
        let manifest: CacheManifest = serde_json::from_value(serde_json::json!({
            "format": 2,
            "key": "cache-key",
            "roots": [],
            "output": [],
        }))
        .unwrap();

        assert_eq!(manifest.restored_bytes, 0);
        assert_eq!(manifest.execution_duration_ns, 0);
        assert_eq!(manifest.task_identity, "");
        assert_eq!(manifest.artifact_checksum, None);
    }

    #[test]
    fn artifact_checksum_is_independent_of_cache_key_and_task_identity() {
        let mut manifest = CacheManifest {
            format: CACHE_FORMAT_VERSION,
            key: "first-key".into(),
            task_identity: "first-task".into(),
            artifact_checksum: None,
            roots: Vec::new(),
            output: vec![TaskCacheOutput::Stdout("result\n".into())],
            restored_bytes: 7,
            execution_duration_ns: 42,
        };
        let first = calculate_artifact_checksum(&manifest, None).unwrap();
        manifest.key = "second-key".into();
        manifest.task_identity = "second-task".into();
        let second = calculate_artifact_checksum(&manifest, None).unwrap();

        assert_eq!(first, second);
        assert!(first.starts_with("blake3:"));

        manifest.artifact_checksum = Some(second);
        let stale_archive = tempfile::NamedTempFile::new().unwrap();
        fs::write(stale_archive.path(), "stale archive").unwrap();
        verify_artifact_checksum(&manifest, Some(stale_archive.path())).unwrap();
    }

    #[test]
    fn cache_explanation_omits_environment_and_variable_values() {
        let explanation = TaskCacheKeyExplanation {
            format: CACHE_FORMAT_VERSION,
            action_version: 1,
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
        // `display_cache_path` settles separators for display and then `escape_debug`s the
        // result, so on Windows the separator arrives doubled alongside the escaped control
        // characters this assertion is really about.
        #[cfg(windows)]
        assert!(
            output.contains(r"source: src\\\u{1b}[2J\nfile.rs"),
            "{output}"
        );
        #[cfg(not(windows))]
        assert!(
            output.contains(r"source: src/\u{1b}[2J\nfile.rs"),
            "{output}"
        );
        assert!(output.contains("pattern: !dist/private/**"));
        assert!(output.contains("output: dist"));
        assert!(output.contains("dependencies: 1 artifact keys"));
        assert!(output.contains("command inputs: 2"));
        assert!(output.contains("tools: 4 resolved versions"));
        assert!(!output.contains("secret"));
        assert!(!output.contains("hunter2"));

        let json = explanation.to_json("build", "cache-key").unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["task"], "build");
        assert_eq!(value["cache_key"], "cache-key");
        assert_eq!(value["source_paths"][0], "input.txt");
        assert_eq!(value["environment"]["TOKEN"], true);
        assert_eq!(value["environment"]["MISSING"], false);
        assert_eq!(value["command_input_count"], 2);
        assert!(!json.contains("secret"));
        assert!(!json.contains("hunter2"));
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
        assert_eq!(
            TaskCacheMissReason::Expired.to_string(),
            "cache entry exceeded its age limit"
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
