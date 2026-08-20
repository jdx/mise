use crate::{
    BlobSource, BlobUpload, CacheDigest, CacheDirectory, LocalActionCache, LocalCas,
    ManifestPutOutcome, RemoteActionResult, RemoteCacheClient, RemoteCacheMode, RustcMetadata,
    canonical_json,
};
use eyre::{Result, bail};
use futures_util::{FutureExt, StreamExt, future::BoxFuture, stream};
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_EXECUTABLE_IDENTITIES: usize = 64;
const MAX_EXECUTABLE_IDENTITY_SIZE: usize = 64 * 1024;
const MAX_EXECUTABLE_IDENTITY_BYTES: usize = 256 * 1024;
const TASK_ACTION_MANIFEST_VERSION: u8 = 1;
const MAX_TASK_ACTION_PREDICTIONS: usize = 16 * 1024;
const MAX_ACTION_PREDICTION_PAYLOAD: usize = 256 * 1024;
const MAX_REMOTE_TRANSFERS: usize = 64;
const MAX_PREFETCH_TRANSFERS: usize = 48;
const MAX_PREFETCH_ACTION_BATCH: usize = 256;
const PREFETCH_ACTION_BATCH_DELAY: Duration = Duration::from_millis(5);
const MAX_PREFETCH_DIRECTORY_OBJECTS: usize = 100_000;
const MAX_PREFETCH_OBJECTS_PER_WAVE: usize = 100_000;

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
    /// Resolve a blob to a session-verified local CAS path.
    FindBlob {
        digest: CacheDigest,
    },
    /// Resolve blobs to session-verified local CAS paths.
    FindBlobs {
        digests: Vec<CacheDigest>,
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
        restore: RestoreStats,
    },
    RecordActionVerification {
        matched: bool,
        restore: RestoreStats,
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

/// Local output restoration work performed by one action-cache adapter hit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreStats {
    /// Cumulative time spent materializing and validating output files.
    pub duration_ns: u64,
    /// Number of compiler output files restored.
    pub output_files: u64,
    /// Declared size of compiler output files restored.
    pub output_bytes: u64,
}

/// A response returned by the task-scoped cache agent.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentResponse {
    Hello {
        protocol: u8,
        agent_version: String,
    },
    /// A local CAS path already verified against the requested digest.
    Blob {
        path: Option<PathBuf>,
    },
    /// Local CAS paths already verified against the requested digests.
    Blobs {
        paths: Vec<Option<PathBuf>>,
    },
    Stored {
        path: PathBuf,
    },
    ActionResult {
        result: Option<RemoteActionResult>,
    },
    ActionHitRecorded,
    ActionVerificationRecorded,
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
    /// End-to-end lifetime of the task-scoped cache session.
    pub session_duration_ns: u64,
    /// Number of action-result lookups.
    pub lookups: u64,
    /// Number of lookups that found a valid local action result.
    pub hits: u64,
    /// Number of newly stored content-addressed objects.
    pub stores: u64,
    /// Total size of newly stored objects.
    pub stored_bytes: u64,
    /// Number of cache hits compiled again for qualification.
    pub verifications: u64,
    /// Number of qualification builds that diverged from the cached result.
    pub divergences: u64,
    /// CAS payload bytes downloaded from the remote cache.
    pub downloaded_bytes: u64,
    /// CAS payload bytes uploaded to the remote cache.
    pub uploaded_bytes: u64,
    /// Complete actions staged before an adapter requested them.
    pub prefetched_actions: u64,
    /// Number of task manifest requests made to the remote cache.
    pub remote_manifest_lookups: u64,
    /// Cumulative time spent requesting remote task manifests.
    pub remote_manifest_lookup_duration_ns: u64,
    /// Number of action-result requests made to the remote cache.
    pub remote_action_lookups: u64,
    /// Cumulative time spent requesting remote action results.
    pub remote_action_lookup_duration_ns: u64,
    /// Number of blob requests made to the remote cache.
    pub remote_blob_requests: u64,
    /// Number of packed blob requests made to the remote cache.
    pub remote_blob_pack_requests: u64,
    /// Number of verified blobs received through packed responses.
    pub remote_blob_pack_blobs: u64,
    /// Cumulative time spent downloading and verifying remote blobs.
    pub remote_blob_transfer_duration_ns: u64,
    /// Cumulative time spent ingesting downloaded blobs into the local CAS.
    pub local_cas_write_duration_ns: u64,
    /// Number of speculative prefetch runs started for task manifests.
    pub prefetch_runs: u64,
    /// Cumulative wall time of speculative task-manifest prefetch runs.
    pub prefetch_duration_ns: u64,
    /// Cumulative time spent staging or materializing and validating cached outputs.
    pub materialization_duration_ns: u64,
    /// Number of compiler output files restored from action hits.
    pub restored_output_files: u64,
    /// Declared size of compiler output files restored from action hits.
    pub restored_output_bytes: u64,
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
    verifications: AtomicU64,
    divergences: AtomicU64,
    downloaded_bytes: AtomicU64,
    uploaded_bytes: AtomicU64,
    prefetched_actions: AtomicU64,
    remote_manifest_lookups: AtomicU64,
    remote_manifest_lookup_duration_ns: AtomicU64,
    remote_action_lookups: AtomicU64,
    remote_action_lookup_duration_ns: AtomicU64,
    remote_blob_requests: AtomicU64,
    remote_blob_pack_requests: AtomicU64,
    remote_blob_pack_blobs: AtomicU64,
    remote_blob_transfer_duration_ns: AtomicU64,
    local_cas_write_duration_ns: AtomicU64,
    prefetch_runs: AtomicU64,
    prefetch_duration_ns: AtomicU64,
    materialization_duration_ns: AtomicU64,
    restored_output_files: AtomicU64,
    restored_output_bytes: AtomicU64,
}

struct AtomicDurationTimer<'a> {
    started: Instant,
    target: &'a AtomicU64,
}

impl<'a> AtomicDurationTimer<'a> {
    fn start(target: &'a AtomicU64) -> Self {
        Self {
            started: Instant::now(),
            target,
        }
    }
}

impl Drop for AtomicDurationTimer<'_> {
    fn drop(&mut self) {
        atomic_saturating_add(self.target, duration_ns(self.started));
    }
}

fn duration_ns(started: Instant) -> u64 {
    started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX)
}

fn atomic_saturating_add(target: &AtomicU64, value: u64) {
    let _ = target.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

fn queue_prefetch_digest(
    verified: &BTreeMap<CacheDigest, PathBuf>,
    pending: &mut BTreeMap<CacheDigest, ()>,
    digest: CacheDigest,
) {
    if verified.contains_key(&digest) || pending.contains_key(&digest) {
        return;
    }
    pending.insert(digest, ());
}

fn queue_prefetch_directory(
    seen: &BTreeMap<CacheDigest, ()>,
    pending: &mut BTreeMap<CacheDigest, ()>,
    digest: CacheDigest,
    limit: usize,
) -> bool {
    if seen.contains_key(&digest) || pending.contains_key(&digest) {
        return true;
    }
    if seen.len().saturating_add(pending.len()) >= limit {
        return false;
    }
    pending.insert(digest, ());
    true
}

/// Shared state for an agent hosted by the top-level `mise run` process.
///
/// Transport listeners deliberately live in mise so the task-run lifecycle owns
/// them. This type only contains ecosystem-independent CAS and protocol logic.
#[derive(Clone)]
pub struct CacheAgent {
    cas: LocalCas,
    actions: LocalActionCache,
    verified_blobs: Arc<Mutex<BTreeMap<CacheDigest, PathBuf>>>,
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
    prefetch_transfers: Arc<tokio::sync::Semaphore>,
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

struct PrefetchedAction {
    adapter: String,
    result: RemoteActionResult,
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
            verified_blobs: Arc::new(Mutex::new(BTreeMap::new())),
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
            prefetch_transfers: Arc::new(tokio::sync::Semaphore::new(MAX_PREFETCH_TRANSFERS)),
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
        self.stats
            .remote_manifest_lookups
            .fetch_add(1, Ordering::Relaxed);
        let _timer = AtomicDurationTimer::start(&self.stats.remote_manifest_lookup_duration_ns);
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
            session_duration_ns: 0,
            lookups: self.stats.lookups.load(Ordering::Relaxed),
            hits: self.stats.hits.load(Ordering::Relaxed),
            stores: self.stats.stores.load(Ordering::Relaxed),
            stored_bytes: self.stats.stored_bytes.load(Ordering::Relaxed),
            verifications: self.stats.verifications.load(Ordering::Relaxed),
            divergences: self.stats.divergences.load(Ordering::Relaxed),
            downloaded_bytes: self.stats.downloaded_bytes.load(Ordering::Relaxed),
            uploaded_bytes: self.stats.uploaded_bytes.load(Ordering::Relaxed),
            prefetched_actions: self.stats.prefetched_actions.load(Ordering::Relaxed),
            remote_manifest_lookups: self.stats.remote_manifest_lookups.load(Ordering::Relaxed),
            remote_manifest_lookup_duration_ns: self
                .stats
                .remote_manifest_lookup_duration_ns
                .load(Ordering::Relaxed),
            remote_action_lookups: self.stats.remote_action_lookups.load(Ordering::Relaxed),
            remote_action_lookup_duration_ns: self
                .stats
                .remote_action_lookup_duration_ns
                .load(Ordering::Relaxed),
            remote_blob_requests: self.stats.remote_blob_requests.load(Ordering::Relaxed),
            remote_blob_pack_requests: self.stats.remote_blob_pack_requests.load(Ordering::Relaxed),
            remote_blob_pack_blobs: self.stats.remote_blob_pack_blobs.load(Ordering::Relaxed),
            remote_blob_transfer_duration_ns: self
                .stats
                .remote_blob_transfer_duration_ns
                .load(Ordering::Relaxed),
            local_cas_write_duration_ns: self
                .stats
                .local_cas_write_duration_ns
                .load(Ordering::Relaxed),
            prefetch_runs: self.stats.prefetch_runs.load(Ordering::Relaxed),
            prefetch_duration_ns: self.stats.prefetch_duration_ns.load(Ordering::Relaxed),
            materialization_duration_ns: self
                .stats
                .materialization_duration_ns
                .load(Ordering::Relaxed),
            restored_output_files: self.stats.restored_output_files.load(Ordering::Relaxed),
            restored_output_bytes: self.stats.restored_output_bytes.load(Ordering::Relaxed),
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
        self.stats.prefetch_runs.fetch_add(1, Ordering::Relaxed);
        let _timer = AtomicDurationTimer::start(&self.stats.prefetch_duration_ns);
        let mut actions = BTreeMap::new();
        for prediction in predictions {
            actions
                .entry(prediction.action.clone())
                .or_insert_with(|| prediction.adapter.clone());
        }
        let mut actions = actions.into_iter();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..MAX_PREFETCH_TRANSFERS {
            let Some((action, adapter)) = actions.next() else {
                break;
            };
            let agent = self.clone();
            tasks.spawn(async move { agent.resolve_prefetch_action(action, adapter).await });
        }
        let mut resolved = Vec::new();
        while !tasks.is_empty() {
            let result = if resolved.is_empty() {
                tasks.join_next().await
            } else {
                match tokio::time::timeout(PREFETCH_ACTION_BATCH_DELAY, tasks.join_next()).await {
                    Ok(result) => result,
                    Err(_) => {
                        self.prefetch_resolved_actions(std::mem::take(&mut resolved))
                            .await;
                        continue;
                    }
                }
            };
            let Some(result) = result else {
                break;
            };
            match result {
                Ok(Ok(Some(action))) => resolved.push(action),
                Ok(Ok(None)) => {}
                Ok(Err(error)) => warn!("remote action prefetch failed: {error}"),
                Err(error) => warn!("remote action prefetch task failed: {error}"),
            }
            if let Some((action, adapter)) = actions.next() {
                let agent = self.clone();
                tasks.spawn(async move { agent.resolve_prefetch_action(action, adapter).await });
            }
            if resolved.len() == MAX_PREFETCH_ACTION_BATCH {
                self.prefetch_resolved_actions(std::mem::take(&mut resolved))
                    .await;
            }
        }
        if !resolved.is_empty() {
            self.prefetch_resolved_actions(resolved).await;
        }
    }

    #[cfg(test)]
    async fn prefetch_action(&self, action: CacheDigest, adapter: String) -> Result<()> {
        if let Some(action) = self.resolve_prefetch_action(action, adapter).await? {
            self.prefetch_resolved_actions(vec![action]).await;
        }
        Ok(())
    }

    async fn resolve_prefetch_action(
        &self,
        action: CacheDigest,
        adapter: String,
    ) -> Result<Option<PrefetchedAction>> {
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| eyre::eyre!("remote cache is not configured"))?;
        let result = {
            let lock = self.action_lock(&action);
            let _guard = lock.lock().await;
            if self.actions.find(&action)?.is_some() {
                return Ok(None);
            }
            if let Some(result) = self
                .pending_remote_actions
                .lock()
                .unwrap()
                .get(&action)
                .cloned()
            {
                result
            } else {
                let _prefetch_permit = self.prefetch_transfers.acquire().await?;
                let result = {
                    let _permit = self.remote_transfers.acquire().await?;
                    self.get_remote_action_result(remote, &action).await?
                };
                let Some(result) = result else {
                    return Ok(None);
                };
                self.pending_remote_actions
                    .lock()
                    .unwrap()
                    .insert(action.clone(), result.clone());
                result
            }
        };
        Ok(Some(PrefetchedAction { adapter, result }))
    }

    fn prefetch_resolved_actions(&self, actions: Vec<PrefetchedAction>) -> BoxFuture<'_, ()> {
        self.prefetch_resolved_actions_inner(actions).boxed()
    }

    async fn prefetch_resolved_actions_inner(&self, actions: Vec<PrefetchedAction>) {
        let Some(remote) = self.remote.as_deref() else {
            return;
        };
        if actions.is_empty() {
            return;
        }

        let mut top_level = BTreeMap::new();
        for action in &actions {
            for digest in [
                Some(&action.result.action),
                action.result.metadata.as_ref(),
                action.result.output_root.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                top_level.insert(digest.clone(), ());
            }
        }
        let mut verified = self
            .fetch_remote_blobs(
                remote,
                top_level.into_keys().collect(),
                Some(&self.prefetch_transfers),
            )
            .await;

        let mut next = BTreeMap::new();
        let mut pending_directories = BTreeMap::new();
        let mut parsed_directories = BTreeMap::new();
        let mut rustc_metadata = BTreeMap::new();
        for action in &actions {
            if action.adapter == "rustc"
                && let Some(metadata_digest) = &action.result.metadata
            {
                match verified
                    .get(metadata_digest)
                    .ok_or_else(|| eyre::eyre!("remote rustc action metadata is missing"))
                    .and_then(|path| Self::parse_rustc_metadata(path))
                {
                    Ok(metadata) => {
                        queue_prefetch_digest(&verified, &mut next, metadata.stdout.clone());
                        queue_prefetch_digest(&verified, &mut next, metadata.stderr.clone());
                        rustc_metadata.insert(metadata_digest.clone(), metadata);
                    }
                    Err(error) => warn!(
                        "remote rustc action metadata prefetch failed for {}: {error}",
                        action.result.action.hash
                    ),
                }
            }
            if let Some(output_root) = &action.result.output_root {
                pending_directories.insert(output_root.clone(), ());
            }
        }

        let mut seen_directories = BTreeMap::new();
        loop {
            let mut following = BTreeMap::new();
            let mut directory_limit_exceeded = false;
            for digest in pending_directories.into_keys() {
                following.remove(&digest);
                if seen_directories.insert(digest.clone(), ()).is_some() {
                    continue;
                }
                if seen_directories.len() > MAX_PREFETCH_DIRECTORY_OBJECTS {
                    warn!("remote action output tree is too large to prefetch");
                    following.clear();
                    break;
                }
                match verified
                    .get(&digest)
                    .ok_or_else(|| eyre::eyre!("remote action output directory is missing"))
                    .and_then(|path| Self::parse_cache_directory(path))
                {
                    Ok(directory) => {
                        for file in &directory.files {
                            queue_prefetch_digest(&verified, &mut next, file.digest.clone());
                            if next.len() >= MAX_PREFETCH_OBJECTS_PER_WAVE {
                                self.flush_prefetch_digest_batch(remote, &mut verified, &mut next)
                                    .await;
                            }
                        }
                        for child in &directory.directories {
                            if !queue_prefetch_directory(
                                &seen_directories,
                                &mut following,
                                child.digest.clone(),
                                MAX_PREFETCH_DIRECTORY_OBJECTS,
                            ) {
                                warn!("remote action output tree is too large to prefetch");
                                directory_limit_exceeded = true;
                                break;
                            }
                            queue_prefetch_digest(&verified, &mut next, child.digest.clone());
                            if next.len() >= MAX_PREFETCH_OBJECTS_PER_WAVE {
                                self.flush_prefetch_digest_batch(remote, &mut verified, &mut next)
                                    .await;
                            }
                        }
                        parsed_directories.insert(digest, directory);
                    }
                    Err(error) => warn!(
                        "remote action output directory prefetch failed for {}: {error}",
                        digest.hash
                    ),
                }
                if directory_limit_exceeded {
                    following.clear();
                    break;
                }
            }
            self.flush_prefetch_digest_batch(remote, &mut verified, &mut next)
                .await;
            if following.is_empty() {
                break;
            }
            pending_directories = following;
        }

        for action in actions {
            match Self::validate_prefetched_action(
                &action,
                &verified,
                &rustc_metadata,
                &parsed_directories,
            ) {
                Ok(()) => {
                    if let Err(error) = self.actions.store(&action.result) {
                        warn!(
                            "remote action prefetch could not publish {}: {error}",
                            action.result.action.hash
                        );
                        continue;
                    }
                    self.pending_remote_actions
                        .lock()
                        .unwrap()
                        .remove(&action.result.action);
                    self.stats
                        .prefetched_actions
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => warn!(
                    "remote action prefetch was incomplete for {}: {error}",
                    action.result.action.hash
                ),
            }
        }
    }

    async fn flush_prefetch_digest_batch(
        &self,
        remote: &RemoteCacheClient,
        verified: &mut BTreeMap<CacheDigest, PathBuf>,
        pending: &mut BTreeMap<CacheDigest, ()>,
    ) {
        if pending.is_empty() {
            return;
        }
        let digests = std::mem::take(pending).into_keys().collect();
        verified.extend(
            self.fetch_remote_blobs(remote, digests, Some(&self.prefetch_transfers))
                .await,
        );
    }

    async fn fetch_remote_blobs(
        &self,
        remote: &RemoteCacheClient,
        digests: Vec<CacheDigest>,
        prefetch_limit: Option<&tokio::sync::Semaphore>,
    ) -> BTreeMap<CacheDigest, PathBuf> {
        let mut verified = BTreeMap::new();
        let mut missing = BTreeMap::new();
        for digest in digests {
            match self.find_verified_blob(&digest) {
                Ok(Some(path)) => {
                    verified.insert(digest, path);
                }
                Ok(None) => {
                    missing.insert(digest, ());
                }
                Err(error) => warn!(
                    "local cache blob lookup failed for {}: {error}",
                    digest.hash
                ),
            }
        }
        if missing.is_empty() {
            return verified;
        }

        let mut pack_candidates = missing.clone();
        while !pack_candidates.is_empty() {
            let requested = pack_candidates.keys().cloned().collect::<Vec<_>>();
            let (pack, transfer_duration_ns) = {
                let _prefetch_permit = match prefetch_limit {
                    Some(limit) => match limit.acquire().await {
                        Ok(permit) => Some(permit),
                        Err(error) => {
                            warn!(
                                "remote cache blob pack could not acquire prefetch limit: {error}"
                            );
                            break;
                        }
                    },
                    None => None,
                };
                let _transfer_permit = match self.remote_transfers.acquire().await {
                    Ok(permit) => permit,
                    Err(error) => {
                        warn!("remote cache blob pack could not acquire transfer limit: {error}");
                        break;
                    }
                };
                let transfer_started = Instant::now();
                let pack = remote
                    .get_blob_pack(&requested, self.remote_staging_dir.as_path())
                    .await;
                (pack, duration_ns(transfer_started))
            };
            let pack = match pack {
                Ok(Some(pack)) => pack,
                Ok(None) => break,
                Err(error) => {
                    atomic_saturating_add(
                        &self.stats.remote_blob_transfer_duration_ns,
                        transfer_duration_ns,
                    );
                    warn!(
                        "remote cache blob pack failed; falling back to individual blobs: {error}"
                    );
                    break;
                }
            };
            atomic_saturating_add(
                &self.stats.remote_blob_transfer_duration_ns,
                transfer_duration_ns,
            );
            atomic_saturating_add(&self.stats.remote_blob_pack_requests, pack.requests);
            atomic_saturating_add(
                &self.stats.remote_blob_pack_blobs,
                pack.blobs.len().try_into().unwrap_or(u64::MAX),
            );
            if pack.requested.is_empty() {
                break;
            }
            for digest in &pack.requested {
                pack_candidates.remove(digest);
            }
            let mut ingests = stream::iter(pack.blobs.into_iter().map(|(digest, source)| {
                let digest_for_result = digest.clone();
                async move {
                    (
                        digest_for_result,
                        self.ingest_packed_blob(digest, source).await,
                    )
                }
            }))
            .buffer_unordered(MAX_PREFETCH_TRANSFERS);
            while let Some((digest, result)) = ingests.next().await {
                match result {
                    Ok(path) => {
                        missing.remove(&digest);
                        verified.insert(digest, path);
                    }
                    Err(error) => warn!(
                        "remote cache packed blob ingest failed for {}: {error}",
                        digest.hash
                    ),
                }
            }
        }

        let mut transfers = stream::iter(missing.into_keys().map(|digest| {
            let digest_for_result = digest.clone();
            async move {
                (
                    digest_for_result,
                    self.fetch_remote_blob_with_limit(remote, &digest, prefetch_limit)
                        .await,
                )
            }
        }))
        .buffer_unordered(MAX_PREFETCH_TRANSFERS);
        while let Some((digest, result)) = transfers.next().await {
            match result {
                Ok(path) => {
                    verified.insert(digest, path);
                }
                Err(error) => warn!(
                    "remote cache blob prefetch failed for {}: {error}",
                    digest.hash
                ),
            }
        }
        verified
    }

    async fn ingest_packed_blob(&self, digest: CacheDigest, source: PathBuf) -> Result<PathBuf> {
        atomic_saturating_add(&self.stats.downloaded_bytes, digest.size);
        let digest_size = digest.size;
        let lock = self.write_lock(&digest);
        let _guard = lock.lock().await;
        let agent = self.clone();
        let (path, stored, cas_duration_ns) = tokio::task::spawn_blocking(move || {
            if let Some(path) = agent.find_verified_blob(&digest)? {
                return Ok::<_, eyre::Report>((path, false, 0));
            }
            let cas_started = Instant::now();
            let path = agent.cas.store_verified_file(&digest, &source)?;
            let cas_duration_ns = duration_ns(cas_started);
            agent.remember_verified_blob(&digest, &path);
            Ok((path, true, cas_duration_ns))
        })
        .await??;
        atomic_saturating_add(&self.stats.local_cas_write_duration_ns, cas_duration_ns);
        if stored {
            self.stats.stores.fetch_add(1, Ordering::Relaxed);
            atomic_saturating_add(&self.stats.stored_bytes, digest_size);
        }
        Ok(path)
    }

    fn parse_rustc_metadata(path: &Path) -> Result<RustcMetadata> {
        let bytes = fs::read(path)?;
        let metadata: RustcMetadata = serde_json::from_slice(&bytes)?;
        if metadata.version != 1 || metadata.kind != "rustc" || canonical_json(&metadata)? != bytes
        {
            bail!("remote rustc action metadata is invalid");
        }
        Ok(metadata)
    }

    fn parse_cache_directory(path: &Path) -> Result<CacheDirectory> {
        let bytes = fs::read(path)?;
        let directory: CacheDirectory = serde_json::from_slice(&bytes)?;
        if directory.version != 1 || canonical_json(&directory)? != bytes {
            bail!("remote action output directory is invalid");
        }
        Ok(directory)
    }

    #[cfg(test)]
    fn load_cache_directory(&self, digest: &CacheDigest) -> Result<CacheDirectory> {
        let path = self
            .find_verified_blob(digest)?
            .ok_or_else(|| eyre::eyre!("remote action output directory is missing"))?;
        Self::parse_cache_directory(&path)
    }

    fn validate_prefetched_action(
        action: &PrefetchedAction,
        verified: &BTreeMap<CacheDigest, PathBuf>,
        rustc_metadata: &BTreeMap<CacheDigest, RustcMetadata>,
        directories: &BTreeMap<CacheDigest, CacheDirectory>,
    ) -> Result<()> {
        if !verified.contains_key(&action.result.action) {
            bail!("remote action descriptor is missing");
        }
        if let Some(metadata) = &action.result.metadata {
            if action.adapter == "rustc" {
                let metadata = rustc_metadata
                    .get(metadata)
                    .ok_or_else(|| eyre::eyre!("remote rustc action metadata is missing"))?;
                for digest in [&metadata.stdout, &metadata.stderr] {
                    if !verified.contains_key(digest) {
                        bail!("remote rustc action diagnostic blob is missing");
                    }
                }
            } else if !verified.contains_key(metadata) {
                bail!("remote action metadata is missing");
            }
        }
        let mut pending = action
            .result
            .output_root
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut seen = BTreeMap::new();
        while let Some(digest) = pending.pop() {
            if seen.insert(digest.clone(), ()).is_some() {
                continue;
            }
            if seen.len() > MAX_PREFETCH_DIRECTORY_OBJECTS {
                bail!("remote action output tree is too large");
            }
            let directory = directories
                .get(&digest)
                .ok_or_else(|| eyre::eyre!("remote action output directory is missing"))?;
            for file in &directory.files {
                if !verified.contains_key(&file.digest) {
                    bail!("remote action output file is missing");
                }
            }
            pending.extend(
                directory
                    .directories
                    .iter()
                    .map(|directory| directory.digest.clone()),
            );
        }
        Ok(())
    }

    #[cfg(test)]
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
            self.fetch_remote_blob_with_limit(remote, &digest, Some(&self.prefetch_transfers))
                .await?;
            let directory = self.load_cache_directory(&digest)?;
            let mut transfers = stream::iter(directory.files.into_iter().map(|file| async move {
                self.fetch_remote_blob_with_limit(
                    remote,
                    &file.digest,
                    Some(&self.prefetch_transfers),
                )
                .await
                .map(|_| ())
            }))
            .buffer_unordered(MAX_PREFETCH_TRANSFERS);
            while let Some(result) = transfers.next().await {
                result?;
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
        self.fetch_remote_blob_with_limit(remote, digest, None)
            .await
    }

    async fn fetch_remote_blob_with_limit(
        &self,
        remote: &RemoteCacheClient,
        digest: &CacheDigest,
        prefetch_limit: Option<&tokio::sync::Semaphore>,
    ) -> Result<PathBuf> {
        let lock = self.write_lock(digest);
        let _guard = lock.lock().await;
        if let Some(path) = self.find_verified_blob(digest)? {
            return Ok(path);
        }
        let _prefetch_permit = match prefetch_limit {
            Some(limit) => Some(limit.acquire().await?),
            None => None,
        };
        let _permit = self.remote_transfers.acquire().await?;
        self.stats
            .remote_blob_requests
            .fetch_add(1, Ordering::Relaxed);
        let transfer_timer =
            AtomicDurationTimer::start(&self.stats.remote_blob_transfer_duration_ns);
        let temporary = remote
            .get_blob_file(digest, self.remote_staging_dir.as_path())
            .await?;
        drop(transfer_timer);
        let _cas_timer = AtomicDurationTimer::start(&self.stats.local_cas_write_duration_ns);
        let path = self.cas.store_verified_file(digest, temporary.path())?;
        self.remember_verified_blob(digest, &path);
        self.stats.stores.fetch_add(1, Ordering::Relaxed);
        self.stats
            .stored_bytes
            .fetch_add(digest.size, Ordering::Relaxed);
        self.stats
            .downloaded_bytes
            .fetch_add(digest.size, Ordering::Relaxed);
        Ok(path)
    }

    async fn respond(&self, request: AgentRequest) -> AgentResponse {
        let result = match request {
            AgentRequest::FindBlob { digest } => self.find_blob(&digest).await,
            AgentRequest::FindBlobs { digests } => self.find_blobs(digests).await,
            AgentRequest::StoreBlob { digest, source } => self.store_blob(&digest, &source).await,
            AgentRequest::FindActionResult { action } => {
                self.stats.lookups.fetch_add(1, Ordering::Relaxed);
                self.find_action_result(&action).await
            }
            AgentRequest::RecordActionHit { action, restore } => {
                self.record_action_hit(&action, restore)
            }
            AgentRequest::RecordActionVerification { matched, restore } => {
                self.record_materialization(restore);
                self.stats.verifications.fetch_add(1, Ordering::Relaxed);
                if !matched {
                    self.stats.divergences.fetch_add(1, Ordering::Relaxed);
                }
                Ok(AgentResponse::ActionVerificationRecorded)
            }
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
        if let Some(path) = self.find_verified_blob(digest)? {
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

    async fn find_blobs(&self, digests: Vec<CacheDigest>) -> Result<AgentResponse> {
        let mut paths = BTreeMap::new();
        let mut missing = Vec::new();
        for digest in &digests {
            match self.find_verified_blob(digest)? {
                Some(path) => {
                    paths.insert(digest.clone(), path);
                }
                None => {
                    missing.push(digest.clone());
                }
            }
        }

        if !missing.is_empty()
            && self.remote_mode.reads()
            && let Some(remote) = &self.remote
        {
            paths.extend(self.fetch_remote_blobs(remote, missing, None).await);
        }

        Ok(AgentResponse::Blobs {
            paths: digests
                .into_iter()
                .map(|digest| paths.get(&digest).cloned())
                .collect(),
        })
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
            if let Some(path) = self.find_verified_blob(digest)? {
                path
            } else {
                let path = self.cas.store_file(digest, source)?;
                self.remember_verified_blob(digest, &path);
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
            } else {
                self.stats
                    .uploaded_bytes
                    .fetch_add(digest.size, Ordering::Relaxed);
            }
        }
        Ok(AgentResponse::Stored { path })
    }

    fn find_verified_blob(&self, digest: &CacheDigest) -> Result<Option<PathBuf>> {
        let remembered = self.verified_blobs.lock().unwrap().get(digest).cloned();
        if let Some(path) = remembered {
            if digest.matches_file(&path).unwrap_or(false) {
                return Ok(Some(path));
            }
            self.verified_blobs.lock().unwrap().remove(digest);
        }
        let path = self.cas.find(digest)?;
        if let Some(path) = &path {
            self.remember_verified_blob(digest, path);
        }
        Ok(path)
    }

    fn remember_verified_blob(&self, digest: &CacheDigest, path: &Path) {
        self.verified_blobs
            .lock()
            .unwrap()
            .insert(digest.clone(), path.to_path_buf());
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
        match self.get_remote_action_result(remote, action).await {
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

    async fn get_remote_action_result(
        &self,
        remote: &RemoteCacheClient,
        action: &CacheDigest,
    ) -> Result<Option<RemoteActionResult>> {
        self.stats
            .remote_action_lookups
            .fetch_add(1, Ordering::Relaxed);
        let _timer = AtomicDurationTimer::start(&self.stats.remote_action_lookup_duration_ns);
        remote.get_action_result(action).await
    }

    fn record_action_hit(
        &self,
        action: &CacheDigest,
        restore: RestoreStats,
    ) -> Result<AgentResponse> {
        if self.actions.find(action)?.is_none() {
            let pending = self.pending_remote_actions.lock().unwrap().remove(action);
            if let Some(result) = pending {
                self.actions.store(&result)?;
            } else {
                bail!("cannot record a hit for a missing action result");
            }
        }
        self.record_restore(restore);
        self.stats.hits.fetch_add(1, Ordering::Relaxed);
        Ok(AgentResponse::ActionHitRecorded)
    }

    fn record_restore(&self, restore: RestoreStats) {
        self.record_materialization(restore);
        atomic_saturating_add(&self.stats.restored_output_files, restore.output_files);
        atomic_saturating_add(&self.stats.restored_output_bytes, restore.output_bytes);
    }

    fn record_materialization(&self, restore: RestoreStats) {
        atomic_saturating_add(&self.stats.materialization_duration_ns, restore.duration_ns);
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

    #[test]
    fn directory_queue_counts_only_unique_unseen_nodes() {
        let shared = CacheDigest::blake3(b"shared");
        let first = CacheDigest::blake3(b"first");
        let second = CacheDigest::blake3(b"second");
        let overflow = CacheDigest::blake3(b"overflow");
        let seen = BTreeMap::from([(shared.clone(), ())]);
        let mut pending = BTreeMap::new();

        assert!(queue_prefetch_directory(&seen, &mut pending, shared, 3));
        assert!(pending.is_empty());
        assert!(queue_prefetch_directory(
            &seen,
            &mut pending,
            first.clone(),
            3
        ));
        assert!(queue_prefetch_directory(&seen, &mut pending, first, 3));
        assert!(queue_prefetch_directory(&seen, &mut pending, second, 3));
        assert!(!queue_prefetch_directory(&seen, &mut pending, overflow, 3));
        assert_eq!(pending.len(), 2);
    }

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

    #[test]
    fn remembered_blobs_reject_same_size_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let agent = CacheAgent::new(directory.path().join("cache"), "test-version");
        let digest = CacheDigest::blake3(b"cached object");
        let path = agent.cas.store_bytes(&digest, b"cached object").unwrap();
        assert_eq!(
            agent.find_verified_blob(&digest).unwrap(),
            Some(path.clone())
        );

        std::fs::write(&path, b"broken object").unwrap();

        assert!(agent.find_verified_blob(&digest).is_err());
        assert!(!agent.verified_blobs.lock().unwrap().contains_key(&digest));
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
                    action: action.clone(),
                    restore: RestoreStats {
                        duration_ns: 7,
                        output_files: 2,
                        output_bytes: 11,
                    },
                })
                .await,
            AgentResponse::ActionHitRecorded
        ));
        assert_eq!(
            agent.stats(),
            AgentStats {
                lookups: 1,
                hits: 1,
                materialization_duration_ns: 7,
                restored_output_files: 2,
                restored_output_bytes: 11,
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
                .respond(AgentRequest::RecordActionHit {
                    action,
                    restore: RestoreStats::default(),
                })
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

        assert!(matches!(
            agent
                .respond(AgentRequest::RecordActionVerification {
                    matched: false,
                    restore: RestoreStats {
                        duration_ns: 7,
                        output_files: 2,
                        output_bytes: 11,
                    },
                })
                .await,
            AgentResponse::ActionVerificationRecorded
        ));
        assert_eq!(agent.stats().verifications, 1);
        assert_eq!(agent.stats().divergences, 1);
        assert_eq!(agent.stats().materialization_duration_ns, 7);
        assert_eq!(agent.stats().restored_output_files, 0);
        assert_eq!(agent.stats().restored_output_bytes, 0);
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
                .respond(AgentRequest::RecordActionHit {
                    action,
                    restore: RestoreStats::default(),
                })
                .await,
            AgentResponse::ActionHitRecorded
        ));
        for mock in mocks {
            mock.assert_async().await;
        }
        let stats = reader.stats();
        assert_eq!(stats.prefetch_runs, 1);
        assert_eq!(stats.prefetched_actions, 1);
        assert!(stats.remote_manifest_lookups > 0);
        assert!(stats.remote_action_lookups > 0);
        assert!(stats.remote_blob_requests > 0);
        assert!(stats.remote_manifest_lookup_duration_ns > 0);
        assert!(stats.remote_action_lookup_duration_ns > 0);
        assert!(stats.remote_blob_transfer_duration_ns > 0);
        assert!(stats.local_cas_write_duration_ns > 0);
        assert!(stats.prefetch_duration_ns > 0);
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
    async fn prefetches_complete_actions_in_directory_wave_blob_packs() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let action_bytes = b"packed action descriptor";
        let stdout_bytes = b"packed stdout";
        let stderr_bytes = b"packed stderr";
        let artifact_bytes = b"packed artifact";
        let action = CacheDigest::blake3(action_bytes);
        let stdout = CacheDigest::blake3(stdout_bytes);
        let stderr = CacheDigest::blake3(stderr_bytes);
        let artifact = CacheDigest::blake3(artifact_bytes);
        let metadata_bytes = canonical_json(&RustcMetadata {
            version: 1,
            kind: "rustc".into(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
        })
        .unwrap();
        let metadata = CacheDigest::blake3(&metadata_bytes);
        let directory_bytes = canonical_json(&serde_json::json!({
            "directories": [],
            "files": [{
                "digest": artifact,
                "executable": false,
                "mode": 420,
                "name": "artifact",
            }],
            "symlinks": [],
            "version": 1,
        }))
        .unwrap();
        let output_root = CacheDigest::blake3(&directory_bytes);
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: Some(metadata.clone()),
            output_root: Some(output_root.clone()),
            version: 1,
        };
        let action_result = server
            .mock("GET", action_path(&action).as_str())
            .with_status(200)
            .with_header("content-type", ACTION_RESULT_MEDIA_TYPE)
            .with_body(serde_json::to_vec(&result).unwrap())
            .expect(1)
            .create_async()
            .await;
        let capabilities = server
            .mock("GET", "/v1/capabilities")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":100,"max_pack_bytes":1048576}
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let mut top = vec![
            (action.clone(), action_bytes.as_slice()),
            (metadata.clone(), metadata_bytes.as_slice()),
            (output_root.clone(), directory_bytes.as_slice()),
        ];
        top.sort_by(|left, right| left.0.cmp(&right.0));
        let first_pack = server
            .mock("POST", "/v1/blobs:pack")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "digests": top.iter().map(|(digest, _)| digest).collect::<Vec<_>>()
            })))
            .with_status(200)
            .with_header("content-type", crate::BLOB_PACK_MEDIA_TYPE)
            .with_body(blob_pack_body(&top))
            .expect(1)
            .create_async()
            .await;
        let mut leaves = vec![
            (stdout.clone(), stdout_bytes.as_slice()),
            (stderr.clone(), stderr_bytes.as_slice()),
            (artifact.clone(), artifact_bytes.as_slice()),
        ];
        leaves.sort_by(|left, right| left.0.cmp(&right.0));
        let second_pack = server
            .mock("POST", "/v1/blobs:pack")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "digests": leaves.iter().map(|(digest, _)| digest).collect::<Vec<_>>()
            })))
            .with_status(200)
            .with_header("content-type", crate::BLOB_PACK_MEDIA_TYPE)
            .with_body(blob_pack_body(&leaves))
            .expect(1)
            .create_async()
            .await;
        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );

        agent
            .prefetch_action(action.clone(), "rustc".into())
            .await
            .unwrap();

        assert_eq!(agent.actions.find(&action).unwrap(), Some(result));
        let stats = agent.stats();
        assert_eq!(stats.prefetched_actions, 1);
        assert_eq!(stats.remote_blob_requests, 0);
        assert_eq!(stats.remote_blob_pack_requests, 2);
        assert_eq!(stats.remote_blob_pack_blobs, 6);
        action_result.assert_async().await;
        capabilities.assert_async().await;
        first_pack.assert_async().await;
        second_pack.assert_async().await;
    }

    #[tokio::test]
    async fn foreground_blob_batches_use_blob_packs() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let mut entries = [
            (CacheDigest::blake3(b"first"), b"first".as_slice()),
            (CacheDigest::blake3(b"second"), b"second".as_slice()),
        ];
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let requested = entries
            .iter()
            .map(|(digest, _)| digest.clone())
            .collect::<Vec<_>>();
        let response_requested = vec![
            entries[0].0.clone(),
            entries[1].0.clone(),
            entries[0].0.clone(),
        ];
        let capabilities = server
            .mock("GET", "/v1/capabilities")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":100,"max_pack_bytes":1048576}
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let pack = server
            .mock("POST", "/v1/blobs:pack")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "digests": requested.clone()
            })))
            .with_status(200)
            .with_header("content-type", crate::BLOB_PACK_MEDIA_TYPE)
            .with_body(blob_pack_body(&entries))
            .expect(1)
            .create_async()
            .await;
        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );
        let response = agent
            .respond(AgentRequest::FindBlobs {
                digests: response_requested,
            })
            .await;

        let AgentResponse::Blobs { paths } = response else {
            panic!("unexpected blob lookup response");
        };
        assert_eq!(paths.len(), 3);
        for (expected, path) in [entries[0].1, entries[1].1, entries[0].1]
            .into_iter()
            .zip(paths)
        {
            assert_eq!(fs::read(path.unwrap()).unwrap(), expected);
        }
        let stats = agent.stats();
        assert_eq!(stats.remote_blob_requests, 0);
        assert_eq!(stats.remote_blob_pack_requests, 1);
        assert_eq!(stats.remote_blob_pack_blobs, 2);
        capabilities.assert_async().await;
        pack.assert_async().await;
    }

    #[tokio::test]
    async fn preserves_successful_pack_metrics_when_a_later_chunk_falls_back() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let mut entries = [
            (CacheDigest::blake3(b"first"), b"first".as_slice()),
            (CacheDigest::blake3(b"second"), b"second".as_slice()),
        ];
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let (first_digest, first_bytes) = entries[0].clone();
        let (second_digest, second_bytes) = entries[1].clone();
        let capabilities = server
            .mock("GET", "/v1/capabilities")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":1,"max_pack_bytes":1048576}
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let first_pack = server
            .mock("POST", "/v1/blobs:pack")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "digests": [&first_digest]
            })))
            .with_status(200)
            .with_header("content-type", crate::BLOB_PACK_MEDIA_TYPE)
            .with_body(blob_pack_body(&[(first_digest.clone(), first_bytes)]))
            .expect(1)
            .create_async()
            .await;
        let failed_pack = server
            .mock("POST", "/v1/blobs:pack")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "digests": [&second_digest]
            })))
            .with_status(500)
            .expect(1)
            .create_async()
            .await;
        let fallback = server
            .mock("GET", blob_path(&second_digest).as_str())
            .with_status(200)
            .with_body(second_bytes)
            .expect(1)
            .create_async()
            .await;
        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );
        let remote = agent.remote.as_deref().unwrap();

        let verified = agent
            .fetch_remote_blobs(
                remote,
                vec![first_digest.clone(), second_digest.clone()],
                Some(&agent.prefetch_transfers),
            )
            .await;

        assert_eq!(verified.len(), 2);
        assert_eq!(fs::read(&verified[&first_digest]).unwrap(), first_bytes);
        assert_eq!(fs::read(&verified[&second_digest]).unwrap(), second_bytes);
        let stats = agent.stats();
        assert_eq!(stats.remote_blob_pack_requests, 1);
        assert_eq!(stats.remote_blob_pack_blobs, 1);
        assert_eq!(stats.remote_blob_requests, 1);
        capabilities.assert_async().await;
        first_pack.assert_async().await;
        failed_pack.assert_async().await;
        fallback.assert_async().await;
    }

    #[tokio::test]
    async fn foreground_action_lookup_does_not_wait_for_prefetch_output() {
        let directory = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let action_bytes = b"prefetched action";
        let artifact_bytes = b"prefetched artifact";
        let action = CacheDigest::blake3(action_bytes);
        let artifact = CacheDigest::blake3(artifact_bytes);
        let directory_bytes = canonical_json(&serde_json::json!({
            "directories": [],
            "files": [{
                "digest": artifact,
                "executable": false,
                "mode": 420,
                "name": "artifact",
            }],
            "symlinks": [],
            "version": 1,
        }))
        .unwrap();
        let output_root = CacheDigest::blake3(&directory_bytes);
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: None,
            output_root: Some(output_root.clone()),
            version: 1,
        };
        let action_result = server
            .mock("GET", action_path(&action).as_str())
            .with_status(200)
            .with_header("content-type", ACTION_RESULT_MEDIA_TYPE)
            .with_body(serde_json::to_vec(&result).unwrap())
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
        let output_directory = server
            .mock("GET", blob_path(&output_root).as_str())
            .with_status(200)
            .with_body(directory_bytes)
            .expect(1)
            .create_async()
            .await;
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let response_started = started.clone();
        let response_release = release.clone();
        let artifact_blob = server
            .mock("GET", blob_path(&artifact).as_str())
            .with_status(200)
            .with_chunked_body(move |writer| {
                response_started.store(true, Ordering::Release);
                while !response_release.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                std::io::Write::write_all(writer, artifact_bytes)
            })
            .expect(1)
            .create_async()
            .await;
        let agent = remote_agent(
            &server,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );
        let prefetch_agent = agent.clone();
        let prefetch_action = action.clone();
        let prefetch = tokio::spawn(async move {
            prefetch_agent
                .prefetch_action(prefetch_action, "rustc".into())
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !started.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("prefetch did not request the output blob");

        let foreground = tokio::time::timeout(
            Duration::from_millis(250),
            agent.find_action_result(&action),
        )
        .await;
        release.store(true, Ordering::Release);
        prefetch.await.unwrap().unwrap();
        let foreground = foreground.expect("foreground action lookup waited for output prefetch");

        assert!(matches!(
            foreground.unwrap(),
            AgentResponse::ActionResult {
                result: Some(found)
            } if found == result
        ));
        action_result.assert_async().await;
        action_blob.assert_async().await;
        output_directory.assert_async().await;
        artifact_blob.assert_async().await;
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
            .acquire_many(MAX_PREFETCH_TRANSFERS as u32)
            .await
            .unwrap();
        assert!(transfers.available_permits() > 0);
    }

    #[tokio::test]
    async fn prefetches_output_files_concurrently() {
        let directory = tempfile::tempdir().unwrap();
        let (responses, output_root) = output_tree_responses(8);
        let (base_url, maximum_in_flight, server) =
            delayed_blob_server(responses, Duration::from_millis(50)).await;
        let agent = remote_agent_url(
            base_url,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );

        agent
            .prefetch_output_tree(agent.remote.as_deref().unwrap(), &output_root)
            .await
            .unwrap();
        server.await.unwrap();

        assert!(maximum_in_flight.load(Ordering::Relaxed) > 1);
    }

    #[tokio::test]
    #[ignore = "local remote-cache throughput benchmark"]
    async fn benchmark_prefetch_output_tree_latency() {
        let files = std::env::var("MISE_CACHE_BENCH_FILES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(96);
        let latency = Duration::from_millis(
            std::env::var("MISE_CACHE_BENCH_LATENCY_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(100),
        );

        let directory = tempfile::tempdir().unwrap();
        let (responses, output_root) = output_tree_responses(files);
        let (base_url, maximum_in_flight, server) = delayed_blob_server(responses, latency).await;
        let agent = remote_agent_url(
            base_url,
            directory.path().join("reader"),
            RemoteCacheMode::ReadOnly,
        );
        let remote = agent.remote.as_deref().unwrap();

        let started = std::time::Instant::now();
        agent
            .prefetch_output_tree(remote, &output_root)
            .await
            .unwrap();
        let elapsed = started.elapsed();

        eprintln!(
            "prefetched {files} blobs with {} ms latency in {elapsed:?}",
            latency.as_millis()
        );
        server.await.unwrap();
        eprintln!(
            "maximum concurrent requests: {}",
            maximum_in_flight.load(Ordering::Relaxed)
        );
    }

    fn output_tree_responses(files: usize) -> (BTreeMap<String, Vec<u8>>, CacheDigest) {
        let mut entries = Vec::with_capacity(files);
        let mut responses = BTreeMap::new();
        for index in 0..files {
            let body = format!("cached artifact {index}").into_bytes();
            let digest = CacheDigest::blake3(&body);
            entries.push(serde_json::json!({
                "digest": digest,
                "executable": false,
                "mode": 420,
                "name": format!("artifact-{index}"),
            }));
            responses.insert(blob_path(&digest), body);
        }
        let directory = canonical_json(&serde_json::json!({
            "directories": [],
            "files": entries,
            "symlinks": [],
            "version": 1,
        }))
        .unwrap();
        let output_root = CacheDigest::blake3(&directory);
        responses.insert(blob_path(&output_root), directory);
        (responses, output_root)
    }

    fn blob_pack_body(entries: &[(CacheDigest, &[u8])]) -> Vec<u8> {
        let mut pack = crate::BLOB_PACK_MAGIC.to_vec();
        for (digest, bytes) in entries {
            pack.push(match digest.algorithm.as_str() {
                "blake3" => 1,
                "sha256" => 2,
                algorithm => panic!("unexpected test digest algorithm {algorithm}"),
            });
            pack.extend(hex::decode(&digest.hash).unwrap());
            pack.extend(digest.size.to_be_bytes());
            pack.extend_from_slice(bytes);
        }
        pack
    }

    async fn delayed_blob_server(
        responses: BTreeMap<String, Vec<u8>>,
        latency: Duration,
    ) -> (
        url::Url,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use std::sync::atomic::AtomicUsize;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let responses = Arc::new(responses);
        let request_count = responses.len();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let maximum_in_flight = Arc::new(AtomicUsize::new(0));
        let observed_maximum = maximum_in_flight.clone();
        let server = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            for _ in 0..request_count {
                let (mut socket, _) = listener.accept().await.unwrap();
                let responses = responses.clone();
                let in_flight = in_flight.clone();
                let maximum_in_flight = maximum_in_flight.clone();
                requests.spawn(async move {
                    let mut request = Vec::new();
                    loop {
                        let mut chunk = [0; 1024];
                        let size = socket.read(&mut chunk).await.unwrap();
                        assert!(size > 0, "client closed before sending request headers");
                        request.extend_from_slice(&chunk[..size]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&request);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap();
                    let body = responses.get(path).unwrap();
                    let active = in_flight.fetch_add(1, Ordering::Relaxed) + 1;
                    maximum_in_flight.fetch_max(active, Ordering::Relaxed);
                    tokio::time::sleep(latency).await;
                    socket
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    socket.write_all(body).await.unwrap();
                    in_flight.fetch_sub(1, Ordering::Relaxed);
                });
            }
            while requests.join_next().await.is_some() {}
        });
        (
            format!("http://{address}").parse().unwrap(),
            observed_maximum,
            server,
        )
    }

    fn remote_agent(
        server: &mockito::ServerGuard,
        cache_dir: PathBuf,
        mode: RemoteCacheMode,
    ) -> CacheAgent {
        remote_agent_url(server.url().parse().unwrap(), cache_dir, mode)
    }

    fn remote_agent_url(
        base_url: url::Url,
        cache_dir: PathBuf,
        mode: RemoteCacheMode,
    ) -> CacheAgent {
        let client = RemoteCacheClient::new(crate::RemoteCacheConfig {
            base_url,
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
