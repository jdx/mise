use crate::config::Settings;
use crate::task::Task;
#[cfg(test)]
use crate::task::TaskRustCacheConfig;
use bytesize::ByteSize;
use eyre::{Context, Result, bail};
#[cfg(any(windows, test))]
use mbx_cache_core::AGENT_PROTOCOL_VERSION;
#[cfg(unix)]
use mbx_cache_core::BlockingAgentClient;
use mbx_cache_core::{
    AgentRemoteCache, AgentRequest, AgentResponse, AgentStats, CacheAgent, CacheDigest,
    RemoteCacheClient, RemoteCacheConfig, canonical_json,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const RUSTC_SHIM_STEM: &str = "mise-cache-rustc";
const CARGO_SHIM_STEM: &str = "cargo";
pub(super) const CARGO_TARGET_ENV: &str = "MISE_CACHE_CARGO_TARGET_DIR";
const SOCKET_ENV: &str = "MISE_CACHE_SOCKET";
const REAL_CARGO_ENV: &str = "MISE_CACHE_REAL_CARGO";
const ACTION_STORE_ENV: &str = "MISE_CACHE_ACTION_STORE";
pub(super) const STAGING_ENV: &str = "MISE_CACHE_STAGING_DIR";
pub(super) const TASK_ENV: &str = "MISE_CACHE_TASK";
pub(super) const TASK_ROOT_ENV: &str = "MISE_CACHE_TASK_ROOT";
pub(super) const VERIFY_ENV: &str = "MISE_CACHE_RUST_VERIFY";
pub(super) const BUILD_ENV: &str = TASK_ENV;
pub(super) const TARGET_DIR_ENV: &str = CARGO_TARGET_ENV;
pub(super) const WORKSPACE_ROOT_ENV: &str = TASK_ROOT_ENV;
const SHARE_OUT_DIR_ENV: &str = "MISE_CACHE_SHARE_OUT_DIR";
const PREVIOUS_RUSTC_WRAPPER_ENV: &str = "MISE_CACHE_PREVIOUS_RUSTC_WRAPPER";
const VERSION: &str = env!("CARGO_PKG_VERSION");
// The ceiling of mbx's disk-scaled default. This keeps an embedded client from
// pruning more aggressively than mbx on the same machine; mbx applies any
// tighter configured policy on its next coordinated sweep.
const SHARED_STORE_MAX_BYTES: u64 = 100 * 1024 * 1024 * 1024;
const SHARED_STORE_GC_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub(crate) struct CacheSessionEnvironment {
    socket: String,
    rustc_shim: String,
    shim_dir: String,
    staging: String,
    store: String,
    agent: CacheAgent,
}

impl CacheSessionEnvironment {
    /// Adds the Rust action-cache environment for an enabled task.
    pub(crate) async fn apply(
        &self,
        task: &Task,
        task_root: &Path,
        environment: &mut BTreeMap<String, String>,
    ) -> Option<TaskActionRun> {
        if !task.rust_cache.as_ref().is_some_and(|cache| cache.enabled) {
            return None;
        }
        let task_identity = task_action_identity(task);
        let (protocol_task, action_run) = match self.agent.begin_task(&task_identity).await {
            Ok(run) => (
                run.clone(),
                Some(TaskActionRun {
                    run,
                    agent: self.agent.clone(),
                }),
            ),
            Err(error) => {
                warn!("task {} action manifest was not loaded: {error}", task.name);
                (task_identity, None)
            }
        };
        environment.insert(SOCKET_ENV.into(), self.socket.clone());
        environment.insert(STAGING_ENV.into(), self.staging.clone());
        environment.insert(ACTION_STORE_ENV.into(), self.store.clone());
        environment.insert(TASK_ENV.into(), protocol_task);
        environment.insert(
            TASK_ROOT_ENV.into(),
            task_root.to_string_lossy().into_owned(),
        );
        if let Some(target) = environment.get("CARGO_TARGET_DIR").cloned() {
            environment.insert(CARGO_TARGET_ENV.into(), target);
        }
        if task.rust_cache.as_ref().is_some_and(|cache| cache.verify) {
            environment.insert(VERIFY_ENV.into(), "1".into());
        } else {
            environment.remove(VERIFY_ENV);
        }
        environment.insert(SHARE_OUT_DIR_ENV.into(), "0".into());
        if let Some(previous) = environment.insert("RUSTC_WRAPPER".into(), self.rustc_shim.clone())
            && previous != self.rustc_shim
        {
            environment.insert(PREVIOUS_RUSTC_WRAPPER_ENV.into(), previous);
        }
        environment.insert("CARGO_INCREMENTAL".into(), "0".into());
        environment.remove(REAL_CARGO_ENV);
        if let Some(path) = environment.get(crate::env::PATH_KEY.as_str()).cloned()
            && let Ok(cargo) = which::which_in("cargo", Some(&path), task_root)
        {
            environment.insert(REAL_CARGO_ENV.into(), cargo.to_string_lossy().into_owned());
            let paths = std::iter::once(PathBuf::from(&self.shim_dir))
                .chain(std::env::split_paths(OsStr::new(&path)));
            if let Ok(path) = std::env::join_paths(paths) {
                environment.insert(
                    crate::env::PATH_KEY.to_string(),
                    path.to_string_lossy().into_owned(),
                );
            }
        }
        action_run
    }

    pub(crate) fn sandbox_paths(&self) -> [PathBuf; 5] {
        [
            PathBuf::from(&self.rustc_shim),
            PathBuf::from(&self.shim_dir).join(if cfg!(windows) {
                format!("{CARGO_SHIM_STEM}.exe")
            } else {
                CARGO_SHIM_STEM.into()
            }),
            PathBuf::from(&self.socket),
            PathBuf::from(&self.staging),
            PathBuf::from(&self.store),
        ]
    }
}

pub(crate) struct TaskActionRun {
    run: String,
    agent: CacheAgent,
}

impl TaskActionRun {
    pub(crate) async fn commit(self) -> Result<()> {
        self.agent.commit_task(&self.run).await
    }
}

#[derive(Serialize)]
struct TaskActionIdentity<'a> {
    version: u8,
    name: &'a str,
    phase: crate::task::TaskRunPhase,
    run: &'a [crate::task::RunEntry],
    args: &'a [String],
    shell: &'a Option<String>,
    dir: &'a Option<String>,
    source: String,
}

fn task_action_identity(task: &Task) -> String {
    let source = task
        .config_root
        .as_deref()
        .and_then(|root| task.config_source.strip_prefix(root).ok())
        .map(Path::to_path_buf)
        .or_else(|| task.config_source.file_name().map(PathBuf::from))
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let material = TaskActionIdentity {
        version: 1,
        name: &task.name,
        phase: task.run_phase,
        run: task.run(),
        args: &task.args,
        shell: &task.shell,
        dir: &task.dir,
        source,
    };
    let bytes = canonical_json(&material).expect("task action identity must serialize");
    CacheDigest::blake3(&bytes).hash
}

pub(crate) struct CacheSession {
    environment: CacheSessionEnvironment,
    agent: CacheAgent,
    started: Instant,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    server: Mutex<Option<JoinHandle<Result<()>>>>,
}

impl CacheSession {
    pub(crate) async fn start(session_dir: &Path, cache_dir: PathBuf) -> Result<Self> {
        let rustc_shim = install_session_shim(session_dir, RUSTC_SHIM_STEM)?;
        let _cargo_shim = install_session_shim(session_dir, CARGO_SHIM_STEM)?;
        let staging = session_dir.join("staging");
        std::fs::create_dir(&staging)?;
        let agent = if let Some(remote) = action_remote_cache(&cache_dir)? {
            CacheAgent::new_remote(cache_dir.clone(), VERSION, remote)
        } else {
            CacheAgent::new(cache_dir.clone(), VERSION)
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (socket, server) = spawn_server(session_dir, agent.clone(), shutdown_rx).await?;
        Ok(Self {
            environment: CacheSessionEnvironment {
                socket,
                rustc_shim: rustc_shim.to_string_lossy().into_owned(),
                shim_dir: session_dir.to_string_lossy().into_owned(),
                staging: staging.to_string_lossy().into_owned(),
                store: cache_dir.to_string_lossy().into_owned(),
                agent: agent.clone(),
            },
            agent,
            started: Instant::now(),
            shutdown: Mutex::new(Some(shutdown_tx)),
            server: Mutex::new(Some(server)),
        })
    }

    pub(crate) fn environment(&self) -> CacheSessionEnvironment {
        self.environment.clone()
    }

    pub(crate) async fn finish(&self) -> Result<AgentStats> {
        if let Some(shutdown) = self.shutdown.lock().unwrap().take() {
            let _ = shutdown.send(());
        }
        let server = self.server.lock().unwrap().take();
        if let Some(server) = server {
            server.await??;
        }
        self.agent.cancel_prefetches().await;
        let mut stats = self.agent.stats();
        stats.session_duration_ns = duration_ns(self.started.elapsed());
        if let Err(error) = mbx_cache_store::sweep_if_due(
            Path::new(&self.environment.store),
            SHARED_STORE_MAX_BYTES,
            SHARED_STORE_GC_INTERVAL,
        ) {
            warn!("shared mbx action-store GC failed: {error:#}");
        }
        Ok(stats)
    }
}

fn action_remote_cache(cache_dir: &Path) -> Result<Option<AgentRemoteCache>> {
    let settings = Settings::get();
    let Some(base_url) = settings.task.cache.remote_url.clone() else {
        return Ok(None);
    };
    let namespace = settings
        .task
        .cache
        .remote_namespace
        .as_deref()
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
        .ok_or_else(|| {
            eyre::eyre!("task.cache.remote_namespace is required when task.cache.remote_url is set")
        })?
        .to_string();
    let client = RemoteCacheClient::new(RemoteCacheConfig {
        base_url: base_url.parse().wrap_err("invalid task.cache.remote_url")?,
        namespace,
        token: settings.task.cache.remote_token.clone(),
        token_file: settings.task.cache.remote_token_file.clone(),
        oidc_audience: settings.task.cache.remote_oidc_audience.clone(),
        connect_timeout: settings.http_timeout(),
        read_timeout: settings.http_timeout(),
        download_timeout: settings.http_download_timeout(),
        retries: settings.http_retries(),
    })?;
    let Some(mode) = crate::cache::effective_remote_cache_mode(settings.task.cache.remote_mode)
    else {
        return Ok(None);
    };
    Ok(Some(AgentRemoteCache {
        client,
        mode,
        staging_dir: cache_dir.join("remote"),
    }))
}

impl Drop for CacheSession {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.get_mut().unwrap().take() {
            let _ = shutdown.send(());
        }
        if let Some(server) = self.server.get_mut().unwrap().take() {
            server.abort();
        }
    }
}

#[derive(Serialize)]
struct ActionCacheStatsReport {
    version: u8,
    session_duration_ns: u64,
    lookups: u64,
    hits: u64,
    misses: u64,
    compiler_invocations_avoided: u64,
    verifications: u64,
    divergences: u64,
    prefetched_actions: u64,
    prefetch_runs: u64,
    downloaded_bytes: u64,
    uploaded_bytes: u64,
    stored_bytes: u64,
    restored_output_files: u64,
    restored_output_bytes: u64,
    remote_manifest_lookups: u64,
    remote_action_lookups: u64,
    remote_blob_requests: u64,
    remote_blob_pack_requests: u64,
    remote_blob_pack_blobs: u64,
    remote_manifest_lookup_duration_ns: u64,
    remote_action_lookup_duration_ns: u64,
    remote_blob_transfer_duration_ns: u64,
    local_cas_write_duration_ns: u64,
    prefetch_duration_ns: u64,
    materialization_duration_ns: u64,
}

impl From<&AgentStats> for ActionCacheStatsReport {
    fn from(stats: &AgentStats) -> Self {
        Self {
            version: 1,
            session_duration_ns: stats.session_duration_ns,
            lookups: stats.lookups,
            hits: stats.hits,
            misses: cache_misses(stats),
            compiler_invocations_avoided: stats.hits,
            verifications: stats.verifications,
            divergences: stats.divergences,
            prefetched_actions: stats.prefetched_actions,
            prefetch_runs: stats.prefetch_runs,
            downloaded_bytes: stats.downloaded_bytes,
            uploaded_bytes: stats.uploaded_bytes,
            stored_bytes: stats.stored_bytes,
            restored_output_files: stats.restored_output_files,
            restored_output_bytes: stats.restored_output_bytes,
            remote_manifest_lookups: stats.remote_manifest_lookups,
            remote_action_lookups: stats.remote_action_lookups,
            remote_blob_requests: stats.remote_blob_requests,
            remote_blob_pack_requests: stats.remote_blob_pack_requests,
            remote_blob_pack_blobs: stats.remote_blob_pack_blobs,
            remote_manifest_lookup_duration_ns: stats.remote_manifest_lookup_duration_ns,
            remote_action_lookup_duration_ns: stats.remote_action_lookup_duration_ns,
            remote_blob_transfer_duration_ns: stats.remote_blob_transfer_duration_ns,
            local_cas_write_duration_ns: stats.local_cas_write_duration_ns,
            prefetch_duration_ns: stats.prefetch_duration_ns,
            materialization_duration_ns: stats.materialization_duration_ns,
        }
    }
}

pub(crate) fn display_stats(stats: AgentStats) {
    if let Some(path) = &Settings::get().task.cache.stats_report
        && let Err(error) = write_stats_report(path, &stats)
    {
        warn!(
            "action cache could not write its statistics report to {}: {error}",
            path.display()
        );
    }
    if stats.lookups == 0
        && stats.stores == 0
        && stats.verifications == 0
        && stats.downloaded_bytes == 0
        && stats.uploaded_bytes == 0
    {
        return;
    }
    safe_eprintln!(
        "Action cache: {} hits, {} misses, {} prefetched; {} downloaded, {} uploaded, {} stored locally",
        stats.hits,
        cache_misses(&stats),
        stats.prefetched_actions,
        ByteSize::b(stats.downloaded_bytes).display().iec(),
        ByteSize::b(stats.uploaded_bytes).display().iec(),
        ByteSize::b(stats.stored_bytes).display().iec(),
    );
    let remote_lookup_duration_ns = stats
        .remote_manifest_lookup_duration_ns
        .saturating_add(stats.remote_action_lookup_duration_ns);
    safe_eprintln!(
        "Action cache timing: {} session, {} prefetch; cumulative {} remote lookup, {} blob transfer, {} CAS write, {} materialization",
        format_duration(stats.session_duration_ns),
        format_duration(stats.prefetch_duration_ns),
        format_duration(remote_lookup_duration_ns),
        format_duration(stats.remote_blob_transfer_duration_ns),
        format_duration(stats.local_cas_write_duration_ns),
        format_duration(stats.materialization_duration_ns),
    );
    if stats.verifications > 0 {
        safe_eprintln!(
            "Action cache qualification: {} verified, {} diverged",
            stats.verifications,
            stats.divergences,
        );
    }
}

fn write_stats_report(path: &Path, stats: &AgentStats) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        crate::file::create_dir_all(parent)?;
    }
    let mut report = serde_json::to_vec_pretty(&ActionCacheStatsReport::from(stats))?;
    report.push(b'\n');
    crate::file::write_atomic(path, report)
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn format_duration(nanoseconds: u64) -> String {
    crate::ui::time::format_duration(Duration::from_nanos(nanoseconds))
}

fn cache_misses(stats: &AgentStats) -> u64 {
    stats
        .lookups
        .saturating_sub(stats.hits)
        .saturating_sub(stats.verifications)
}

fn install_session_shim(session_dir: &Path, stem: &str) -> Result<PathBuf> {
    let executable =
        std::env::current_exe().wrap_err("failed to locate the running mise binary")?;
    let filename = if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.into()
    };
    let shim = session_dir.join(filename);
    if let Err(link_error) = std::fs::hard_link(&executable, &shim) {
        std::fs::copy(&executable, &shim).wrap_err_with(|| {
            format!("failed to install the action-cache shim by hard link ({link_error}) or copy")
        })?;
    }
    Ok(shim)
}

#[cfg(unix)]
async fn spawn_server(
    session_dir: &Path,
    agent: CacheAgent,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(String, JoinHandle<Result<()>>)> {
    use std::os::unix::fs::PermissionsExt as _;

    let socket = session_dir.join("cache-agent.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let endpoint = socket.to_string_lossy().into_owned();
    let server = tokio::spawn(async move {
        let _cleanup = SocketCleanup(socket);
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let agent = agent.clone();
                    tokio::spawn(async move {
                        if let Err(error) = agent.handle_connection(stream).await {
                            debug!("action-cache agent connection failed: {error}");
                        }
                    });
                }
                _ = &mut shutdown => return Ok(()),
            }
        }
    });
    Ok((endpoint, server))
}

#[cfg(unix)]
struct SocketCleanup(PathBuf);

#[cfg(unix)]
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
async fn spawn_server(
    _session_dir: &Path,
    agent: CacheAgent,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(String, JoinHandle<Result<()>>)> {
    let endpoint = format!(
        r"\\.\pipe\mise-cache-{}-{}",
        std::process::id(),
        crate::rand::random_string(12)
    );
    let first_server = create_named_pipe(&endpoint, true)?;
    let server_endpoint = endpoint.clone();
    let server = tokio::spawn(async move {
        let mut next_server = Some(first_server);
        loop {
            let pipe = next_server
                .take()
                .expect("the next named-pipe server is always prepared");
            tokio::select! {
                connected = pipe.connect() => {
                    connected?;
                    next_server = Some(create_named_pipe(&server_endpoint, false)?);
                    let agent = agent.clone();
                    tokio::spawn(async move {
                        if let Err(error) = agent.handle_connection(pipe).await {
                            debug!("action-cache agent connection failed: {error}");
                        }
                    });
                }
                _ = &mut shutdown => return Ok(()),
            }
        }
    });
    Ok((endpoint, server))
}

#[cfg(windows)]
fn create_named_pipe(
    endpoint: &str,
    first_pipe_instance: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use std::mem::size_of;
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

    let security = CurrentUserSecurityDescriptor::new()?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
    options.first_pipe_instance(first_pipe_instance);
    // SAFETY: `attributes` and its owned security descriptor remain valid for
    // the duration of CreateNamedPipeW, and the handle is not inheritable.
    unsafe {
        options
            .create_with_security_attributes_raw(endpoint, (&raw mut attributes).cast())
            .wrap_err("failed to create the current-user-only action-cache pipe")
    }
}

#[cfg(windows)]
struct CurrentUserSecurityDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl CurrentUserSecurityDescriptor {
    fn new() -> Result<Self> {
        use std::mem::size_of;
        use std::ptr::null_mut;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token: HANDLE = null_mut();
        // SAFETY: `token` is a valid out pointer and the process pseudo-handle
        // is always valid in the calling process.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
            return Err(std::io::Error::last_os_error())
                .wrap_err("failed to open the current process token");
        }
        let token = OwnedWindowsHandle(token);

        let mut required = 0;
        // The first call intentionally obtains the required buffer size.
        // SAFETY: a null information pointer is required for this size query.
        unsafe {
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &raw mut required);
        }
        if required == 0 {
            return Err(std::io::Error::last_os_error())
                .wrap_err("failed to size the current process user token");
        }
        let word_count = (required as usize).div_ceil(size_of::<usize>());
        let mut token_information = vec![0usize; word_count];
        // SAFETY: the aligned buffer is at least `required` bytes long and the
        // API initializes it as TOKEN_USER on success.
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                token_information.as_mut_ptr().cast(),
                required,
                &raw mut required,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .wrap_err("failed to read the current process user token");
        }
        // SAFETY: GetTokenInformation successfully initialized the aligned
        // buffer as TOKEN_USER and its SID remains owned by that buffer.
        let user_sid = unsafe {
            (*(token_information.as_ptr().cast::<TOKEN_USER>()))
                .User
                .Sid
        };
        let mut sid_string = null_mut();
        // SAFETY: `user_sid` points into the live token-information buffer and
        // `sid_string` is a valid out pointer.
        if unsafe { ConvertSidToStringSidW(user_sid, &raw mut sid_string) } == 0 {
            return Err(std::io::Error::last_os_error())
                .wrap_err("failed to format the current process user SID");
        }
        let sid_string = LocalWindowsAllocation(sid_string.cast());
        // SAFETY: ConvertSidToStringSidW returned a valid NUL-terminated string
        // whose allocation remains live through `sid_string`.
        let sid = unsafe { nul_terminated_wide(sid_string.0.cast()) };

        let mut sddl: Vec<u16> = "D:P(A;;GA;;;".encode_utf16().collect();
        sddl.extend_from_slice(sid);
        sddl.extend([')' as u16, 0]);
        let mut descriptor = null_mut();
        // SAFETY: the SDDL is NUL-terminated, `descriptor` is a valid out
        // pointer, and the returned allocation is owned by LocalFree.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .wrap_err("failed to restrict the action-cache pipe to the current user");
        }
        drop(token);
        drop(sid_string);
        debug_assert!(!descriptor.is_null());
        Ok(Self(descriptor))
    }
}

#[cfg(windows)]
impl Drop for CurrentUserSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: this allocation came from
        // ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0.cast());
        }
    }
}

#[cfg(windows)]
struct OwnedWindowsHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for OwnedWindowsHandle {
    fn drop(&mut self) {
        // SAFETY: this handle came from OpenProcessToken and is owned here.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
struct LocalWindowsAllocation(windows_sys::Win32::Foundation::HLOCAL);

#[cfg(windows)]
impl Drop for LocalWindowsAllocation {
    fn drop(&mut self) {
        // SAFETY: this allocation came from a Win32 API documented to use
        // LocalAlloc and has not previously been freed.
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

#[cfg(windows)]
unsafe fn nul_terminated_wide<'a>(value: *const u16) -> &'a [u16] {
    let mut length = 0;
    // SAFETY: the caller guarantees that `value` points to a valid
    // NUL-terminated UTF-16 string.
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: the scan above established the initialized string length.
    unsafe { std::slice::from_raw_parts(value, length) }
}

pub(crate) fn is_rustc_shim() -> bool {
    std::env::args_os()
        .next()
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_stem)
        .is_some_and(|stem| stem == OsStr::new(RUSTC_SHIM_STEM))
}

pub(crate) fn is_cargo_shim() -> bool {
    std::env::args_os()
        .next()
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_stem)
        .is_some_and(|stem| stem == OsStr::new(CARGO_SHIM_STEM))
}

pub(crate) fn run_cargo_shim() -> ExitCode {
    let Some(cargo) = std::env::var_os(REAL_CARGO_ENV) else {
        eprintln!("mise action-cache Cargo shim could not resolve the real Cargo executable");
        return ExitCode::from(1);
    };
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let strings = arguments
        .iter()
        .map(|argument| argument.clone().into_string())
        .collect::<std::result::Result<Vec<_>, _>>();
    let working_dir = std::env::current_dir();
    let invocation = strings
        .ok()
        .zip(working_dir.ok())
        .map(|(arguments, working_dir)| {
            mbx_cache_cargo::resolve(
                cargo.as_os_str(),
                &arguments,
                &working_dir,
                std::env::var_os("CARGO_TARGET_DIR"),
            )
        });

    let mut run = None;
    if let Some(invocation) = &invocation {
        if let Some(store) = std::env::var_os(ACTION_STORE_ENV)
            && let Err(error) = mbx_cache_store::record_checkout(
                Path::new(&store),
                &invocation.build_identity,
                &invocation.workspace_root,
                &invocation.target_dir,
            )
        {
            eprintln!("mise action-cache warning: checkout was not recorded: {error:#}");
        }
        match request_agent(&[AgentRequest::BeginTask {
            task: invocation.build_identity.clone(),
        }]) {
            Ok(responses) => match responses.into_iter().next() {
                Some(AgentResponse::TaskBegun { run: action_run }) => {
                    run = Some(action_run);
                }
                Some(AgentResponse::Error { message }) => {
                    eprintln!(
                        "mise action-cache warning: Cargo manifest was not loaded: {message}"
                    );
                }
                _ => eprintln!(
                    "mise action-cache warning: agent returned an unexpected begin-task response"
                ),
            },
            Err(error) => {
                eprintln!("mise action-cache warning: Cargo manifest was not loaded: {error:#}");
            }
        }
    }

    let mut command = Command::new(cargo);
    command.args(&arguments);
    if let (Some(invocation), Some(action_run)) = (&invocation, &run) {
        command.env(TASK_ENV, action_run);
        command.env(TASK_ROOT_ENV, &invocation.workspace_root);
        command.env(CARGO_TARGET_ENV, &invocation.target_dir);
    }
    let status = match command.status() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("mise action-cache Cargo shim failed to execute Cargo: {error}");
            return ExitCode::from(1);
        }
    };

    if let Some(action_run) = run
        && let Err(error) =
            request_agent(&[AgentRequest::CommitTask { run: action_run }]).and_then(|responses| {
                match responses.into_iter().next() {
                    Some(AgentResponse::TaskCommitted) => Ok(()),
                    Some(AgentResponse::Error { message }) => bail!(message),
                    _ => bail!("agent returned an unexpected commit-task response"),
                }
            })
    {
        eprintln!("mise action-cache warning: Cargo manifest was not committed: {error:#}");
    }
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(1), ExitCode::from)
}

/// Ultra-early argv0 path used by Cargo's `RUSTC_WRAPPER` integration.
///
/// This runs before mise creates a Tokio runtime, installs logging, or discovers
/// configuration. Cacheable invocations restore from or publish through the
/// task-scoped agent; unsupported invocations remain transparent compiler calls.
pub(crate) fn run_rustc_shim() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(rustc) = arguments.next() else {
        eprintln!("mise action-cache shim expected the rustc executable as its first argument");
        return ExitCode::from(1);
    };
    let arguments = arguments.collect::<Vec<_>>();
    // Cargo and toolchain discovery can invoke the wrapper without a source
    // argument. Avoid initializing the cache path for these compiler probes.
    if arguments.is_empty() {
        return run_transparent_rustc(rustc, arguments);
    }
    if std::env::var_os(PREVIOUS_RUSTC_WRAPPER_ENV).is_none() {
        match crate::cache::rustc::compile(&rustc, &arguments) {
            Ok(exit_code) => return exit_code,
            Err(error) => {
                record_bypass(&error);
                #[cfg(debug_assertions)]
                eprintln!("mise rustc cache bypassed: {error:#}");
            }
        }
    }

    run_transparent_rustc(rustc, arguments)
}

pub(super) fn verify_requested() -> bool {
    std::env::var_os(VERIFY_ENV).is_some_and(|value| !value.is_empty() && value != "0")
}

pub(super) fn share_out_dir_requested() -> bool {
    std::env::var_os(SHARE_OUT_DIR_ENV).is_some_and(|value| !value.is_empty() && value != "0")
}

fn record_bypass(error: &eyre::Report) {
    let kind = error
        .downcast_ref::<mbx_cache_rustc::BypassReason>()
        .map_or("other", mbx_cache_rustc::BypassReason::kind);
    let _ = request_agent(&[AgentRequest::RecordBypass { kind: kind.into() }]);
}

pub(super) fn record_unconsulted() {
    let _ = request_agent(&[AgentRequest::RecordUnconsulted]);
}

pub(super) fn record_compiler_invocation(
    outcome: &str,
    crate_name: Option<&str>,
    duration_ns: u64,
) {
    let _ = request_agent(&[AgentRequest::RecordCompilerInvocation {
        outcome: outcome.into(),
        crate_name: crate_name.map(str::to_string),
        duration_ns,
    }]);
}

fn run_transparent_rustc(rustc: OsString, arguments: Vec<OsString>) -> ExitCode {
    let mut command = if let Some(wrapper) = std::env::var_os(PREVIOUS_RUSTC_WRAPPER_ENV) {
        let mut command = Command::new(wrapper);
        command.arg(&rustc);
        command
    } else {
        Command::new(&rustc)
    };
    command.args(arguments);
    command.env_remove(PREVIOUS_RUSTC_WRAPPER_ENV);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let error = command.exec();
        eprintln!("mise action-cache shim failed to execute rustc: {error}");
        ExitCode::from(1)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;

        match command.spawn().and_then(|mut child| {
            child.wait()?;
            let mut exit_code = 1;
            // SAFETY: the child owns a valid process handle until it is
            // dropped, and `exit_code` is a valid out pointer.
            if unsafe { GetExitCodeProcess(child.as_raw_handle().cast(), &raw mut exit_code) } == 0
            {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(exit_code)
            }
        }) {
            Ok(exit_code) => {
                // SAFETY: This process is only a transparent compiler wrapper.
                // ExitProcess is required to preserve Windows exception codes,
                // which cannot be represented by stable Rust's ExitCode API.
                unsafe { windows_sys::Win32::System::Threading::ExitProcess(exit_code) }
            }
            Err(error) => {
                eprintln!("mise action-cache shim failed to execute rustc: {error}");
                ExitCode::from(1)
            }
        }
    }
}

pub(super) fn request_agent(requests: &[AgentRequest]) -> Result<Vec<AgentResponse>> {
    let socket =
        std::env::var_os(SOCKET_ENV).ok_or_else(|| eyre::eyre!("{SOCKET_ENV} is not set"))?;
    request_agent_at(&socket, requests)
}

#[cfg(unix)]
fn request_agent_at(socket: &OsString, requests: &[AgentRequest]) -> Result<Vec<AgentResponse>> {
    let stream = std::os::unix::net::UnixStream::connect(Path::new(socket))
        .wrap_err("failed to connect to the action-cache session")?;
    let mut client = BlockingAgentClient::connect(stream, VERSION)?;
    requests
        .iter()
        .cloned()
        .map(|request| client.request(request))
        .collect()
}

#[cfg(windows)]
fn request_agent_at(socket: &OsString, requests: &[AgentRequest]) -> Result<Vec<AgentResponse>> {
    let endpoint = socket.to_string_lossy().into_owned();
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?
        .block_on(async move {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(&endpoint)
                .wrap_err("failed to connect to the action-cache session")?;
            let request = AgentRequest::Hello {
                protocol: AGENT_PROTOCOL_VERSION,
                client_version: VERSION.into(),
            };
            let mut encoded = serde_json::to_vec(&request)?;
            encoded.push(b'\n');
            stream.write_all(&encoded).await?;
            stream.flush().await?;
            let mut response = String::new();
            tokio::io::BufReader::new(&mut stream)
                .read_line(&mut response)
                .await?;
            validate_handshake_response(&response)?;
            let mut responses = Vec::with_capacity(requests.len());
            for request in requests {
                let mut encoded = serde_json::to_vec(request)?;
                encoded.push(b'\n');
                stream.write_all(&encoded).await?;
                stream.flush().await?;
                let mut response = String::new();
                tokio::io::BufReader::new(&mut stream)
                    .read_line(&mut response)
                    .await?;
                responses.push(serde_json::from_str(&response)?);
            }
            Ok(responses)
        })
}

#[cfg(any(windows, test))]
fn validate_handshake_response(response: &str) -> Result<()> {
    match serde_json::from_str(response)? {
        AgentResponse::Hello {
            protocol,
            agent_version,
        } if protocol == AGENT_PROTOCOL_VERSION && agent_version == VERSION => Ok(()),
        AgentResponse::Error { message } => bail!(message),
        _ => bail!("action-cache agent returned an incompatible handshake"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn session_environment_is_scoped_to_selected_adapters() {
        let cache = tempfile::tempdir().unwrap();
        let environment = CacheSessionEnvironment {
            socket: "socket".into(),
            rustc_shim: "shim".into(),
            shim_dir: "shim-dir".into(),
            staging: "staging".into(),
            store: cache.path().to_string_lossy().into_owned(),
            agent: CacheAgent::new(cache.path(), VERSION),
        };
        let mut task = Task::default();
        let mut values = BTreeMap::from([
            ("CARGO_TARGET_DIR".into(), "target".into()),
            ("RUSTC_WRAPPER".into(), "existing".into()),
            (
                crate::env::PATH_KEY.to_string(),
                std::env::var(crate::env::PATH_KEY.as_str()).unwrap(),
            ),
        ]);
        let task_directory = tempfile::tempdir().unwrap();
        let task_root = task_directory.path();
        let run = environment.apply(&task, task_root, &mut values).await;
        assert!(run.is_none());
        assert_eq!(values.get("RUSTC_WRAPPER").unwrap(), "existing");

        task.rust_cache = Some(TaskRustCacheConfig {
            enabled: false,
            ..TaskRustCacheConfig::default()
        });
        let run = environment.apply(&task, task_root, &mut values).await;
        assert!(run.is_none());
        assert_eq!(values.get("RUSTC_WRAPPER").unwrap(), "existing");

        task.rust_cache = Some(TaskRustCacheConfig {
            verify: true,
            ..TaskRustCacheConfig::default()
        });
        let run = environment.apply(&task, task_root, &mut values).await;
        assert!(run.is_some());
        assert_eq!(values.get(SOCKET_ENV).unwrap(), "socket");
        assert_eq!(values.get(STAGING_ENV).unwrap(), "staging");
        assert_eq!(values.get(TASK_ENV).unwrap().len(), 64);
        assert_eq!(values.get(CARGO_TARGET_ENV).unwrap(), "target");
        assert_eq!(
            values.get(TASK_ROOT_ENV).unwrap(),
            &task_root.to_string_lossy()
        );
        assert_eq!(values.get("RUSTC_WRAPPER").unwrap(), "shim");
        assert_eq!(values.get(PREVIOUS_RUSTC_WRAPPER_ENV).unwrap(), "existing");
        assert_eq!(values.get("CARGO_INCREMENTAL").unwrap(), "0");
        assert_eq!(values.get(VERIFY_ENV).unwrap(), "1");
        assert_eq!(
            values.get(ACTION_STORE_ENV).unwrap(),
            &cache.path().to_string_lossy()
        );
        assert_ne!(
            values.get(REAL_CARGO_ENV),
            values.get(crate::env::PATH_KEY.as_str())
        );
        assert_eq!(
            std::env::split_paths(OsStr::new(
                values.get(crate::env::PATH_KEY.as_str()).unwrap()
            ))
            .next()
            .unwrap(),
            PathBuf::from("shim-dir")
        );
    }

    #[test]
    fn handshake_rejects_version_skew() {
        let response = serde_json::to_string(&AgentResponse::Hello {
            protocol: AGENT_PROTOCOL_VERSION,
            agent_version: "another-version".into(),
        })
        .unwrap();
        assert!(validate_handshake_response(&response).is_err());
    }

    #[test]
    fn qualification_results_are_not_reported_as_misses() {
        let mut stats = AgentStats::default();
        stats.lookups = 5;
        stats.hits = 2;
        stats.verifications = 2;
        assert_eq!(cache_misses(&stats), 1);
    }

    #[test]
    fn writes_versioned_action_cache_stats_report() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("stats.json");
        let mut stats = AgentStats::default();
        stats.session_duration_ns = 42;
        stats.lookups = 5;
        stats.hits = 2;
        stats.verifications = 1;
        stats.prefetched_actions = 3;
        stats.downloaded_bytes = 1024;
        stats.restored_output_files = 7;
        stats.restored_output_bytes = 2048;
        stats.remote_blob_requests = 4;
        stats.remote_blob_pack_requests = 2;
        stats.remote_blob_pack_blobs = 100;
        stats.materialization_duration_ns = 9;

        write_stats_report(&path, &stats).unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();

        assert_eq!(report["version"], 1);
        assert_eq!(report["session_duration_ns"], 42);
        assert_eq!(report["hits"], 2);
        assert_eq!(report["misses"], 2);
        assert_eq!(report["compiler_invocations_avoided"], 2);
        assert_eq!(report["prefetched_actions"], 3);
        assert_eq!(report["downloaded_bytes"], 1024);
        assert_eq!(report["restored_output_files"], 7);
        assert_eq!(report["restored_output_bytes"], 2048);
        assert_eq!(report["remote_blob_requests"], 4);
        assert_eq!(report["remote_blob_pack_requests"], 2);
        assert_eq!(report["remote_blob_pack_blobs"], 100);
        assert_eq!(report["materialization_duration_ns"], 9);
    }
}
