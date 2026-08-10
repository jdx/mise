use crate::{
    BlobSource, BlobUpload, CacheDigest, CacheDirectory, LocalActionCache, LocalCas,
    ManifestPutOutcome, RemoteActionResult, RemoteCacheClient, RemoteCacheMode, RustcMetadata,
    canonical_json,
};
use eyre::{Result, bail};
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_EXECUTABLE_IDENTITIES: usize = 64;
const MAX_EXECUTABLE_IDENTITY_SIZE: usize = 64 * 1024;
const MAX_EXECUTABLE_IDENTITY_BYTES: usize = 256 * 1024;
const TASK_ACTION_MANIFEST_VERSION: u8 = 1;
const MAX_TASK_ACTION_PREDICTIONS: usize = 16 * 1024;
const MAX_ACTION_PREDICTION_PAYLOAD: usize = 256 * 1024;
const MAX_REMOTE_TRANSFERS: usize = 8;
const MAX_PREFETCH_CONCURRENCY: usize = 4;
const MAX_PREFETCH_DIRECTORY_OBJECTS: usize = 100_000;

/// Remote action-cache access owned by one task session.
pub struct AgentRemoteCache {
    pub client: RemoteCacheClient,
    pub mode: RemoteCacheMode,
    pub staging_dir: PathBuf,
}

/// Wire protocol version used between an in-process cache agent and its shims.
pub const AGENT_PROTOCOL_VERSION: u8 = 1;

/// A request accepted by the task-scoped cache agent.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentRequest {
    Hello {
        protocol: u8,
        client_version: String,
    },
    FindBlob {
        digest: CacheDigest,
    },
    StoreBlob {
        digest: CacheDigest,
        source: PathBuf,
    },
    FindActionResult {
        action: CacheDigest,
    },
    RecordActionHit {
        action: CacheDigest,
    },
    StoreActionResult {
        result: RemoteActionResult,
    },
    FindActionPrediction {
        task: String,
        invocation: CacheDigest,
    },
    RecordActionPrediction {
        task: String,
        prediction: ActionPrediction,
    },
    FindExecutableIdentity {
        executable: PathBuf,
        environment: BTreeMap<String, Option<String>>,
    },
    StoreExecutableIdentity {
        executable: PathBuf,
        environment: BTreeMap<String, Option<String>>,
        stdout: Vec<u8>,
    },
}

/// A response returned by the task-scoped cache agent.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentResponse {
    Hello {
        protocol: u8,
        agent_version: String,
    },
    Blob {
        path: Option<PathBuf>,
    },
    Stored {
        path: PathBuf,
    },
    ActionResult {
        result: Option<RemoteActionResult>,
    },
    ActionHitRecorded,
    ActionStored {
        path: PathBuf,
    },
    ActionPrediction {
        prediction: Option<ActionPrediction>,
    },
    ActionPredictionRecorded,
    ExecutableIdentity {
        stdout: Option<Vec<u8>>,
    },
    Error {
        message: String,
    },
}

/// Aggregate cache activity for one task session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentStats {
    /// Number of action-result lookups.
    pub lookups: u64,
    /// Number of lookups that found a valid local action result.
    pub hits: u64,
    /// Number of newly stored content-addressed objects.
    pub stores: u64,
    /// Total size of newly stored objects.
    pub stored_bytes: u64,
}

/// Adapter-owned data needed to reconstruct an action before fresh dependency
/// discovery is available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPrediction {
    pub invocation: CacheDigest,
    pub action: CacheDigest,
    pub adapter: String,
    pub payload: String,
}

#[derive(Default)]
struct AtomicAgentStats {
    lookups: AtomicU64,
    hits: AtomicU64,
    stores: AtomicU64,
    stored_bytes: AtomicU64,
}

/// Shared state for an agent hosted by the top-level `mise run` process.
///
/// Transport listeners deliberately live in mise so the task-run lifecycle owns
/// them. This type only contains ecosystem-independent CAS and protocol logic.
#[derive(Clone)]
pub struct CacheAgent {
    cas: LocalCas,
    actions: LocalActionCache,
    version: Arc<str>,
    write_locks: Arc<Mutex<BTreeMap<CacheDigest, Weak<tokio::sync::Mutex<()>>>>>,
    action_locks: Arc<Mutex<BTreeMap<CacheDigest, Weak<tokio::sync::Mutex<()>>>>>,
    stats: Arc<AtomicAgentStats>,
    executable_identities: Arc<Mutex<BTreeMap<ExecutableIdentityKey, Vec<u8>>>>,
    manifest_dir: Arc<PathBuf>,
    task_actions: Arc<Mutex<BTreeMap<String, TaskActionState>>>,
    next_task_run: Arc<AtomicU64>,
    manifest_write_lock: Arc<Mutex<()>>,
    remote: Option<Arc<RemoteCacheClient>>,
    remote_mode: RemoteCacheMode,
    remote_staging_dir: Arc<PathBuf>,
    pending_remote_actions: Arc<Mutex<BTreeMap<CacheDigest, RemoteActionResult>>>,
    remote_transfers: Arc<tokio::sync::Semaphore>,
    prefetch_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskActionManifest {
    version: u8,
    task: String,
    predictions: Vec<ActionPrediction>,
}

#[derive(Serialize)]
struct TaskActionManifestSelector<'a> {
    version: u8,
    kind: &'static str,
    task: &'a str,
}

#[derive(Debug, Clone, Default)]
struct TaskActionState {
    manifest: String,
    baseline_loaded: bool,
    predictions: BTreeMap<CacheDigest, ActionPrediction>,
    remote_etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExecutableIdentityKey {
    executable: PathBuf,
    environment: BTreeMap<String, Option<String>>,
}

impl CacheAgent {
    /// Create an agent backed by the cache rooted at `cache_dir`.
    pub fn new(cache_dir: impl Into<PathBuf>, version: impl Into<Arc<str>>) -> Self {
        Self::build(cache_dir.into(), version.into(), None)
    }

    /// Create an agent with local-first access to a remote action cache.
    pub fn new_remote(
        cache_dir: impl Into<PathBuf>,
        version: impl Into<Arc<str>>,
        remote: AgentRemoteCache,
    ) -> Self {
        Self::build(cache_dir.into(), version.into(), Some(remote))
    }

    fn build(cache_dir: PathBuf, version: Arc<str>, remote: Option<AgentRemoteCache>) -> Self {
        let remote_mode = remote
            .as_ref()
            .map_or(RemoteCacheMode::ReadOnly, |remote| remote.mode);
        let remote_staging_dir = remote.as_ref().map_or_else(
            || cache_dir.join("remote"),
            |remote| remote.staging_dir.clone(),
        );
        let remote = remote.map(|remote| Arc::new(remote.client));
        Self {
            cas: LocalCas::new(cache_dir.clone()),
            actions: LocalActionCache::new(cache_dir.clone()),
            version,
            write_locks: Arc::new(Mutex::new(BTreeMap::new())),
            action_locks: Arc::new(Mutex::new(BTreeMap::new())),
            stats: Arc::new(AtomicAgentStats::default()),
            executable_identities: Arc::new(Mutex::new(BTreeMap::new())),
            manifest_dir: Arc::new(cache_dir.join("task-manifests").join("v1")),
            task_actions: Arc::new(Mutex::new(BTreeMap::new())),
            next_task_run: Arc::new(AtomicU64::new(0)),
            manifest_write_lock: Arc::new(Mutex::new(())),
            remote,
            remote_mode,
            remote_staging_dir: Arc::new(remote_staging_dir),
            pending_remote_actions: Arc::new(Mutex::new(BTreeMap::new())),
            remote_transfers: Arc::new(tokio::sync::Semaphore::new(MAX_REMOTE_TRANSFERS)),
            prefetch_tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Load the last successful action manifest for a task into this session.
    pub async fn begin_task(&self, task: &str) -> Result<String> {
        validate_task_identity(task)?;
        let (remote_manifest, mut remote_etag) = if self.remote_mode.reads() {
            match self.get_remote_task_manifest(task).await {
                Ok(Some((manifest, etag))) => (Some(manifest), Some(etag)),
                Ok(None) => (None, None),
                Err(error) => {
                    warn!("remote task action manifest lookup failed for {task}: {error}");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };
        let manifest = {
            let _write_guard = self.manifest_write_lock.lock().unwrap();
            let _file_guard = self.lock_task_manifest(task)?;
            let local_manifest = self.load_task_manifest(task)?;
            let manifest = match (remote_manifest, local_manifest) {
                (Some(remote), Some(local)) => {
                    let (manifest, merged) = merge_remote_task_manifest(task, remote, local);
                    if !merged {
                        remote_etag = None;
                    }
                    Some(manifest)
                }
                (Some(remote), None) => Some(remote),
                (None, local) => local,
            };
            if let Some(manifest) = &manifest {
                self.persist_task_manifest(manifest)?;
            }
            manifest
        };
        let state = if let Some(manifest) = manifest {
            TaskActionState {
                manifest: task.to_string(),
                baseline_loaded: true,
                predictions: manifest
                    .predictions
                    .into_iter()
                    .map(|prediction| (prediction.invocation.clone(), prediction))
                    .collect(),
                remote_etag,
            }
        } else {
            TaskActionState {
                manifest: task.to_string(),
                baseline_loaded: true,
                remote_etag,
                ..TaskActionState::default()
            }
        };
        let sequence = self.next_task_run.fetch_add(1, Ordering::Relaxed);
        let run =
            CacheDigest::blake3(format!("{task}\0{}\0{sequence}", std::process::id()).as_bytes())
                .hash;
        let predictions = state.predictions.values().cloned().collect();
        self.task_actions.lock().unwrap().insert(run.clone(), state);
        self.spawn_prefetch_predictions(predictions);
        Ok(run)
    }

    /// Cancel speculative downloads before the owning session exits.
    pub async fn cancel_prefetches(&self) {
        let tasks = std::mem::take(&mut *self.prefetch_tasks.lock().unwrap());
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            if let Err(error) = task.await
                && !error.is_cancelled()
            {
                warn!("remote action prefetch task failed: {error}");
            }
        }
    }

    #[cfg(test)]
    async fn wait_for_prefetches(&self) {
        let tasks = std::mem::take(&mut *self.prefetch_tasks.lock().unwrap());
        for task in tasks {
            if let Err(error) = task.await {
                warn!("remote action prefetch task failed: {error}");
            }
        }
    }

    /// Atomically publish the candidate manifest collected by a successful task.
    pub async fn commit_task(&self, run: &str) -> Result<()> {
        validate_task_identity(run)?;
        let state = self
            .task_actions
            .lock()
            .unwrap()
            .get(run)
            .cloned()
            .ok_or_else(|| eyre::eyre!("task action manifest baseline was not loaded"))?;
        if !state.baseline_loaded {
            bail!("task action manifest baseline was not loaded");
        }
        let task = state.manifest;
        validate_task_identity(&task)?;
        let manifest = {
            let _write_guard = self.manifest_write_lock.lock().unwrap();
            let _file_guard = self.lock_task_manifest(&task)?;
            let mut predictions = self
                .load_task_manifest(&task)?
                .map(|manifest| {
                    manifest
                        .predictions
                        .into_iter()
                        .map(|prediction| (prediction.invocation.clone(), prediction))
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            predictions.extend(state.predictions);
            let manifest = TaskActionManifest {
                version: TASK_ACTION_MANIFEST_VERSION,
                task: task.clone(),
                predictions: predictions.into_values().collect(),
            };
            validate_task_manifest(&manifest, &task)?;
            self.persist_task_manifest(&manifest)?;
            manifest
        };
        self.task_actions.lock().unwrap().remove(run);
        if self.remote_mode.writes() {
            match self
                .put_remote_task_manifest(&task, manifest, state.remote_etag)
                .await
            {
                Ok(remote_manifest) => {
                    let _write_guard = self.manifest_write_lock.lock().unwrap();
                    let reconciliation = (|| {
                        let _file_guard = self.lock_task_manifest(&task)?;
                        let manifest = match self.load_task_manifest(&task)? {
                            Some(local) => {
                                merge_remote_task_manifest(&task, remote_manifest, local).0
                            }
                            None => remote_manifest,
                        };
                        self.persist_task_manifest(&manifest)
                    })();
                    if let Err(error) = reconciliation {
                        warn!(
                            "remote task action manifest reconciliation failed for {task}: {error}"
                        );
                    }
                }
                Err(error) => {
                    warn!("remote task action manifest upload failed for {task}: {error}");
                }
            }
        }
        Ok(())
    }

    fn task_manifest_path(&self, task: &str) -> PathBuf {
        self.manifest_dir.join(format!("{task}.json"))
    }

    fn task_manifest_lock_path(&self, task: &str) -> PathBuf {
        self.manifest_dir.join("locks").join(format!("{task}.lock"))
    }

    fn lock_task_manifest(&self, task: &str) -> Result<fslock::LockFile> {
        let path = self.task_manifest_lock_path(task);
        fs::create_dir_all(path.parent().expect("task manifest lock has a parent"))?;
        let mut lock = fslock::LockFile::open(&path)?;
        lock.lock()?;
        Ok(lock)
    }

    fn load_task_manifest(&self, task: &str) -> Result<Option<TaskActionManifest>> {
        match fs::read(self.task_manifest_path(task)) {
            Ok(contents) => Ok(Some(self.parse_task_manifest(task, &contents, false)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn parse_task_manifest(
        &self,
        task: &str,
        contents: &[u8],
        require_canonical: bool,
    ) -> Result<TaskActionManifest> {
        let manifest: TaskActionManifest = serde_json::from_slice(contents)?;
        validate_task_manifest(&manifest, task)?;
        if require_canonical && canonical_json(&manifest)? != contents {
            bail!("task action manifest is not canonical JSON");
        }
        Ok(manifest)
    }

    fn task_manifest_selector(task: &str) -> Result<(Vec<u8>, CacheDigest)> {
        let bytes = canonical_json(&TaskActionManifestSelector {
            version: 1,
            kind: "task_action_manifest",
            task,
        })?;
        let digest = CacheDigest::blake3(&bytes);
        Ok((bytes, digest))
    }

    fn persist_task_manifest(&self, manifest: &TaskActionManifest) -> Result<()> {
        let bytes = canonical_json(manifest)?;
        fs::create_dir_all(self.manifest_dir.as_path())?;
        let mut temporary = tempfile::NamedTempFile::new_in(self.manifest_dir.as_path())?;
        std::io::Write::write_all(temporary.as_file_mut(), &bytes)?;
        temporary.as_file_mut().sync_all()?;
        temporary
            .persist(self.task_manifest_path(&manifest.task))
            .map_err(|error| error.error)?;
        Ok(())
    }

    async fn get_remote_task_manifest(
        &self,
        task: &str,
    ) -> Result<Option<(TaskActionManifest, String)>> {
        let Some(remote) = &self.remote else {
            return Ok(None);
        };
        let (_, selector) = Self::task_manifest_selector(task)?;
        let _permit = self.remote_transfers.acquire().await?;
        let Some(remote_manifest) = remote.get_action_manifest(&selector).await? else {
            return Ok(None);
        };
        let manifest = self.parse_task_manifest(task, &remote_manifest.bytes, true)?;
        Ok(Some((manifest, remote_manifest.etag)))
    }

    async fn put_remote_task_manifest(
        &self,
        task: &str,
        mut manifest: TaskActionManifest,
        mut expected_etag: Option<String>,
    ) -> Result<TaskActionManifest> {
        let Some(remote) = &self.remote else {
            return Ok(manifest);
        };
        let (_, selector) = Self::task_manifest_selector(task)?;
        for _ in 0..4 {
            let bytes = canonical_json(&manifest)?;
            let outcome = {
                let _permit = self.remote_transfers.acquire().await?;
                remote
                    .put_action_manifest(&selector, &bytes, expected_etag.as_deref())
                    .await?
            };
            match outcome {
                ManifestPutOutcome::Stored => return Ok(manifest),
                ManifestPutOutcome::PreconditionFailed => {
                    let Some((remote_manifest, etag)) = self.get_remote_task_manifest(task).await?
                    else {
                        expected_etag = None;
                        continue;
                    };
                    manifest = merge_task_manifests(task, Some(remote_manifest), manifest)?;
                    expected_etag = Some(etag);
                }
            }
        }
        bail!("remote task action manifest changed too frequently")
    }

    /// Return a snapshot of this session's cache activity.
    pub fn stats(&self) -> AgentStats {
        AgentStats {
            lookups: self.stats.lookups.load(Ordering::Relaxed),
            hits: self.stats.hits.load(Ordering::Relaxed),
            stores: self.stats.stores.load(Ordering::Relaxed),
            stored_bytes: self.stats.stored_bytes.load(Ordering::Relaxed),
        }
    }

    fn write_lock(&self, digest: &CacheDigest) -> Arc<tokio::sync::Mutex<()>> {
        Self::digest_lock(&self.write_locks, digest)
    }

    fn action_lock(&self, digest: &CacheDigest) -> Arc<tokio::sync::Mutex<()>> {
        Self::digest_lock(&self.action_locks, digest)
    }

    fn digest_lock(
        locks: &Mutex<BTreeMap<CacheDigest, Weak<tokio::sync::Mutex<()>>>>,
        digest: &CacheDigest,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = locks.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(digest).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(digest.clone(), Arc::downgrade(&lock));
        lock
    }

    fn spawn_prefetch_predictions(&self, predictions: Vec<ActionPrediction>) {
        if predictions.is_empty() || !self.remote_mode.reads() || self.remote.is_none() {
            return;
        }
        let agent = self.clone();
        let task = tokio::spawn(async move {
            agent.prefetch_predictions(predictions.iter()).await;
        });
        self.prefetch_tasks.lock().unwrap().push(task);
    }

    async fn prefetch_predictions<'a>(
        &self,
        predictions: impl Iterator<Item = &'a ActionPrediction>,
    ) {
        if !self.remote_mode.reads() || self.remote.is_none() {
            return;
        }
        let mut actions = BTreeMap::new();
        for prediction in predictions {
            actions
                .entry(prediction.action.clone())
                .or_insert_with(|| prediction.adapter.clone());
        }
        let mut actions = actions.into_iter();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..MAX_PREFETCH_CONCURRENCY {
            let Some((action, adapter)) = actions.next() else {
                break;
            };
            let agent = self.clone();
            tasks.spawn(async move { agent.prefetch_action(action, adapter).await });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => warn!("remote action prefetch failed: {error}"),
                Err(error) => warn!("remote action prefetch task failed: {error}"),
            }
            if let Some((action, adapter)) = actions.next() {
                let agent = self.clone();
                tasks.spawn(async move { agent.prefetch_action(action, adapter).await });
            }
        }
    }

    async fn prefetch_action(&self, action: CacheDigest, adapter: String) -> Result<()> {
        let lock = self.action_lock(&action);
        let _guard = lock.lock().await;
        if self.actions.find(&action)?.is_some() {
            return Ok(());
        }
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| eyre::eyre!("remote cache is not configured"))?;
        let pending = {
            self.pending_remote_actions
                .lock()
                .unwrap()
                .get(&action)
                .cloned()
        };
        let result = match pending {
            Some(result) => result,
            None => {
                let result = {
                    let _permit = self.remote_transfers.acquire().await?;
                    remote.get_action_result(&action).await?
                };
                let Some(result) = result else { return Ok(()) };
                result
            }
        };
        self.fetch_remote_blob(remote, &result.action).await?;
        if let Some(metadata) = &result.metadata {
            let path = self.fetch_remote_blob(remote, metadata).await?;
            if adapter == "rustc" {
                let bytes = fs::read(path)?;
                let metadata: RustcMetadata = serde_json::from_slice(&bytes)?;
                if metadata.version != 1
                    || metadata.kind != "rustc"
                    || canonical_json(&metadata)? != bytes
                {
                    bail!("remote rustc action metadata is invalid");
                }
                self.fetch_remote_blob(remote, &metadata.stdout).await?;
                self.fetch_remote_blob(remote, &metadata.stderr).await?;
            }
        }
        if let Some(output_root) = &result.output_root {
            self.prefetch_output_tree(remote, output_root).await?;
        }
        self.actions.store(&result)?;
        self.pending_remote_actions.lock().unwrap().remove(&action);
        Ok(())
    }

    async fn prefetch_output_tree(
        &self,
        remote: &RemoteCacheClient,
        output_root: &CacheDigest,
    ) -> Result<()> {
        let mut pending = vec![output_root.clone()];
        let mut seen = BTreeMap::new();
        while let Some(digest) = pending.pop() {
            if seen.insert(digest.clone(), ()).is_some() {
                continue;
            }
            if seen.len() > MAX_PREFETCH_DIRECTORY_OBJECTS {
                bail!("remote action output tree is too large");
            }
            let path = self.fetch_remote_blob(remote, &digest).await?;
            let bytes = fs::read(path)?;
            let directory: CacheDirectory = serde_json::from_slice(&bytes)?;
            if directory.version != 1 || canonical_json(&directory)? != bytes {
                bail!("remote action output directory is invalid");
            }
            for file in directory.files {
                self.fetch_remote_blob(remote, &file.digest).await?;
            }
            pending.extend(
                directory
                    .directories
                    .into_iter()
                    .map(|directory| directory.digest),
            );
        }
        Ok(())
    }

    async fn fetch_remote_blob(
        &self,
        remote: &RemoteCacheClient,
        digest: &CacheDigest,
    ) -> Result<PathBuf> {
        let lock = self.write_lock(digest);
        let _guard = lock.lock().await;
        if let Some(path) = self.cas.find(digest)? {
            return Ok(path);
        }
        let _permit = self.remote_transfers.acquire().await?;
        let temporary = remote
            .get_blob_file(digest, self.remote_staging_dir.as_path())
            .await?;
        let path = self.cas.store_file(digest, temporary.path())?;
        self.stats.stores.fetch_add(1, Ordering::Relaxed);
        self.stats
            .stored_bytes
            .fetch_add(digest.size, Ordering::Relaxed);
        Ok(path)
    }

    async fn respond(&self, request: AgentRequest) -> AgentResponse {
        let result = match request {
            AgentRequest::FindBlob { digest } => self.find_blob(&digest).await,
            AgentRequest::StoreBlob { digest, source } => self.store_blob(&digest, &source).await,
            AgentRequest::FindActionResult { action } => {
                self.stats.lookups.fetch_add(1, Ordering::Relaxed);
                self.find_action_result(&action).await
            }
            AgentRequest::RecordActionHit { action } => self.record_action_hit(&action),
            AgentRequest::StoreActionResult { result } => self.store_action_result(&result).await,
            AgentRequest::FindActionPrediction { task, invocation } => {
                self.find_action_prediction(&task, &invocation)
            }
            AgentRequest::RecordActionPrediction { task, prediction } => {
                self.record_action_prediction(&task, prediction)
            }
            AgentRequest::FindExecutableIdentity {
                executable,
                environment,
            } => self.find_executable_identity(executable, environment),
            AgentRequest::StoreExecutableIdentity {
                executable,
                environment,
                stdout,
            } => self.store_executable_identity(executable, environment, stdout),
            AgentRequest::Hello { .. } => {
                Err(eyre::eyre!("hello is only valid as the first request"))
            }
        };
        result.unwrap_or_else(|error| AgentResponse::Error {
            message: error.to_string(),
        })
    }

    async fn find_blob(&self, digest: &CacheDigest) -> Result<AgentResponse> {
        if let Some(path) = self.cas.find(digest)? {
            return Ok(AgentResponse::Blob { path: Some(path) });
        }
        if !self.remote_mode.reads() {
            return Ok(AgentResponse::Blob { path: None });
        }
        let Some(remote) = &self.remote else {
            return Ok(AgentResponse::Blob { path: None });
        };
        match self.fetch_remote_blob(remote, digest).await {
            Ok(path) => Ok(AgentResponse::Blob { path: Some(path) }),
            Err(error) => {
                warn!(
                    "remote cache blob lookup failed for {}: {error}",
                    digest.hash
                );
                Ok(AgentResponse::Blob { path: None })
            }
        }
    }

    async fn store_blob(&self, digest: &CacheDigest, source: &Path) -> Result<AgentResponse> {
        let remote = if self.remote_mode.writes() {
            self.remote.as_deref()
        } else {
            None
        };
        let path = {
            let lock = self.write_lock(digest);
            let _guard = lock.lock().await;
            if let Some(path) = self.cas.find(digest)? {
                path
            } else {
                let path = self.cas.store_file(digest, source)?;
                self.stats.stores.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .stored_bytes
                    .fetch_add(digest.size, Ordering::Relaxed);
                path
            }
        };
        if let Some(remote) = remote {
            let _permit = self.remote_transfers.acquire().await?;
            if let Err(error) = remote
                .put_blob(&BlobUpload {
                    digest: digest.clone(),
                    source: BlobSource::Path(path.clone()),
                })
                .await
            {
                warn!(
                    "remote cache blob upload failed for {}: {error}",
                    digest.hash
                );
            }
        }
        Ok(AgentResponse::Stored { path })
    }

    async fn find_action_result(&self, action: &CacheDigest) -> Result<AgentResponse> {
        if let Some(result) = self.actions.find(action)? {
            return Ok(AgentResponse::ActionResult {
                result: Some(result),
            });
        }
        if !self.remote_mode.reads() {
            return Ok(AgentResponse::ActionResult { result: None });
        }
        let Some(remote) = &self.remote else {
            return Ok(AgentResponse::ActionResult { result: None });
        };
        let lock = self.action_lock(action);
        let _guard = lock.lock().await;
        if let Some(result) = self.actions.find(action)? {
            return Ok(AgentResponse::ActionResult {
                result: Some(result),
            });
        }
        if let Some(result) = self
            .pending_remote_actions
            .lock()
            .unwrap()
            .get(action)
            .cloned()
        {
            return Ok(AgentResponse::ActionResult {
                result: Some(result),
            });
        }
        let _permit = self.remote_transfers.acquire().await?;
        match remote.get_action_result(action).await {
            Ok(Some(result)) => {
                self.pending_remote_actions
                    .lock()
                    .unwrap()
                    .insert(action.clone(), result.clone());
                Ok(AgentResponse::ActionResult {
                    result: Some(result),
                })
            }
            Ok(None) => Ok(AgentResponse::ActionResult { result: None }),
            Err(error) => {
                warn!(
                    "remote cache action lookup failed for {}: {error}",
                    action.hash
                );
                Ok(AgentResponse::ActionResult { result: None })
            }
        }
    }

    async fn store_action_result(&self, result: &RemoteActionResult) -> Result<AgentResponse> {
        let path = self.actions.store(result)?;
        if self.remote_mode.writes()
            && let Some(remote) = &self.remote
        {
            let _permit = self.remote_transfers.acquire().await?;
            if let Err(error) = remote.put_action_result(result).await {
                warn!(
                    "remote cache action upload failed for {}: {error}",
                    result.action.hash
                );
            }
        }
        Ok(AgentResponse::ActionStored { path })
    }

    fn record_action_hit(&self, action: &CacheDigest) -> Result<AgentResponse> {
        if self.actions.find(action)?.is_none() {
            let pending = self.pending_remote_actions.lock().unwrap().remove(action);
            if let Some(result) = pending {
                self.actions.store(&result)?;
            } else {
                bail!("cannot record a hit for a missing action result");
            }
        }
        self.stats.hits.fetch_add(1, Ordering::Relaxed);
        Ok(AgentResponse::ActionHitRecorded)
    }

    fn find_action_prediction(
        &self,
        task: &str,
        invocation: &CacheDigest,
    ) -> Result<AgentResponse> {
        validate_task_identity(task)?;
        invocation.validate()?;
        let prediction = self
            .task_actions
            .lock()
            .unwrap()
            .get(task)
            .and_then(|state| state.predictions.get(invocation))
            .cloned();
        Ok(AgentResponse::ActionPrediction { prediction })
    }

    fn record_action_prediction(
        &self,
        task: &str,
        prediction: ActionPrediction,
    ) -> Result<AgentResponse> {
        validate_task_identity(task)?;
        validate_action_prediction(&prediction)?;
        let mut tasks = self.task_actions.lock().unwrap();
        let state = tasks.entry(task.to_string()).or_default();
        if !state.predictions.contains_key(&prediction.invocation)
            && state.predictions.len() >= MAX_TASK_ACTION_PREDICTIONS
        {
            bail!("task action manifest contains too many predictions");
        }
        state
            .predictions
            .insert(prediction.invocation.clone(), prediction);
        Ok(AgentResponse::ActionPredictionRecorded)
    }

    fn executable_identity_key(
        &self,
        executable: PathBuf,
        environment: BTreeMap<String, Option<String>>,
    ) -> Result<ExecutableIdentityKey> {
        if !environment
            .keys()
            .all(|name| matches!(name.as_str(), "RUSTUP_HOME" | "RUSTUP_TOOLCHAIN"))
        {
            bail!("executable identity contains an unsupported environment variable");
        }
        Ok(ExecutableIdentityKey {
            executable,
            environment,
        })
    }

    fn find_executable_identity(
        &self,
        executable: PathBuf,
        environment: BTreeMap<String, Option<String>>,
    ) -> Result<AgentResponse> {
        let key = self.executable_identity_key(executable, environment)?;
        let stdout = self
            .executable_identities
            .lock()
            .unwrap()
            .get(&key)
            .cloned();
        Ok(AgentResponse::ExecutableIdentity { stdout })
    }

    fn store_executable_identity(
        &self,
        executable: PathBuf,
        environment: BTreeMap<String, Option<String>>,
        stdout: Vec<u8>,
    ) -> Result<AgentResponse> {
        if stdout.len() > MAX_EXECUTABLE_IDENTITY_SIZE {
            bail!("executable identity exceeds {MAX_EXECUTABLE_IDENTITY_SIZE} bytes");
        }
        let key = self.executable_identity_key(executable, environment)?;
        let mut identities = self.executable_identities.lock().unwrap();
        let is_new = !identities.contains_key(&key);
        let previous_size = identities.get(&key).map_or(0, Vec::len);
        if is_new && identities.len() >= MAX_EXECUTABLE_IDENTITIES {
            bail!("executable identity cache contains too many entries");
        }
        let retained_bytes = identities.values().map(Vec::len).sum::<usize>();
        if retained_bytes - previous_size + stdout.len() > MAX_EXECUTABLE_IDENTITY_BYTES {
            bail!("executable identity cache contains too many bytes");
        }
        identities.insert(key, stdout.clone());
        Ok(AgentResponse::ExecutableIdentity {
            stdout: Some(stdout),
        })
    }

    /// Serve newline-delimited protocol requests on an authenticated session stream.
    pub async fn handle_connection<S>(&self, stream: S) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        let hello = lines
            .next_line()
            .await?
            .ok_or_else(|| eyre::eyre!("connection closed before the agent handshake"))?;
        let request: AgentRequest = serde_json::from_str(&hello)?;
        match request {
            AgentRequest::Hello {
                protocol,
                client_version,
            } if protocol == AGENT_PROTOCOL_VERSION && client_version == self.version.as_ref() => {}
            AgentRequest::Hello { protocol, .. } if protocol != AGENT_PROTOCOL_VERSION => {
                send_response(
                    &mut writer,
                    &AgentResponse::Error {
                        message: format!(
                            "unsupported agent protocol {protocol}; expected {AGENT_PROTOCOL_VERSION}"
                        ),
                    },
                )
                .await?;
                return Ok(());
            }
            AgentRequest::Hello { client_version, .. } => {
                send_response(
                    &mut writer,
                    &AgentResponse::Error {
                        message: format!(
                            "cache client {client_version} does not match agent {}",
                            self.version
                        ),
                    },
                )
                .await?;
                return Ok(());
            }
            _ => bail!("the first agent request must be hello"),
        }
        send_response(
            &mut writer,
            &AgentResponse::Hello {
                protocol: AGENT_PROTOCOL_VERSION,
                agent_version: self.version.to_string(),
            },
        )
        .await?;

        while let Some(line) = lines.next_line().await? {
            let response = match serde_json::from_str(&line) {
                Ok(request) => self.respond(request).await,
                Err(error) => AgentResponse::Error {
                    message: format!("invalid agent request: {error}"),
                },
            };
            send_response(&mut writer, &response).await?;
        }
        Ok(())
    }
}

fn validate_task_identity(task: &str) -> Result<()> {
    if task.len() != 64
        || !task
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid task action identity");
    }
    Ok(())
}

fn validate_action_prediction(prediction: &ActionPrediction) -> Result<()> {
    prediction.invocation.validate()?;
    prediction.action.validate()?;
    if prediction.adapter.is_empty()
        || !prediction
            .adapter
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid action prediction adapter");
    }
    if prediction.payload.len() > MAX_ACTION_PREDICTION_PAYLOAD {
        bail!("action prediction payload is too large");
    }
    serde_json::from_str::<serde_json::Value>(&prediction.payload)?;
    Ok(())
}

fn validate_task_manifest(manifest: &TaskActionManifest, task: &str) -> Result<()> {
    if manifest.version != TASK_ACTION_MANIFEST_VERSION || manifest.task != task {
        bail!("task action manifest has an invalid identity");
    }
    if manifest.predictions.len() > MAX_TASK_ACTION_PREDICTIONS {
        bail!("task action manifest contains too many predictions");
    }
    let mut invocations = BTreeMap::new();
    for prediction in &manifest.predictions {
        validate_action_prediction(prediction)?;
        if invocations.insert(&prediction.invocation, ()).is_some() {
            bail!("task action manifest contains duplicate predictions");
        }
    }
    Ok(())
}

fn merge_task_manifests(
    task: &str,
    base: Option<TaskActionManifest>,
    update: TaskActionManifest,
) -> Result<TaskActionManifest> {
    validate_task_manifest(&update, task)?;
    let mut predictions = BTreeMap::new();
    if let Some(base) = base {
        validate_task_manifest(&base, task)?;
        predictions.extend(
            base.predictions
                .into_iter()
                .map(|prediction| (prediction.invocation.clone(), prediction)),
        );
    }
    predictions.extend(
        update
            .predictions
            .into_iter()
            .map(|prediction| (prediction.invocation.clone(), prediction)),
    );
    let manifest = TaskActionManifest {
        version: TASK_ACTION_MANIFEST_VERSION,
        task: task.to_owned(),
        predictions: predictions.into_values().collect(),
    };
    validate_task_manifest(&manifest, task)?;
    Ok(manifest)
}

fn merge_remote_task_manifest(
    task: &str,
    remote: TaskActionManifest,
    local: TaskActionManifest,
) -> (TaskActionManifest, bool) {
    match merge_task_manifests(task, Some(remote), local.clone()) {
        Ok(manifest) => (manifest, true),
        Err(error) => {
            warn!("remote task action manifest merge failed for {task}: {error}");
            (local, false)
        }
    }
}

async fn send_response(
    writer: &mut (impl AsyncWrite + Unpin),
    response: &AgentResponse,
) -> Result<()> {
    let mut encoded = serde_json::to_vec(response)?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ACTION_RESULT_MEDIA_TYPE;
    use std::time::Duration;

    async fn handshake(stream: &mut (impl AsyncRead + AsyncWrite + Unpin), version: &str) {
        let request = AgentRequest::Hello {
            protocol: AGENT_PROTOCOL_VERSION,
            client_version: version.to_string(),
        };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        stream.write_all(&encoded).await.unwrap();
        stream.flush().await.unwrap();
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .await
            .unwrap();
        assert!(matches!(
            serde_json::from_str(&response).unwrap(),
            AgentResponse::Hello { .. }
        ));
    }

    #[tokio::test]
    async fn handshake_and_blob_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        std::fs::write(&source, b"cached object").unwrap();
        let digest = CacheDigest::blake3(b"cached object");
        let agent = CacheAgent::new(directory.path().join("cache"), "test-version");
        let (mut client, server) = tokio::io::duplex(16 * 1024);
        let server_agent = agent.clone();
        let task = tokio::spawn(async move { server_agent.handle_connection(server).await });

        handshake(&mut client, "test-version").await;
        let request = AgentRequest::StoreBlob {
            digest: digest.clone(),
            source,
        };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .unwrap();
        assert!(matches!(
            serde_json::from_str(&response).unwrap(),
            AgentResponse::Stored { .. }
        ));
        drop(client);
        task.await.unwrap().unwrap();
        assert_eq!(
            agent.stats(),
            AgentStats {
                stores: 1,
                stored_bytes: digest.size,
                ..AgentStats::default()
            }
        );
    }

    #[tokio::test]
    async fn publishes_a_complete_action_result() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path().join("cache"), "test-version");
        let action = CacheDigest::blake3(b"action");
        let metadata = CacheDigest::blake3(b"metadata");
        let output_root = CacheDigest::blake3(b"directory");
        for (digest, contents) in [
            (&action, b"action".as_slice()),
            (&metadata, b"metadata".as_slice()),
            (&output_root, b"directory".as_slice()),
        ] {
            agent.cas.store_bytes(digest, contents).unwrap();
        }
        let response = agent
            .respond(AgentRequest::StoreActionResult {
                result: RemoteActionResult {
                    action: action.clone(),
                    metadata: Some(metadata),
                    output_root: Some(output_root),
                    version: 1,
                },
            })
            .await;
        assert!(matches!(response, AgentResponse::ActionStored { .. }));
        let response = agent
            .respond(AgentRequest::FindActionResult {
                action: action.clone(),
            })
            .await;
        assert!(matches!(
            response,
            AgentResponse::ActionResult {
                result: Some(result)
            } if result.action == action
        ));
        assert!(matches!(
            agent
                .respond(AgentRequest::RecordActionHit {
                    action: action.clone()
                })
                .await,
            AgentResponse::ActionHitRecorded
        ));
        assert_eq!(
            agent.stats(),
            AgentStats {
                lookups: 1,
                hits: 1,
                ..AgentStats::default()
            }
        );
    }

    #[tokio::test]
    async fn missing_action_result_is_a_cache_miss() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "test-version");
        let action = CacheDigest::blake3(b"missing action");
        let response = agent
            .respond(AgentRequest::FindActionResult {
                action: action.clone(),
            })
            .await;

        assert!(matches!(
            response,
            AgentResponse::ActionResult { result: None }
        ));
        assert!(matches!(
            agent
                .respond(AgentRequest::RecordActionHit { action })
                .await,
            AgentResponse::Error { .. }
        ));
        assert_eq!(
            agent.stats(),
            AgentStats {
                lookups: 1,
                ..AgentStats::default()
            }
        );
    }

    #[tokio::test]
    async fn coalesces_repeated_remote_action_lookups() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let action = CacheDigest::blake3(b"remote action");
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: None,
            output_root: None,
            version: 1,
        };
        let remote = server
            .mock("GET", action_path(&action).as_str())
            .with_status(200)
            .with_header("content-type", ACTION_RESULT_MEDIA_TYPE)
            .with_body(serde_json::to_vec(&result).unwrap())
            .expect(1)
            .create_async()
            .await;
        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );

        for _ in 0..2 {
            assert!(matches!(
                agent
                    .respond(AgentRequest::FindActionResult {
                        action: action.clone(),
                    })
                    .await,
                AgentResponse::ActionResult {
                    result: Some(found)
                } if found == result
            ));
        }
        remote.assert_async().await;
    }

    #[tokio::test]
    async fn publishes_only_successfully_committed_task_action_manifests() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("cache");
        let task = "a".repeat(64);
        let first_invocation = CacheDigest::blake3(b"first invocation");
        let first = ActionPrediction {
            invocation: first_invocation.clone(),
            action: CacheDigest::blake3(b"first action"),
            adapter: "rustc".into(),
            payload: "{}".into(),
        };

        let agent = CacheAgent::new(&cache, "test-version");
        let first_run = agent.begin_task(&task).await.unwrap();
        assert!(matches!(
            agent
                .respond(AgentRequest::RecordActionPrediction {
                    task: first_run.clone(),
                    prediction: first.clone(),
                })
                .await,
            AgentResponse::ActionPredictionRecorded
        ));
        agent.commit_task(&first_run).await.unwrap();

        let uncommitted = CacheAgent::new(&cache, "test-version");
        let uncommitted_run = uncommitted.begin_task(&task).await.unwrap();
        let second_invocation = CacheDigest::blake3(b"second invocation");
        assert!(matches!(
            uncommitted
                .respond(AgentRequest::RecordActionPrediction {
                    task: uncommitted_run,
                    prediction: ActionPrediction {
                        invocation: second_invocation.clone(),
                        action: CacheDigest::blake3(b"second action"),
                        adapter: "rustc".into(),
                        payload: "{}".into(),
                    },
                })
                .await,
            AgentResponse::ActionPredictionRecorded
        ));

        let next_session = CacheAgent::new(&cache, "test-version");
        let next_run = next_session.begin_task(&task).await.unwrap();
        assert!(matches!(
            next_session
                .respond(AgentRequest::FindActionPrediction {
                    task: next_run.clone(),
                    invocation: first_invocation,
                })
                .await,
            AgentResponse::ActionPrediction {
                prediction: Some(prediction)
            } if prediction == first
        ));
        assert!(matches!(
            next_session
                .respond(AgentRequest::FindActionPrediction {
                    task: next_run,
                    invocation: second_invocation,
                })
                .await,
            AgentResponse::ActionPrediction { prediction: None }
        ));

        let corrupt_task = "b".repeat(64);
        fs::create_dir_all(next_session.manifest_dir.as_path()).unwrap();
        fs::write(next_session.task_manifest_path(&corrupt_task), b"not json").unwrap();
        assert!(next_session.begin_task(&corrupt_task).await.is_err());
        let corrupt_run = "c".repeat(64);
        next_session.task_actions.lock().unwrap().insert(
            corrupt_run.clone(),
            TaskActionState {
                manifest: corrupt_task.clone(),
                ..TaskActionState::default()
            },
        );
        assert!(matches!(
            next_session
                .respond(AgentRequest::RecordActionPrediction {
                    task: corrupt_run.clone(),
                    prediction: first,
                })
                .await,
            AgentResponse::ActionPredictionRecorded
        ));
        assert!(next_session.commit_task(&corrupt_run).await.is_err());
        assert_eq!(
            fs::read(next_session.task_manifest_path(&corrupt_task)).unwrap(),
            b"not json"
        );
    }

    #[tokio::test]
    async fn round_trips_task_actions_between_fresh_local_caches() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let task = "e".repeat(64);
        let invocation = CacheDigest::blake3(b"remote invocation");
        let action_bytes = canonical_json(&serde_json::json!({"kind":"rustc"})).unwrap();
        let stdout_bytes = b"cached stdout".to_vec();
        let stderr_bytes = b"cached stderr".to_vec();
        let artifact_bytes = b"cached artifact".to_vec();
        let stdout = CacheDigest::blake3(&stdout_bytes);
        let stderr = CacheDigest::blake3(&stderr_bytes);
        let artifact = CacheDigest::blake3(&artifact_bytes);
        let metadata_bytes = canonical_json(&RustcMetadata {
            version: 1,
            kind: "rustc".into(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
        })
        .unwrap();
        let directory_bytes = canonical_json(&serde_json::json!({
            "directories":[],
            "files":[{"digest":artifact,"executable":false,"mode":420,"name":"artifact"}],
            "symlinks":[],
            "version":1
        }))
        .unwrap();
        let action = CacheDigest::blake3(&action_bytes);
        let metadata = CacheDigest::blake3(&metadata_bytes);
        let output_root = CacheDigest::blake3(&directory_bytes);
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: Some(metadata.clone()),
            output_root: Some(output_root.clone()),
            version: 1,
        };
        let prediction = ActionPrediction {
            invocation: invocation.clone(),
            action: action.clone(),
            adapter: "rustc".into(),
            payload: "{}".into(),
        };
        let manifest_bytes = canonical_json(&TaskActionManifest {
            version: TASK_ACTION_MANIFEST_VERSION,
            task: task.clone(),
            predictions: vec![prediction.clone()],
        })
        .unwrap();
        let manifest_etag = blake3::hash(&manifest_bytes).to_hex().to_string();
        let (_, selector) = CacheAgent::task_manifest_selector(&task).unwrap();

        let mut mocks = Vec::new();
        for (digest, bytes) in [
            (&action, action_bytes.as_slice()),
            (&metadata, metadata_bytes.as_slice()),
            (&output_root, directory_bytes.as_slice()),
            (&stdout, stdout_bytes.as_slice()),
            (&stderr, stderr_bytes.as_slice()),
            (&artifact, artifact_bytes.as_slice()),
        ] {
            mocks.push(
                server
                    .mock("PUT", blob_path(digest).as_str())
                    .match_header("mise-cache-namespace", "test")
                    .match_body(bytes.to_vec())
                    .with_status(200)
                    .expect(1)
                    .create_async()
                    .await,
            );
        }
        mocks.push(
            server
                .mock("PUT", action_path(&result.action).as_str())
                .match_header("mise-cache-namespace", "test")
                .with_status(200)
                .expect(1)
                .create_async()
                .await,
        );
        mocks.push(
            server
                .mock("PUT", action_manifest_path(&selector).as_str())
                .match_header("mise-cache-namespace", "test")
                .match_header("if-none-match", "*")
                .match_body(manifest_bytes.clone())
                .with_status(201)
                .expect(1)
                .create_async()
                .await,
        );
        mocks.push(
            server
                .mock("GET", action_manifest_path(&selector).as_str())
                .with_status(200)
                .with_header("etag", &format!("\"{manifest_etag}\""))
                .with_body(manifest_bytes.clone())
                .expect(1)
                .create_async()
                .await,
        );
        mocks.push(
            server
                .mock("GET", action_path(&action).as_str())
                .with_status(200)
                .with_header("content-type", ACTION_RESULT_MEDIA_TYPE)
                .with_body(serde_json::to_vec(&result).unwrap())
                .expect(1)
                .create_async()
                .await,
        );
        for (digest, bytes) in [
            (&action, action_bytes.as_slice()),
            (&metadata, metadata_bytes.as_slice()),
            (&output_root, directory_bytes.as_slice()),
            (&stdout, stdout_bytes.as_slice()),
            (&stderr, stderr_bytes.as_slice()),
            (&artifact, artifact_bytes.as_slice()),
        ] {
            mocks.push(
                server
                    .mock("GET", blob_path(digest).as_str())
                    .with_status(200)
                    .with_body(bytes)
                    .expect(1)
                    .create_async()
                    .await,
            );
        }

        let writer = remote_agent(
            &server,
            directory.path().join("writer"),
            RemoteCacheMode::WriteOnly,
        );
        for (index, (digest, bytes)) in [
            (&action, action_bytes.as_slice()),
            (&metadata, metadata_bytes.as_slice()),
            (&output_root, directory_bytes.as_slice()),
            (&stdout, stdout_bytes.as_slice()),
            (&stderr, stderr_bytes.as_slice()),
            (&artifact, artifact_bytes.as_slice()),
        ]
        .into_iter()
        .enumerate()
        {
            let source = directory.path().join(format!("source-{index}"));
            fs::write(&source, bytes).unwrap();
            assert!(matches!(
                writer
                    .respond(AgentRequest::StoreBlob {
                        digest: digest.clone(),
                        source,
                    })
                    .await,
                AgentResponse::Stored { .. }
            ));
        }
        assert!(matches!(
            writer
                .respond(AgentRequest::StoreActionResult {
                    result: result.clone(),
                })
                .await,
            AgentResponse::ActionStored { .. }
        ));
        let run = writer.begin_task(&task).await.unwrap();
        assert!(matches!(
            writer
                .respond(AgentRequest::RecordActionPrediction {
                    task: run.clone(),
                    prediction: prediction.clone(),
                })
                .await,
            AgentResponse::ActionPredictionRecorded
        ));
        writer.commit_task(&run).await.unwrap();

        let reader = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );
        let run = reader.begin_task(&task).await.unwrap();
        reader.wait_for_prefetches().await;
        assert!(matches!(
            reader
                .respond(AgentRequest::FindActionPrediction {
                    task: run,
                    invocation,
                })
                .await,
            AgentResponse::ActionPrediction {
                prediction: Some(found)
            } if found == prediction
        ));
        assert!(matches!(
            reader
                .respond(AgentRequest::FindActionResult {
                    action: action.clone(),
                })
                .await,
            AgentResponse::ActionResult {
                result: Some(found)
            } if found == result
        ));
        for digest in [&action, &metadata, &output_root] {
            assert!(matches!(
                reader
                    .respond(AgentRequest::FindBlob {
                        digest: digest.clone(),
                    })
                    .await,
                AgentResponse::Blob { path: Some(_) }
            ));
        }
        assert!(matches!(
            reader
                .respond(AgentRequest::RecordActionHit { action })
                .await,
            AgentResponse::ActionHitRecorded
        ));
        for mock in mocks {
            mock.assert_async().await;
        }
    }

    #[tokio::test]
    async fn keeps_newer_local_predictions_when_remote_manifest_is_stale() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let task = "f".repeat(64);
        let invocation = CacheDigest::blake3(b"shared invocation");
        let local_prediction = ActionPrediction {
            invocation: invocation.clone(),
            action: CacheDigest::blake3(b"new local action"),
            adapter: "rustc".into(),
            payload: "{}".into(),
        };
        let remote_prediction = ActionPrediction {
            invocation: invocation.clone(),
            action: CacheDigest::blake3(b"stale remote action"),
            adapter: "rustc".into(),
            payload: "{}".into(),
        };
        let remote_manifest = TaskActionManifest {
            version: TASK_ACTION_MANIFEST_VERSION,
            task: task.clone(),
            predictions: vec![remote_prediction],
        };
        let remote_bytes = canonical_json(&remote_manifest).unwrap();
        let remote_etag = blake3::hash(&remote_bytes).to_hex().to_string();
        let (_, selector) = CacheAgent::task_manifest_selector(&task).unwrap();
        let remote = server
            .mock("GET", action_manifest_path(&selector).as_str())
            .with_status(200)
            .with_header("etag", &format!("\"{remote_etag}\""))
            .with_body(remote_bytes)
            .expect(1)
            .create_async()
            .await;

        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );
        agent
            .persist_task_manifest(&TaskActionManifest {
                version: TASK_ACTION_MANIFEST_VERSION,
                task: task.clone(),
                predictions: vec![local_prediction.clone()],
            })
            .unwrap();

        let run = agent.begin_task(&task).await.unwrap();
        assert!(matches!(
            agent
                .respond(AgentRequest::FindActionPrediction {
                    task: run,
                    invocation,
                })
                .await,
            AgentResponse::ActionPrediction {
                prediction: Some(found)
            } if found == local_prediction
        ));
        let persisted = agent.load_task_manifest(&task).unwrap().unwrap();
        assert_eq!(persisted.predictions, vec![local_prediction]);
        remote.assert_async().await;
    }

    #[tokio::test]
    async fn prefetch_does_not_block_task_initialization() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let task = "9".repeat(64);
        let invocation = CacheDigest::blake3(b"prefetched invocation");
        let action_bytes = b"prefetched action";
        let action = CacheDigest::blake3(action_bytes);
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: None,
            output_root: None,
            version: 1,
        };
        let manifest_bytes = canonical_json(&TaskActionManifest {
            version: TASK_ACTION_MANIFEST_VERSION,
            task: task.clone(),
            predictions: vec![ActionPrediction {
                invocation,
                action: action.clone(),
                adapter: "rustc".into(),
                payload: "{}".into(),
            }],
        })
        .unwrap();
        let manifest_etag = blake3::hash(&manifest_bytes).to_hex().to_string();
        let (_, selector) = CacheAgent::task_manifest_selector(&task).unwrap();
        let manifest = server
            .mock("GET", action_manifest_path(&selector).as_str())
            .with_status(200)
            .with_header("etag", &format!("\"{manifest_etag}\""))
            .with_body(manifest_bytes)
            .expect(1)
            .create_async()
            .await;
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let response_release = release.clone();
        let result_bytes = serde_json::to_vec(&result).unwrap();
        let action_result = server
            .mock("GET", action_path(&action).as_str())
            .with_status(200)
            .with_header("content-type", ACTION_RESULT_MEDIA_TYPE)
            .with_chunked_body(move |writer| {
                let (released, condition) = &*response_release;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = condition.wait(released).unwrap();
                }
                std::io::Write::write_all(writer, &result_bytes)
            })
            .expect(1)
            .create_async()
            .await;
        let action_blob = server
            .mock("GET", blob_path(&action).as_str())
            .with_status(200)
            .with_body(action_bytes)
            .expect(1)
            .create_async()
            .await;
        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );

        let begin = tokio::time::timeout(Duration::from_secs(2), agent.begin_task(&task)).await;
        let (released, condition) = &*release;
        *released.lock().unwrap() = true;
        condition.notify_all();
        let run = begin
            .expect("task initialization waited for prefetch")
            .unwrap();
        assert_eq!(
            agent
                .task_actions
                .lock()
                .unwrap()
                .get(&run)
                .unwrap()
                .predictions
                .len(),
            1
        );
        agent.wait_for_prefetches().await;
        manifest.assert_async().await;
        action_result.assert_async().await;
        action_blob.assert_async().await;
        assert!(agent.actions.find(&action).unwrap().is_some());
    }

    #[tokio::test]
    async fn session_completion_cancels_outstanding_prefetches() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "test-version");
        let task = tokio::spawn(std::future::pending::<()>());
        agent.prefetch_tasks.lock().unwrap().push(task);

        tokio::time::timeout(Duration::from_secs(1), agent.cancel_prefetches())
            .await
            .expect("prefetch cancellation blocked session completion");
        assert!(agent.prefetch_tasks.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn prefetch_reserves_capacity_for_foreground_transfers() {
        let transfers = tokio::sync::Semaphore::new(MAX_REMOTE_TRANSFERS);
        let _prefetch = transfers
            .acquire_many(MAX_PREFETCH_CONCURRENCY as u32)
            .await
            .unwrap();
        assert!(transfers.available_permits() > 0);
    }

    fn remote_agent(
        server: &mockito::ServerGuard,
        cache_dir: PathBuf,
        mode: RemoteCacheMode,
    ) -> CacheAgent {
        let client = RemoteCacheClient::new(crate::RemoteCacheConfig {
            base_url: server.url().parse().unwrap(),
            namespace: "test".into(),
            token: None,
            token_file: None,
            oidc_audience: None,
            connect_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_secs(1),
            download_timeout: Duration::from_secs(1),
            retries: 0,
        })
        .unwrap();
        CacheAgent::new_remote(
            &cache_dir,
            "test-version",
            AgentRemoteCache {
                client,
                mode,
                staging_dir: cache_dir.join("remote"),
            },
        )
    }

    fn blob_path(digest: &CacheDigest) -> String {
        format!(
            "/v1/blobs/{}/{}/{}",
            digest.algorithm, digest.hash, digest.size
        )
    }

    fn action_path(digest: &CacheDigest) -> String {
        format!(
            "/v1/action-results/{}/{}/{}",
            digest.algorithm, digest.hash, digest.size
        )
    }

    fn action_manifest_path(digest: &CacheDigest) -> String {
        format!(
            "/v1/action-manifests/{}/{}/{}",
            digest.algorithm, digest.hash, digest.size
        )
    }

    #[tokio::test]
    async fn merges_overlapping_runs_into_one_task_manifest() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("cache");
        let task = "d".repeat(64);
        let agent = CacheAgent::new(&cache, "test-version");
        let first_run = agent.begin_task(&task).await.unwrap();
        let second_run = agent.begin_task(&task).await.unwrap();
        assert_ne!(first_run, second_run);
        let first_invocation = CacheDigest::blake3(b"overlap one");
        let second_invocation = CacheDigest::blake3(b"overlap two");
        for (run, invocation) in [
            (&first_run, &first_invocation),
            (&second_run, &second_invocation),
        ] {
            assert!(matches!(
                agent
                    .respond(AgentRequest::RecordActionPrediction {
                        task: run.clone(),
                        prediction: ActionPrediction {
                            invocation: invocation.clone(),
                            action: CacheDigest::blake3(invocation.hash.as_bytes()),
                            adapter: "rustc".into(),
                            payload: "{}".into(),
                        },
                    })
                    .await,
                AgentResponse::ActionPredictionRecorded
            ));
        }
        agent.commit_task(&first_run).await.unwrap();
        agent.commit_task(&second_run).await.unwrap();

        let next = CacheAgent::new(cache, "test-version");
        let run = next.begin_task(&task).await.unwrap();
        for invocation in [first_invocation, second_invocation] {
            assert!(matches!(
                next.respond(AgentRequest::FindActionPrediction {
                    task: run.clone(),
                    invocation,
                })
                .await,
                AgentResponse::ActionPrediction {
                    prediction: Some(_)
                }
            ));
        }
    }

    #[test]
    fn keeps_local_manifest_when_remote_merge_exceeds_prediction_limit() {
        let task = "7".repeat(64);
        let prediction = |index: usize| {
            let digest = CacheDigest::blake3(&index.to_le_bytes());
            ActionPrediction {
                invocation: digest.clone(),
                action: digest,
                adapter: "rustc".into(),
                payload: "{}".into(),
            }
        };
        let local = TaskActionManifest {
            version: TASK_ACTION_MANIFEST_VERSION,
            task: task.clone(),
            predictions: (0..MAX_TASK_ACTION_PREDICTIONS).map(prediction).collect(),
        };
        let expected_first = local.predictions[0].clone();
        let remote = TaskActionManifest {
            version: TASK_ACTION_MANIFEST_VERSION,
            task: task.clone(),
            predictions: vec![prediction(MAX_TASK_ACTION_PREDICTIONS)],
        };

        let (manifest, merged) = merge_remote_task_manifest(&task, remote, local);
        assert!(!merged);
        assert_eq!(manifest.predictions.len(), MAX_TASK_ACTION_PREDICTIONS);
        assert_eq!(manifest.predictions[0], expected_first);
    }

    #[test]
    fn task_manifest_lock_is_shared_across_agents() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("cache");
        let first = CacheAgent::new(&cache, "test-version");
        let second = CacheAgent::new(&cache, "test-version");
        let task = "8".repeat(64);

        let first_lock = first.lock_task_manifest(&task).unwrap();
        let mut contender = fslock::LockFile::open(&second.task_manifest_lock_path(&task)).unwrap();
        assert!(!contender.try_lock().unwrap());
        drop(first_lock);
        assert!(contender.try_lock().unwrap());
    }

    #[tokio::test]
    async fn memoizes_client_observed_executable_identities() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "test-version");
        let executable = directory.path().join("rustc");
        let environment = BTreeMap::from([("RUSTUP_TOOLCHAIN".into(), Some("stable".into()))]);

        let response = agent
            .respond(AgentRequest::FindExecutableIdentity {
                executable: executable.clone(),
                environment: environment.clone(),
            })
            .await;
        assert!(matches!(
            response,
            AgentResponse::ExecutableIdentity { stdout: None }
        ));

        let response = agent
            .respond(AgentRequest::StoreExecutableIdentity {
                executable: executable.clone(),
                environment: environment.clone(),
                stdout: b"rustc identity".to_vec(),
            })
            .await;
        assert!(matches!(
            response,
            AgentResponse::ExecutableIdentity {
                stdout: Some(stdout)
            } if stdout == b"rustc identity"
        ));

        let response = agent
            .respond(AgentRequest::FindExecutableIdentity {
                executable,
                environment,
            })
            .await;
        assert!(matches!(
            response,
            AgentResponse::ExecutableIdentity {
                stdout: Some(stdout)
            } if stdout == b"rustc identity"
        ));
    }

    #[test]
    fn bounds_executable_identity_entry_count() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "test-version");
        for index in 0..MAX_EXECUTABLE_IDENTITIES {
            agent
                .store_executable_identity(
                    directory.path().join(format!("rustc-{index}")),
                    BTreeMap::new(),
                    vec![b'x'],
                )
                .unwrap();
        }

        assert!(
            agent
                .store_executable_identity(
                    directory.path().join("one-too-many"),
                    BTreeMap::new(),
                    vec![b'x'],
                )
                .is_err()
        );
    }

    #[test]
    fn bounds_executable_identity_retained_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "test-version");
        for index in 0..MAX_EXECUTABLE_IDENTITY_BYTES / MAX_EXECUTABLE_IDENTITY_SIZE {
            agent
                .store_executable_identity(
                    directory.path().join(format!("rustc-{index}")),
                    BTreeMap::new(),
                    vec![b'x'; MAX_EXECUTABLE_IDENTITY_SIZE],
                )
                .unwrap();
        }

        assert!(
            agent
                .store_executable_identity(
                    directory.path().join("one-byte-too-many"),
                    BTreeMap::new(),
                    vec![b'x'],
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn version_skew_is_a_handshake_miss() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path(), "agent-version");
        let (mut client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move { agent.handle_connection(server).await });
        let request = AgentRequest::Hello {
            protocol: AGENT_PROTOCOL_VERSION,
            client_version: "other-version".into(),
        };
        let mut encoded = serde_json::to_vec(&request).unwrap();
        encoded.push(b'\n');
        client.write_all(&encoded).await.unwrap();
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .unwrap();

        assert!(matches!(
            serde_json::from_str(&response).unwrap(),
            AgentResponse::Error { .. }
        ));
        task.await.unwrap().unwrap();
    }
}
