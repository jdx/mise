use crate::task::Task;
#[cfg(test)]
use crate::task::TaskRustCacheConfig;
use bytesize::ByteSize;
use eyre::{Context, Result, bail};
use mise_cache_core::{
    AGENT_PROTOCOL_VERSION, AgentRequest, AgentResponse, AgentStats, CacheAgent, CacheDigest,
    canonical_json,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const RUSTC_SHIM_STEM: &str = "mise-cache-rustc";
const SOCKET_ENV: &str = "MISE_CACHE_SOCKET";
pub(super) const STAGING_ENV: &str = "MISE_CACHE_STAGING_DIR";
pub(super) const TASK_ENV: &str = "MISE_CACHE_TASK";
const PREVIOUS_RUSTC_WRAPPER_ENV: &str = "MISE_CACHE_PREVIOUS_RUSTC_WRAPPER";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub(crate) struct CacheSessionEnvironment {
    socket: String,
    rustc_shim: String,
    staging: String,
    agent: CacheAgent,
}

impl CacheSessionEnvironment {
    pub(crate) fn apply(
        &self,
        task: &Task,
        environment: &mut BTreeMap<String, String>,
    ) -> Option<TaskActionRun> {
        if !task.rust_cache.as_ref().is_some_and(|cache| cache.enabled) {
            return None;
        }
        let task_identity = task_action_identity(task);
        let (protocol_task, action_run) = match self.agent.begin_task(&task_identity) {
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
        environment.insert(TASK_ENV.into(), protocol_task);
        if let Some(previous) = environment.insert("RUSTC_WRAPPER".into(), self.rustc_shim.clone())
            && previous != self.rustc_shim
        {
            environment.insert(PREVIOUS_RUSTC_WRAPPER_ENV.into(), previous);
        }
        environment.insert("CARGO_INCREMENTAL".into(), "0".into());
        action_run
    }

    pub(crate) fn sandbox_paths(&self) -> [PathBuf; 3] {
        [
            PathBuf::from(&self.rustc_shim),
            PathBuf::from(&self.socket),
            PathBuf::from(&self.staging),
        ]
    }
}

pub(crate) struct TaskActionRun {
    run: String,
    agent: CacheAgent,
}

impl TaskActionRun {
    pub(crate) fn commit(self) -> Result<()> {
        self.agent.commit_task(&self.run)
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
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    server: Mutex<Option<JoinHandle<Result<()>>>>,
}

impl CacheSession {
    pub(crate) async fn start(session_dir: &Path, cache_dir: PathBuf) -> Result<Self> {
        let shim = install_session_shim(session_dir)?;
        let staging = session_dir.join("staging");
        std::fs::create_dir(&staging)?;
        let agent = CacheAgent::new(cache_dir, VERSION);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (socket, server) = spawn_server(session_dir, agent.clone(), shutdown_rx).await?;
        Ok(Self {
            environment: CacheSessionEnvironment {
                socket,
                rustc_shim: shim.to_string_lossy().into_owned(),
                staging: staging.to_string_lossy().into_owned(),
                agent: agent.clone(),
            },
            agent,
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
        Ok(self.agent.stats())
    }
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

pub(crate) fn display_stats(stats: AgentStats) {
    if stats.lookups == 0 && stats.stores == 0 {
        return;
    }
    safe_eprintln!(
        "Action cache: {}/{} hits, {} stored ({})",
        stats.hits,
        stats.lookups,
        stats.stores,
        ByteSize::b(stats.stored_bytes).display().iec(),
    );
}

fn install_session_shim(session_dir: &Path) -> Result<PathBuf> {
    let executable =
        std::env::current_exe().wrap_err("failed to locate the running mise binary")?;
    let filename = if cfg!(windows) {
        format!("{RUSTC_SHIM_STEM}.exe")
    } else {
        RUSTC_SHIM_STEM.into()
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
    if std::env::var_os(PREVIOUS_RUSTC_WRAPPER_ENV).is_none() {
        match crate::cache::rustc::compile(&rustc, &arguments) {
            Ok(exit_code) => return exit_code,
            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!("mise rustc cache bypassed: {_error:#}");
            }
        }
    }

    run_transparent_rustc(rustc, arguments)
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
    let mut stream = std::os::unix::net::UnixStream::connect(Path::new(socket))
        .wrap_err("failed to connect to the action-cache session")?;
    sync_handshake(&mut stream)?;
    requests
        .iter()
        .map(|request| sync_request(&mut stream, request))
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

#[cfg(unix)]
fn sync_handshake(stream: &mut (impl std::io::Read + Write)) -> Result<()> {
    let request = AgentRequest::Hello {
        protocol: AGENT_PROTOCOL_VERSION,
        client_version: VERSION.into(),
    };
    serde_json::to_writer(&mut *stream, &request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut response = String::new();
    BufReader::new(&mut *stream).read_line(&mut response)?;
    validate_handshake_response(&response)
}

#[cfg(unix)]
fn sync_request(
    stream: &mut (impl std::io::Read + Write),
    request: &AgentRequest,
) -> Result<AgentResponse> {
    serde_json::to_writer(&mut *stream, request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut response = String::new();
    BufReader::new(&mut *stream).read_line(&mut response)?;
    Ok(serde_json::from_str(&response)?)
}

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

    #[test]
    fn session_environment_is_scoped_to_selected_adapters() {
        let cache = tempfile::tempdir().unwrap();
        let environment = CacheSessionEnvironment {
            socket: "socket".into(),
            rustc_shim: "shim".into(),
            staging: "staging".into(),
            agent: CacheAgent::new(cache.path(), VERSION),
        };
        let mut task = Task::default();
        let mut values = BTreeMap::from([("RUSTC_WRAPPER".into(), "existing".into())]);
        let run = environment.apply(&task, &mut values);
        assert!(run.is_none());
        assert_eq!(values.get("RUSTC_WRAPPER").unwrap(), "existing");

        task.rust_cache = Some(TaskRustCacheConfig { enabled: false });
        let run = environment.apply(&task, &mut values);
        assert!(run.is_none());
        assert_eq!(values.get("RUSTC_WRAPPER").unwrap(), "existing");

        task.rust_cache = Some(TaskRustCacheConfig { enabled: true });
        let run = environment.apply(&task, &mut values);
        assert!(run.is_some());
        assert_eq!(values.get(SOCKET_ENV).unwrap(), "socket");
        assert_eq!(values.get(STAGING_ENV).unwrap(), "staging");
        assert_eq!(values.get(TASK_ENV).unwrap().len(), 64);
        assert_eq!(values.get("RUSTC_WRAPPER").unwrap(), "shim");
        assert_eq!(values.get(PREVIOUS_RUSTC_WRAPPER_ENV).unwrap(), "existing");
        assert_eq!(values.get("CARGO_INCREMENTAL").unwrap(), "0");
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
}
