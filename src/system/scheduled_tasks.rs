//! Windows Scheduled Tasks for user-scope `[bootstrap.services]` entries.
//!
//! A task named `mise\<name>` is registered from a rendered task definition
//! with `schtasks /create /xml`. The rendered definition is kept under
//! `$MISE_STATE_DIR/user-services/<name>.xml` so drift is detected against
//! what mise wrote, independent of the exporter's formatting.
//!
//! A task that sets `environment` keeps a second file, `<name>.launch.json`:
//! Task Scheduler's XML has no environment block, so such a task runs
//! through mise, which reads the environment back from there. The action
//! names that file by digest, so a changed environment changes the
//! definition and drift is still one comparison (see `exec_action`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use eyre::{Result, bail, eyre};
use indexmap::IndexMap;

const SCHTASKS_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScheduledTaskRequest {
    pub name: String,
    pub task: String,
    pub description: Option<String>,
    pub command: String,
    pub restart_on_failure: bool,
    pub environment: IndexMap<String, String>,
    /// The durable mise executable a task that sets `environment` runs
    /// through. Task Scheduler's XML has no environment block, so mise
    /// carries it (see `exec_action`); `None` when there is no mise to
    /// carry it with.
    pub launcher: Option<String>,
    pub working_directory: Option<String>,
    /// Whether the task should be running now.
    pub start: bool,
    /// A niceness above zero lowers the task's priority.
    pub nice: Option<i8>,
    /// Whether the logon trigger is enabled.
    pub at_logon: bool,
    /// End and start the task even when its definition is unchanged: the
    /// registered process is not the one the caller wants running.
    pub restart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScheduledTaskState {
    Running,
    Ready,
    Disabled,
    Differs,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScheduledTaskStatus {
    pub request: ScheduledTaskRequest,
    pub path: PathBuf,
    pub state: ScheduledTaskState,
}

impl ScheduledTaskStatus {
    pub(crate) fn is_desired(&self) -> bool {
        match self.state {
            ScheduledTaskState::Running => self.request.start,
            ScheduledTaskState::Ready => !self.request.start,
            ScheduledTaskState::Disabled
            | ScheduledTaskState::Differs
            | ScheduledTaskState::Missing => false,
        }
    }
}

impl ScheduledTaskRequest {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            task: task_name(name),
            description: None,
            command: String::new(),
            restart_on_failure: false,
            environment: IndexMap::new(),
            launcher: None,
            working_directory: None,
            start: true,
            at_logon: true,
            nice: None,
            restart: false,
        }
    }
}

pub(crate) fn is_available() -> bool {
    // spawnable as-is: `schtasks.exe`, which a plain lookup does not find
    cfg!(windows) && crate::file::which_spawnable("schtasks").is_some()
}

pub(crate) fn unavailable_reason() -> String {
    if cfg!(windows) {
        "`schtasks` not found".to_string()
    } else {
        "only available on windows".to_string()
    }
}

pub(crate) fn task_name(name: &str) -> String {
    format!("mise\\{name}")
}

/// Where the rendered definition mise registered is kept.
pub(crate) fn definition_path(name: &str) -> PathBuf {
    crate::dirs::STATE
        .join("user-services")
        .join(format!("{name}.xml"))
}

/// The account the task runs as and whose logon triggers it.
fn current_user_id() -> String {
    let user = crate::env::var("USERNAME").unwrap_or_else(|_| "".to_string());
    match crate::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    }
}

/// Render the task definition (Task Scheduler XML, UTF-16LE with a BOM as
/// `schtasks /create /xml` expects).
pub(crate) fn render_definition(request: &ScheduledTaskRequest, user_id: &str) -> Result<Vec<u8>> {
    let xml = render_xml(request, user_id)?;
    let mut out = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(out)
}

pub(crate) fn render_xml(request: &ScheduledTaskRequest, user_id: &str) -> Result<String> {
    let (command, arguments) = exec_action(request)?;
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n");
    out.push_str(
        "<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n",
    );
    out.push_str("  <RegistrationInfo>\n");
    out.push_str(&format!(
        "    <Description>{}</Description>\n",
        escape(
            request
                .description
                .as_deref()
                .unwrap_or("managed by mise bootstrap")
        )
    ));
    out.push_str("  </RegistrationInfo>\n");
    out.push_str("  <Triggers>\n    <LogonTrigger>\n");
    out.push_str(&format!(
        "      <Enabled>{}</Enabled>\n",
        yes_no(request.at_logon)
    ));
    out.push_str(&format!("      <UserId>{}</UserId>\n", escape(user_id)));
    out.push_str("    </LogonTrigger>\n  </Triggers>\n");
    out.push_str("  <Principals>\n    <Principal id=\"Author\">\n");
    out.push_str(&format!("      <UserId>{}</UserId>\n", escape(user_id)));
    out.push_str("      <LogonType>InteractiveToken</LogonType>\n");
    out.push_str("      <RunLevel>LeastPrivilege</RunLevel>\n");
    out.push_str("    </Principal>\n  </Principals>\n");
    out.push_str("  <Settings>\n");
    out.push_str("    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n");
    out.push_str("    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n");
    out.push_str("    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n");
    out.push_str("    <AllowHardTerminate>true</AllowHardTerminate>\n");
    out.push_str("    <StartWhenAvailable>true</StartWhenAvailable>\n");
    out.push_str("    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\n");
    out.push_str("    <AllowStartOnDemand>true</AllowStartOnDemand>\n");
    out.push_str("    <Enabled>true</Enabled>\n");
    out.push_str("    <Hidden>false</Hidden>\n");
    out.push_str("    <RunOnlyIfIdle>false</RunOnlyIfIdle>\n");
    out.push_str("    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n");
    if request.restart_on_failure {
        out.push_str("    <RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>3</Count>\n    </RestartOnFailure>\n");
    }
    // 7 is the default; a nice service runs at the lowest normal priority
    let priority = if request.nice.is_some_and(|nice| nice > 0) {
        9
    } else {
        7
    };
    out.push_str(&format!("    <Priority>{priority}</Priority>\n"));
    out.push_str("  </Settings>\n");
    out.push_str("  <Actions Context=\"Author\">\n    <Exec>\n");
    out.push_str(&format!("      <Command>{}</Command>\n", escape(&command)));
    if !arguments.is_empty() {
        out.push_str(&format!(
            "      <Arguments>{}</Arguments>\n",
            escape(&arguments)
        ));
    }
    if let Some(dir) = &request.working_directory {
        out.push_str(&format!(
            "      <WorkingDirectory>{}</WorkingDirectory>\n",
            escape(&expand_path_string(dir))
        ));
    }
    out.push_str("    </Exec>\n  </Actions>\n");
    out.push_str("</Task>\n");
    Ok(out)
}

/// What a task that sets `environment` runs, stored beside its definition
/// because Task Scheduler's XML has nowhere to put it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ServiceLaunch {
    pub program: String,
    /// The rest of the command line, kept as one string and passed to the
    /// program unchanged. Splitting it would hand the program mise's idea
    /// of where its arguments end instead of its own.
    pub args: String,
    pub environment: IndexMap<String, String>,
}

/// The launch a task carries, or `None` when its action starts the program
/// itself because there is no environment to carry.
///
/// `cmd.exe` is gone, so a shell's metacharacters are carried as written.
/// What Windows itself cannot represent still is not: a process environment
/// is `KEY=VALUE` pairs in a NUL-separated block, so a name that is empty or
/// contains `=`, and any string containing a NUL, have nowhere to go.
/// Rejecting them here means a declaration that cannot run fails while its
/// definition is rendered, rather than registering a task that Task
/// Scheduler starts and Windows refuses.
fn launch(request: &ScheduledTaskRequest) -> Result<Option<ServiceLaunch>> {
    if request.environment.is_empty() {
        return Ok(None);
    }
    for (key, value) in &request.environment {
        if key.is_empty() || key.contains(['=', '\0']) {
            bail!(
                "user service '{}': environment name {key:?} cannot be a Windows environment variable",
                request.name
            );
        }
        if value.contains('\0') {
            bail!(
                "user service '{}': environment value for {key} contains a NUL, which cannot be passed to a process",
                request.name
            );
        }
    }
    let (program, args) = split_command(&request.command);
    Ok(Some(ServiceLaunch {
        program,
        args,
        environment: request.environment.clone(),
    }))
}

/// The launch as it is stored: one line of JSON, so the bytes on disk are
/// the bytes the action's digest covers.
pub(crate) fn render_launch(request: &ScheduledTaskRequest) -> Result<Option<String>> {
    launch(request)?
        .map(|launch| Ok(serde_json::to_string(&launch)?))
        .transpose()
}

/// Where the launch a task was registered with is kept.
pub(crate) fn launch_path(name: &str) -> PathBuf {
    crate::dirs::STATE
        .join("user-services")
        .join(format!("{name}.launch.json"))
}

/// The executable and arguments the task's `<Exec>` action runs.
///
/// With no environment to carry, that is the declared command itself. Task
/// Scheduler's XML has no environment block, so a task that sets one starts
/// mise instead: it applies the environment and runs the service as its
/// child (`bootstrap __service-exec`, see `crate::system::service_exec`).
/// Nothing the declaration contains reaches a command line on the way —
/// the environment and the command line travel as JSON — so no key, value,
/// or argument has to be rejected for what a shell would make of it.
///
/// The digest pins the action to the launch it was rendered from, so the
/// registered definition changes whenever the environment does, and a
/// launch edited behind mise's back does not run. The launch is named by
/// absolute path: Task Scheduler starts the service from the user's logon
/// environment, which need not be the one `apply` ran under, so a
/// `MISE_STATE_DIR` set only for the apply would otherwise leave the
/// service looking for its launch somewhere it was never written.
fn exec_action(request: &ScheduledTaskRequest) -> Result<(String, String)> {
    let Some(launch) = launch(request)? else {
        return Ok(split_command(&request.command));
    };
    let Some(mise) = request.launcher.as_deref() else {
        bail!(
            "user service '{}' sets `environment`, which mise carries for it; install mise on this host first",
            request.name
        );
    };
    let digest = crate::hash::hash_blake3_to_str(&serde_json::to_string(&launch)?);
    let path = quote_argument(&launch_path(&request.name).to_string_lossy());
    Ok((
        mise.to_string(),
        format!(
            "bootstrap __service-exec {} --launch {path} --digest {digest}",
            request.name
        ),
    ))
}

/// Quote one argument for the command line Task Scheduler hands to
/// `CreateProcess`. A path is the only thing that goes through here, so the
/// backslash-before-quote rule the runtime's parser applies cannot arise:
/// the quotes are added around the whole value and a path cannot contain
/// one.
fn quote_argument(value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

/// Whether what is on disk is what `request` renders now: its definition
/// and, for a task that carries an environment, the launch its registered
/// action is pinned to.
fn stored_is_current(request: &ScheduledTaskRequest, user_id: &str) -> Result<bool> {
    if std::fs::read(definition_path(&request.name)).unwrap_or_default()
        != render_definition(request, user_id)?
    {
        return Ok(false);
    }
    let Some(launch) = render_launch(request)? else {
        return Ok(true);
    };
    Ok(std::fs::read(launch_path(&request.name)).unwrap_or_default() == launch.into_bytes())
}

fn split_command(command: &str) -> (String, String) {
    let trimmed = command.trim();
    let (program, args) = if let Some(rest) = trimmed.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        (rest[..end].to_string(), rest[end + 1..].trim().to_string())
    } else {
        match trimmed.split_once(char::is_whitespace) {
            Some((program, args)) => (program.to_string(), args.trim().to_string()),
            None => (trimmed.to_string(), String::new()),
        }
    };
    // `~` and `~/` expand on every platform, as the docs promise
    let program = if program == "~" || program.starts_with("~/") || program.starts_with("~\\") {
        expand_path_string(&program)
    } else {
        program
    };
    (program, args)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn yes_no(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn expand_path_string(path: &str) -> String {
    if path == "~" {
        return crate::dirs::HOME.to_string_lossy().to_string();
    }
    crate::file::replace_path(Path::new(path))
        .to_string_lossy()
        .to_string()
}

pub(crate) async fn status(requests: &[ScheduledTaskRequest]) -> Result<Vec<ScheduledTaskStatus>> {
    let user_id = current_user_id();
    let mut out = vec![];
    for req in requests {
        let path = definition_path(&req.name);
        let registered = query(&req.task).await?;
        let state = match registered {
            None => ScheduledTaskState::Missing,
            Some(query) => {
                if !stored_is_current(req, &user_id)? {
                    ScheduledTaskState::Differs
                } else if query.running {
                    ScheduledTaskState::Running
                } else if query.disabled {
                    ScheduledTaskState::Disabled
                } else {
                    ScheduledTaskState::Ready
                }
            }
        };
        out.push(ScheduledTaskStatus {
            request: req.clone(),
            path,
            state,
        });
    }
    Ok(out)
}

pub(crate) async fn exists(name: &str) -> Result<bool> {
    Ok(query(&task_name(name)).await?.is_some())
}

/// Whether registering `req` must end the running instance first, and
/// whether it must then run the task. A running instance survives an
/// unchanged registration (`IgnoreNew`), so a task that must not keep its
/// old process — it is being stopped, its definition changed, or its
/// process is not the one the caller wants — has to be ended explicitly.
fn transition(running: bool, changed: bool, start: bool, restart: bool) -> (bool, bool) {
    let replace = changed || restart;
    (
        running && (!start || replace),
        start && (!running || replace),
    )
}

pub(crate) async fn apply(requests: &[ScheduledTaskRequest], dry_run: bool) -> Result<()> {
    let user_id = current_user_id();
    for req in requests {
        let path = definition_path(&req.name);
        // the definition is registered from a staging file and stored only
        // once Task Scheduler accepted it, so a failed create never leaves a
        // definition on disk that status would take for the registered one
        let staging = path.with_extension("xml.new");
        let rendered = render_definition(req, &user_id)?;
        let launch = render_launch(req)?;
        let launch_path = launch_path(&req.name);
        let create = [
            "/create".to_string(),
            "/tn".to_string(),
            req.task.clone(),
            "/xml".to_string(),
            staging.display().to_string(),
            "/f".to_string(),
        ];
        let end = [
            "/end".to_string(),
            "/tn".to_string(),
            req.task.clone(),
            "/HRESULT".to_string(),
        ];
        let run = ["/run".to_string(), "/tn".to_string(), req.task.clone()];
        // what is registered now: a running instance keeps its old process
        // (`IgnoreNew`), so a changed definition or a stop ends it first, and
        // a task that is not running is never ended (its message is
        // localized, so it is not parsed)
        let registered = query(&req.task).await?;
        let running = registered.as_ref().is_some_and(|query| query.running);
        let changed = registered.is_some() && !stored_is_current(req, &user_id)?;
        let (end_first, start) = transition(running, changed, req.start, req.restart);
        if dry_run {
            if launch.is_some() {
                miseprintln!(
                    "write {}",
                    shell_words::join([launch_path.display().to_string()])
                );
            }
            miseprintln!("write {}", shell_words::join([path.display().to_string()]));
            miseprintln!("schtasks {}", shell_words::join(&create));
            if end_first {
                miseprintln!("schtasks {}", shell_words::join(&end));
            }
            if start {
                miseprintln!("schtasks {}", shell_words::join(&run));
            }
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The launch goes down before the task that reads it is registered,
        // and a task that stopped setting an environment leaves none behind.
        // What was there is kept until the registration commits: the task
        // registered right now is pinned to it by digest, so replacing it
        // and then failing to register its replacement would leave a working
        // service unable to start until some later apply succeeded.
        let replaced = std::fs::read(&launch_path).ok();
        match &launch {
            Some(launch) => std::fs::write(&launch_path, launch)?,
            None => {
                let _ = std::fs::remove_file(&launch_path);
            }
        }
        let restore_launch = || match &replaced {
            Some(previous) => {
                let _ = std::fs::write(&launch_path, previous);
            }
            None => {
                let _ = std::fs::remove_file(&launch_path);
            }
        };
        if let Err(err) = std::fs::write(&staging, &rendered) {
            restore_launch();
            return Err(err.into());
        }
        if let Err(err) = schtasks(&create).await {
            let _ = std::fs::remove_file(&staging);
            restore_launch();
            return Err(err);
        }
        // written, not renamed: a rename does not replace an existing
        // definition on Windows
        std::fs::write(&path, &rendered)?;
        let _ = std::fs::remove_file(&staging);
        if end_first {
            // it may have exited between the query and now: the HRESULT
            // says so in every locale; the message is matched as a fallback
            let (status, printed) = schtasks_output(&end).await?;
            if !status.success()
                && status.code() != Some(SCHED_E_TASK_NOT_RUNNING)
                && !end_error_is_noop(&printed)
            {
                bail!("`schtasks {}` failed: {printed}", shell_words::join(&end));
            }
        }
        if start {
            schtasks(&run).await?;
        }
    }
    Ok(())
}

/// Delete the task mise registered for `name`. Returns whether one existed.
pub(crate) async fn remove_task(name: &str, dry_run: bool) -> Result<bool> {
    let task = task_name(name);
    let path = definition_path(name);
    let launch = launch_path(name);
    if !exists(name).await? {
        if !dry_run {
            for stale in [&path, &launch] {
                if stale.exists() {
                    std::fs::remove_file(stale)?;
                }
            }
        }
        return Ok(false);
    }
    let args = [
        "/delete".to_string(),
        "/tn".to_string(),
        task,
        "/f".to_string(),
    ];
    if dry_run {
        miseprintln!("schtasks {}", shell_words::join(&args));
        for stale in [&path, &launch] {
            if stale.exists() {
                miseprintln!(
                    "{}",
                    shell_words::join(["rm".to_string(), stale.display().to_string()])
                );
            }
        }
        return Ok(true);
    }
    schtasks(&args).await?;
    for stale in [&path, &launch] {
        if stale.exists() {
            std::fs::remove_file(stale)?;
        }
    }
    Ok(true)
}

struct Query {
    running: bool,
    disabled: bool,
}

/// The task's state through the Task Scheduler API rather than the
/// localized text `schtasks /query` prints. Prints `MISSING` for an
/// unregistered task and the `TaskState` name otherwise. The name is
/// embedded in the script (arguments after `-Command` are more command
/// text, not `$args`); names are validated to letters, digits, `.`, `_`,
/// and `-` before they get here.
fn query_script(name: &str) -> String {
    format!(
        "$t = Get-ScheduledTask -TaskPath '\\mise\\' -TaskName '{name}' -ErrorAction SilentlyContinue; if ($null -eq $t) {{ 'MISSING' }} else {{ $t.State.ToString() }}"
    )
}

async fn query(task: &str) -> Result<Option<Query>> {
    let name = task.strip_prefix("mise\\").unwrap_or(task);
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        bail!("scheduled task name {name:?} contains characters that cannot be queried");
    }
    let args = [
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        query_script(name),
    ];
    debug!("$ powershell {}", shell_words::join(&args));
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SCHTASKS_TIMEOUT, cmd.output())
        .await
        .map_err(|_| eyre!("querying scheduled task {task} timed out"))??;
    if !output.status.success() {
        bail!(
            "querying scheduled task {task} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(parse_query(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_query(output: &str) -> Option<Query> {
    let state = output.trim();
    if state.eq_ignore_ascii_case("MISSING") || state.is_empty() {
        return None;
    }
    Some(Query {
        running: state.eq_ignore_ascii_case("Running"),
        disabled: state.eq_ignore_ascii_case("Disabled"),
    })
}

/// `SCHED_E_TASK_NOT_RUNNING`: the HRESULT `schtasks /end /HRESULT` exits
/// with when the task has no running instance.
const SCHED_E_TASK_NOT_RUNNING: i32 = 0x8004130Bu32 as i32;

fn end_error_is_noop(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("not running") || error.contains("no running instance")
}

async fn schtasks(args: &[String]) -> Result<()> {
    let (status, printed) = schtasks_output(args).await?;
    if !status.success() {
        bail!("`schtasks {}` failed: {printed}", shell_words::join(args));
    }
    Ok(())
}

/// Runs schtasks; its exit status and what it printed (schtasks writes its
/// SUCCESS and ERROR lines to stdout).
async fn schtasks_output(args: &[String]) -> Result<(std::process::ExitStatus, String)> {
    debug!("$ schtasks {}", shell_words::join(args));
    let mut cmd = tokio::process::Command::new("schtasks");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SCHTASKS_TIMEOUT, cmd.output())
        .await
        .map_err(|_| eyre!("`schtasks {}` timed out", shell_words::join(args)))??;
    let printed = [
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ]
    .iter()
    .map(|text| text.trim().to_string())
    .filter(|text| !text.is_empty())
    .collect::<Vec<_>>()
    .join("; ");
    Ok((output.status, printed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ScheduledTaskRequest {
        let mut request = ScheduledTaskRequest::new("agent");
        request.command = "C:\\Tools\\agent.exe --serve".to_string();
        request.description = Some("My <agent>".to_string());
        request.restart_on_failure = true;
        request
    }

    /// Registering a task does not disturb a running instance, so what an
    /// apply ends and runs is decided here. A converged running task is left
    /// alone unless the caller says its process is the wrong one, which is
    /// how a stale history watcher is replaced. All sixteen combinations are
    /// written down: the decision has four inputs, so anything less leaves
    /// room for a precedence change between `changed` and `restart` to pass.
    #[test]
    fn a_running_task_is_replaced_only_when_it_must_be() {
        // (running, changed, start, restart) -> (end_first, run)
        let cases = [
            // Not registered as running. Nothing to end; it is run whenever
            // it should be running, and `changed` and `restart` add nothing.
            ((false, false, false, false), (false, false)),
            ((false, false, false, true), (false, false)),
            ((false, true, false, false), (false, false)),
            ((false, true, false, true), (false, false)),
            ((false, false, true, false), (false, true)),
            ((false, false, true, true), (false, true)),
            ((false, true, true, false), (false, true)),
            ((false, true, true, true), (false, true)),
            // Running and wanted stopped: ended, never run.
            ((true, false, false, false), (true, false)),
            ((true, false, false, true), (true, false)),
            ((true, true, false, false), (true, false)),
            ((true, true, false, true), (true, false)),
            // Running, wanted running, and converged: left alone. This is
            // the state a stale watcher sits in, and why an apply had to be
            // told to replace it.
            ((true, false, true, false), (false, false)),
            // ... which either a changed definition or a forced restart does.
            ((true, false, true, true), (true, true)),
            ((true, true, true, false), (true, true)),
            ((true, true, true, true), (true, true)),
        ];
        assert_eq!(cases.len(), 16, "every combination is covered");
        for ((running, changed, start, restart), expected) in cases {
            assert_eq!(
                transition(running, changed, start, restart),
                expected,
                "running={running} changed={changed} start={start} restart={restart}"
            );
        }
    }

    #[test]
    fn renders_a_logon_task() {
        let xml = render_xml(&sample(), "HOST\\me").unwrap();
        assert!(xml.contains("<Description>My &lt;agent&gt;</Description>"));
        assert!(xml.contains(
            "<LogonTrigger>\n      <Enabled>true</Enabled>\n      <UserId>HOST\\me</UserId>"
        ));
        assert!(xml.contains("<Command>C:\\Tools\\agent.exe</Command>"));
        assert!(xml.contains("<Arguments>--serve</Arguments>"));
        assert!(xml.contains("<RestartOnFailure>"));
        assert!(!xml.contains("<WorkingDirectory>"));
    }

    /// An environment turns the action into mise, which carries it: the
    /// declared command moves into the launch stored beside the definition,
    /// and the action names that launch by digest.
    #[test]
    fn an_environment_makes_mise_the_action() {
        let mut request = sample();
        request.environment.insert("RUST_LOG".into(), "info".into());
        request.launcher = Some("C:\\mise\\mise.exe".to_string());
        request.at_logon = false;
        request.restart_on_failure = false;

        let xml = render_xml(&request, "me").unwrap();
        assert!(
            xml.contains("<Command>C:\\mise\\mise.exe</Command>"),
            "{xml}"
        );
        let launch = render_launch(&request).unwrap().unwrap();
        let digest = crate::hash::hash_blake3_to_str(&launch);
        // the launch is named by absolute path: the service is started from
        // the user's logon environment, which need not have the
        // `MISE_STATE_DIR` the apply ran under
        let path = launch_path("agent");
        assert!(path.is_absolute(), "{}", path.display());
        let expected = format!(
            "<Arguments>bootstrap __service-exec agent --launch {} --digest {digest}</Arguments>",
            escape(&quote_argument(&path.to_string_lossy()))
        );
        assert!(xml.contains(&expected), "{xml}\nexpected {expected}");
        // nothing the declaration holds reaches a command line
        assert!(!xml.contains("RUST_LOG"), "{xml}");
        assert!(!xml.contains("agent.exe"), "{xml}");
        assert!(xml.contains("<Enabled>false</Enabled>\n      <UserId>me</UserId>"));
        assert!(!xml.contains("<RestartOnFailure>"));

        // the launch carries the command split the way Task Scheduler would
        // have passed it, and the environment as declared
        let launch: ServiceLaunch = serde_json::from_str(&launch).unwrap();
        assert_eq!(launch.program, "C:\\Tools\\agent.exe");
        assert_eq!(launch.args, "--serve");
        assert_eq!(launch.environment["RUST_LOG"], "info");

        // the digest follows the environment, so a changed one is drift
        let mut changed = request.clone();
        changed
            .environment
            .insert("RUST_LOG".into(), "debug".into());
        assert_ne!(render_xml(&changed, "me").unwrap(), xml);
    }

    /// The characters `cmd.exe` used to force a bail on. None of them reach
    /// a command line now: the environment and the command travel as JSON
    /// and are applied by mise, so they are carried as written.
    #[test]
    fn what_cmd_would_have_reinterpreted_is_carried_as_written() {
        let mut request = sample();
        request.command = "C:\\Tools\\agent.exe --filter a&b|c<d>e^f".to_string();
        request.launcher = Some("mise.exe".to_string());
        request
            .environment
            .insert("P".into(), "%PATH%;C:\\x".into());
        request
            .environment
            .insert("Q".into(), "a \"quoted\" & piped | value".into());

        let xml = render_xml(&request, "me").unwrap();
        assert!(xml.contains("<Command>mise.exe</Command>"), "{xml}");
        let launch: ServiceLaunch =
            serde_json::from_str(&render_launch(&request).unwrap().unwrap()).unwrap();
        assert_eq!(launch.args, "--filter a&b|c<d>e^f");
        assert_eq!(launch.environment["P"], "%PATH%;C:\\x");
        assert_eq!(launch.environment["Q"], "a \"quoted\" & piped | value");
    }

    /// A shell's metacharacters are carried as written now, but Windows
    /// still cannot hold every name: an environment is `KEY=VALUE` pairs in
    /// a NUL-separated block. Those are refused while the definition is
    /// rendered, so a task that Windows would refuse to start is never
    /// registered in the first place.
    #[test]
    fn names_windows_cannot_hold_are_still_refused() {
        for (key, value) in [("A=B", "1"), ("", "1"), ("A\0B", "1")] {
            let mut request = sample();
            request.launcher = Some("mise.exe".to_string());
            request.environment.insert(key.into(), value.into());
            let err = render_xml(&request, "me").unwrap_err().to_string();
            assert!(
                err.contains("cannot be a Windows environment"),
                "{key:?}: {err}"
            );
        }

        let mut request = sample();
        request.launcher = Some("mise.exe".to_string());
        request.environment.insert("A".into(), "one\0two".into());
        let err = render_xml(&request, "me").unwrap_err().to_string();
        assert!(err.contains("contains a NUL"), "{err}");
    }

    /// A path with a space in it still reaches the launcher as one argument.
    #[test]
    fn a_launch_path_with_spaces_is_quoted() {
        assert_eq!(quote_argument("C:\\x\\y.json"), "C:\\x\\y.json");
        assert_eq!(
            quote_argument("C:\\Program Files\\y.json"),
            "\"C:\\Program Files\\y.json\""
        );
    }

    /// Carrying an environment takes a mise that will still be there when
    /// the task runs; without one the definition does not render at all,
    /// rather than registering a task that cannot start.
    #[test]
    fn carrying_an_environment_needs_mise() {
        let mut request = sample();
        request.environment.insert("A".into(), "1".into());
        let err = render_xml(&request, "me").unwrap_err().to_string();
        assert!(err.contains("install mise on this host first"), "{err}");
    }

    /// Without an environment there is nothing to carry, so the action is
    /// the declared program and no launch is stored beside it.
    #[test]
    fn no_environment_leaves_the_action_alone() {
        let request = sample();
        let xml = render_xml(&request, "me").unwrap();
        assert!(xml.contains("<Command>C:\\Tools\\agent.exe</Command>"));
        assert!(xml.contains("<Arguments>--serve</Arguments>"));
        assert!(render_launch(&request).unwrap().is_none());
    }

    #[test]
    fn tilde_expands_in_the_program() {
        let (program, args) = split_command("~/.local/bin/agent --serve");
        assert!(!program.starts_with('~'), "{program}");
        assert!(program.ends_with("agent"), "{program}");
        assert_eq!(args, "--serve");
    }

    #[test]
    fn quoted_programs_keep_their_spaces() {
        assert_eq!(
            split_command("\"C:\\Program Files\\x\\a.exe\" --flag one"),
            (
                "C:\\Program Files\\x\\a.exe".to_string(),
                "--flag one".to_string()
            )
        );
        assert_eq!(
            split_command("agent.exe"),
            ("agent.exe".to_string(), String::new())
        );
    }

    #[test]
    fn definition_is_utf16_with_bom() {
        let bytes = render_definition(&sample(), "me").unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        assert_eq!(&bytes[2..4], &[b'<', 0]);
    }

    #[test]
    fn parses_query_output() {
        let query = parse_query("Running\r\n").unwrap();
        assert!(query.running);
        assert!(!query.disabled);
        let query = parse_query("Disabled\n").unwrap();
        assert!(!query.running);
        assert!(query.disabled);
        let query = parse_query("Ready\n").unwrap();
        assert!(!query.running && !query.disabled);
        assert!(parse_query("MISSING\n").is_none());
    }

    #[test]
    fn desired_state_follows_start() {
        let mut status = ScheduledTaskStatus {
            request: sample(),
            path: PathBuf::from("x"),
            state: ScheduledTaskState::Running,
        };
        assert!(status.is_desired());
        status.request.start = false;
        assert!(!status.is_desired());
        status.state = ScheduledTaskState::Ready;
        assert!(status.is_desired());
        status.state = ScheduledTaskState::Differs;
        assert!(!status.is_desired());
    }
}
