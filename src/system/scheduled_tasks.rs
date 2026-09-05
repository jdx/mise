//! Windows Scheduled Tasks for user-scope `[bootstrap.services]` entries.
//!
//! A task named `mise\<name>` is registered from a rendered task definition
//! with `schtasks /create /xml`. The rendered definition is kept under
//! `$MISE_STATE_DIR/user-services/<name>.xml` so drift is detected against
//! what mise wrote, independent of the exporter's formatting.

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
    pub working_directory: Option<String>,
    /// Whether the task should be running now.
    pub start: bool,
    /// Whether the logon trigger is enabled.
    pub at_logon: bool,
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
            working_directory: None,
            start: true,
            at_logon: true,
        }
    }
}

pub(crate) fn is_available() -> bool {
    cfg!(windows) && crate::file::which("schtasks").is_some()
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
    out.push_str("    <Priority>7</Priority>\n");
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

/// Split the command line into the executable and its arguments. Task
/// Scheduler has no environment block, so variables are set through
/// `cmd.exe`, which reinterprets some characters; values that it would
/// change are rejected rather than passed through differently.
fn exec_action(request: &ScheduledTaskRequest) -> Result<(String, String)> {
    let (program, args) = split_command(&request.command);
    if request.environment.is_empty() {
        return Ok((program, args));
    }
    let mut sets = vec![];
    for (key, value) in &request.environment {
        if key.is_empty() || key.contains(['=', '"', '%', '\n', '\r']) {
            bail!(
                "user service '{}': environment key {key:?} cannot be set through cmd.exe",
                request.name
            );
        }
        if let Some(c) = value
            .chars()
            .find(|c| matches!(c, '"' | '%' | '&' | '|' | '<' | '>' | '^' | '\n' | '\r'))
        {
            bail!(
                "user service '{}': environment value for {key} contains {c:?}, which cmd.exe would reinterpret; set it inside the program instead",
                request.name
            );
        }
        sets.push(format!("set \"{key}={value}\""));
    }
    let program = if program.contains(char::is_whitespace) {
        format!("\"{program}\"")
    } else {
        program
    };
    let rest = if args.is_empty() {
        program
    } else {
        format!("{program} {args}")
    };
    Ok((
        "cmd.exe".to_string(),
        format!("/c {} && {rest}", sets.join(" && ")),
    ))
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
                let stored = std::fs::read(&path).unwrap_or_default();
                if stored != render_definition(req, &user_id)? {
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

pub(crate) async fn apply(requests: &[ScheduledTaskRequest], dry_run: bool) -> Result<()> {
    let user_id = current_user_id();
    for req in requests {
        let path = definition_path(&req.name);
        let create = [
            "/create".to_string(),
            "/tn".to_string(),
            req.task.clone(),
            "/xml".to_string(),
            path.display().to_string(),
            "/f".to_string(),
        ];
        let run_or_end = if req.start {
            ["/run".to_string(), "/tn".to_string(), req.task.clone()]
        } else {
            ["/end".to_string(), "/tn".to_string(), req.task.clone()]
        };
        if dry_run {
            miseprintln!("write {}", shell_words::join([path.display().to_string()]));
            miseprintln!("schtasks {}", shell_words::join(&create));
            miseprintln!("schtasks {}", shell_words::join(&run_or_end));
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, render_definition(req, &user_id)?)?;
        schtasks(&create).await?;
        match schtasks(&run_or_end).await {
            Ok(()) => {}
            // ending a task that is not running is not an error worth failing on
            Err(err) if !req.start && end_error_is_noop(&err.to_string()) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Delete the task mise registered for `name`. Returns whether one existed.
pub(crate) async fn remove_task(name: &str, dry_run: bool) -> Result<bool> {
    let task = task_name(name);
    let path = definition_path(name);
    if !exists(name).await? {
        if path.exists() && !dry_run {
            std::fs::remove_file(&path)?;
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
        if path.exists() {
            miseprintln!(
                "{}",
                shell_words::join(["rm".to_string(), path.display().to_string()])
            );
        }
        return Ok(true);
    }
    schtasks(&args).await?;
    if path.exists() {
        std::fs::remove_file(&path)?;
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

fn end_error_is_noop(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("not running") || error.contains("no running instance")
}

async fn schtasks(args: &[String]) -> Result<()> {
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
    if !output.status.success() {
        bail!(
            "`schtasks {}` failed: {}",
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
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

    #[test]
    fn environment_goes_through_cmd() {
        let mut request = sample();
        request.environment.insert("RUST_LOG".into(), "info".into());
        request.at_logon = false;
        request.restart_on_failure = false;
        let xml = render_xml(&request, "me").unwrap();
        assert!(xml.contains("<Command>cmd.exe</Command>"));
        assert!(xml.contains(
            "<Arguments>/c set &quot;RUST_LOG=info&quot; &amp;&amp; C:\\Tools\\agent.exe --serve</Arguments>"
        ));
        assert!(xml.contains("<Enabled>false</Enabled>\n      <UserId>me</UserId>"));
        assert!(!xml.contains("<RestartOnFailure>"));

        let mut request = sample();
        request.command = "\"C:\\Program Files\\x\\a.exe\" --serve".to_string();
        request.environment.insert("A".into(), "1".into());
        let xml = render_xml(&request, "me").unwrap();
        assert!(xml.contains(
            "<Arguments>/c set &quot;A=1&quot; &amp;&amp; &quot;C:\\Program Files\\x\\a.exe&quot; --serve</Arguments>"
        ));

        let mut request = sample();
        request
            .environment
            .insert("P".into(), "%PATH%;C:\\x".into());
        let err = render_xml(&request, "me").unwrap_err().to_string();
        assert!(err.contains("cmd.exe would reinterpret"), "{err}");
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
