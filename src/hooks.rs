use crate::cmd::cmd;
use crate::config::{Config, Settings, config_file};
use crate::shell::Shell;
use crate::tera::{contains_template_syntax, get_tera, render_str};
use crate::toolset::{ToolVersion, Toolset};
use crate::{dirs, hook_env};
use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::Mutex;
use std::{iter::once, sync::Arc};
use tokio::sync::OnceCell;

/// Represents installed tool info for hooks
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledToolInfo {
    pub name: String,
    pub version: String,
}

impl From<&ToolVersion> for InstalledToolInfo {
    fn from(tv: &ToolVersion) -> Self {
        Self {
            name: tv.ba().short.clone(),
            version: tv.version.clone(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    strum::Display,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Hooks {
    Enter,
    Leave,
    Cd,
    Preinstall,
    Postinstall,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookScripts {
    One(String),
    Many(Vec<String>),
}

impl HookScripts {
    fn into_script(self) -> String {
        match self {
            Self::One(script) => script,
            Self::Many(scripts) => scripts.join("\n"),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum HookDef {
    /// Simple run string: `enter = "echo hello"`
    RunString(String),
    /// Table with run: `enter = { run = "echo hello" }`
    Run(HookRunTable),
    /// Table with script and optional shell: `enter = { script = "echo hello", shell = "bash" }`
    ScriptTable {
        script: HookScripts,
        shell: Option<String>,
    },
    /// Table with scripts and optional shell: `enter = { scripts = ["echo hello"], shell = "bash" }`
    ScriptsTable {
        scripts: Vec<String>,
        shell: Option<String>,
    },
    /// Task reference: `enter = { task = "setup" }`
    TaskRef { task: String },
    /// Array of hook definitions: `enter = ["echo hello", { task = "setup" }]`
    Array(Vec<HookDef>),
}

#[derive(Debug, Clone)]
pub struct HookRunTable {
    run: Option<String>,
    run_windows: Option<String>,
    shell: Option<String>,
}

impl<'de> serde::Deserialize<'de> for HookRunTable {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Helper {
            run: Option<String>,
            run_windows: Option<String>,
            shell: Option<String>,
        }

        let helper = <Helper as serde::Deserialize>::deserialize(deserializer)?;
        if helper.run.is_none() && helper.run_windows.is_none() {
            return Err(serde::de::Error::custom(
                "hook run table must define `run` or `run_windows`",
            ));
        }
        Ok(Self {
            run: helper.run,
            run_windows: helper.run_windows,
            shell: helper.shell,
        })
    }
}

impl HookDef {
    /// Convert to a list of Hook structs with the given hook type
    pub fn into_hooks(self, hook_type: Hooks) -> Vec<Hook> {
        match self {
            HookDef::RunString(script) => vec![Hook {
                hook: hook_type,
                action: HookAction::Run {
                    run: Some(script),
                    run_windows: None,
                    shell: None,
                    legacy_script: false,
                    ignored_shell: None,
                },
                global: false,
            }],
            HookDef::Run(table) => vec![Hook {
                hook: hook_type,
                action: HookAction::Run {
                    run: table.run,
                    run_windows: table.run_windows,
                    shell: table.shell,
                    legacy_script: false,
                    ignored_shell: None,
                },
                global: false,
            }],
            HookDef::ScriptTable { script, shell } => vec![Hook {
                hook: hook_type,
                action: script_hook_action(hook_type, script.into_script(), shell, true),
                global: false,
            }],
            HookDef::ScriptsTable { scripts, shell } => vec![Hook {
                hook: hook_type,
                action: script_hook_action(hook_type, scripts.join("\n"), shell, false),
                global: false,
            }],
            HookDef::TaskRef { task } => vec![Hook {
                hook: hook_type,
                action: HookAction::Task { task_name: task },
                global: false,
            }],
            HookDef::Array(arr) => arr
                .into_iter()
                .flat_map(|d| d.into_hooks(hook_type))
                .collect(),
        }
    }
}

fn script_hook_action(
    hook_type: Hooks,
    script: String,
    shell: Option<String>,
    legacy_script: bool,
) -> HookAction {
    match (hook_type, shell) {
        (Hooks::Enter | Hooks::Leave | Hooks::Cd, Some(shell)) => {
            HookAction::CurrentShell { script, shell }
        }
        (_, shell) => HookAction::Run {
            run: Some(script),
            run_windows: None,
            shell: None,
            legacy_script,
            ignored_shell: shell,
        },
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Hook {
    pub hook: Hooks,
    pub action: HookAction,
    /// Whether this hook comes from a global config (skip directory matching)
    pub global: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum HookAction {
    Run {
        run: Option<String>,
        run_windows: Option<String>,
        shell: Option<String>,
        legacy_script: bool,
        ignored_shell: Option<String>,
    },
    CurrentShell {
        script: String,
        shell: String,
    },
    Task {
        task_name: String,
    },
}

impl Hook {
    pub fn render_templates<F>(&mut self, mut render: F) -> Result<()>
    where
        F: FnMut(&str) -> Result<String>,
    {
        match &mut self.action {
            HookAction::Run {
                run,
                run_windows,
                shell,
                ignored_shell,
                ..
            } => {
                let run = if cfg!(windows) {
                    run_windows.as_mut().or(run.as_mut())
                } else {
                    run.as_mut()
                };
                if let Some(s) = run {
                    *s = render(s)?;
                    if let Some(s) = shell {
                        *s = render(s)?;
                    }
                    if let Some(s) = ignored_shell {
                        *s = render(s)?;
                    }
                }
            }
            HookAction::CurrentShell { script, shell } => {
                *script = render(script)?;
                *shell = render(shell)?;
            }
            HookAction::Task { task_name } => {
                *task_name = render(task_name)?;
            }
        }
        Ok(())
    }
}

pub static SCHEDULED_HOOKS: Lazy<Mutex<IndexSet<Hooks>>> = Lazy::new(Default::default);

pub fn schedule_hook(hook: Hooks) {
    let mut mu = SCHEDULED_HOOKS.lock().unwrap();
    mu.insert(hook);
}

pub async fn run_all_hooks(config: &Arc<Config>, ts: &Toolset, shell: &dyn Shell) {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    let hooks = {
        let mut mu = SCHEDULED_HOOKS.lock().unwrap();
        mu.drain(..).collect::<Vec<_>>()
    };
    for hook in hooks {
        run_one_hook(config, ts, hook, Some(shell)).await;
    }
}

async fn all_hooks(config: &Arc<Config>) -> &'static Vec<(PathBuf, Hook)> {
    static ALL_HOOKS: OnceCell<Vec<(PathBuf, Hook)>> = OnceCell::const_new();
    ALL_HOOKS
        .get_or_init(async || {
            let mut hooks = config.hooks().await.cloned().unwrap_or_default();
            let cur_configs = config.config_files.keys().cloned().collect::<IndexSet<_>>();
            let prev_configs = &hook_env::PREV_SESSION.loaded_configs;
            let old_configs = prev_configs.difference(&cur_configs);
            for p in old_configs {
                if let Ok(cf) = config_file::parse(p).await
                    && let Ok(mut h) = cf.hooks()
                {
                    let is_global = cf.project_root().is_none();
                    if is_global {
                        for hook in &mut h {
                            hook.global = true;
                        }
                    }
                    let root = cf.project_root().unwrap_or_else(|| cf.config_root());
                    hooks.extend(h.into_iter().map(|h| (root.clone(), h)));
                }
            }
            hooks
        })
        .await
}

#[async_backtrace::framed]
pub async fn run_one_hook(
    config: &Arc<Config>,
    ts: &Toolset,
    hook: Hooks,
    shell: Option<&dyn Shell>,
) {
    run_one_hook_with_context(config, ts, hook, shell, None).await
}

/// Run a hook with optional installed tools context (for postinstall hooks)
#[async_backtrace::framed]
pub async fn run_one_hook_with_context(
    config: &Arc<Config>,
    ts: &Toolset,
    hook: Hooks,
    shell: Option<&dyn Shell>,
    installed_tools: Option<&[InstalledToolInfo]>,
) {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    let shell_name = shell.map(|s| s.to_string()).unwrap_or_default();
    for (root, h) in all_hooks(config).await {
        if hook != h.hook || !matches_shell(h, &shell_name) {
            continue;
        }
        trace!("running hook {hook} in {root:?}");
        // Global hooks skip directory matching — they fire for all projects
        if !h.global {
            match (hook, hook_env::dir_change()) {
                (Hooks::Enter, Some((old, new))) => {
                    if !new.starts_with(root) {
                        continue;
                    }
                    if old.as_ref().is_some_and(|old| old.starts_with(root)) {
                        continue;
                    }
                }
                (Hooks::Leave, Some((old, new))) => {
                    if new.starts_with(root) {
                        continue;
                    }
                    if old.as_ref().is_some_and(|old| !old.starts_with(root)) {
                        continue;
                    }
                }
                (Hooks::Cd, Some((_old, new))) if !new.starts_with(root) => {
                    continue;
                }
                // Pre/postinstall hooks only run if CWD is under the config root
                (Hooks::Preinstall | Hooks::Postinstall, _) => {
                    if let Some(cwd) = dirs::CWD.as_ref()
                        && !cwd.starts_with(root)
                    {
                        continue;
                    }
                }
                _ => {}
            }
        }
        run_matched_hook(config, ts, root, h, shell, installed_tools).await;
    }
}

pub async fn run_enter_hooks_for_newly_loaded_configs(
    config: &Arc<Config>,
    ts: &Toolset,
    shell: &dyn Shell,
) {
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    if hook_env::dir_change().is_some() {
        return;
    }
    let Some(cwd) = dirs::CWD.as_ref() else {
        return;
    };
    let newly_loaded_roots = config
        .config_files
        .iter()
        .filter(|(path, _)| !hook_env::PREV_SESSION.loaded_configs.contains(*path))
        .filter_map(|(_, cf)| cf.project_root())
        .filter(|root| cwd.starts_with(root))
        .collect::<IndexSet<_>>();
    if newly_loaded_roots.is_empty() {
        return;
    }
    let shell_name = shell.to_string();
    for (root, h) in config.hooks().await.cloned().unwrap_or_default() {
        if h.hook != Hooks::Enter || h.global || !cwd.starts_with(&root) {
            continue;
        }
        if !matches_shell(&h, &shell_name) {
            continue;
        }
        if !newly_loaded_roots.contains(&root) {
            continue;
        }
        run_matched_hook(config, ts, &root, &h, Some(shell), None).await;
    }
}

async fn run_matched_hook(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    hook: &Hook,
    shell: Option<&dyn Shell>,
    installed_tools: Option<&[InstalledToolInfo]>,
) {
    let hook_type = hook.hook;
    match &hook.action {
        HookAction::Task { task_name } => {
            if let Err(e) = execute_task(config, ts, root, hook, task_name, installed_tools).await {
                warn!("{hook_type} hook in {} failed: {e}", root.display());
            }
        }
        HookAction::CurrentShell { script, .. } => {
            if let Some(shell) = shell {
                // Set hook environment variables so shell hooks can access them
                println!(
                    "{}",
                    shell.set_env("MISE_PROJECT_ROOT", &root.to_string_lossy())
                );
                println!(
                    "{}",
                    shell.set_env("MISE_CONFIG_ROOT", &root.to_string_lossy())
                );
                if let Some(cwd) = dirs::CWD.as_ref() {
                    println!(
                        "{}",
                        shell.set_env("MISE_ORIGINAL_CWD", &cwd.to_string_lossy())
                    );
                }
                if let Some((Some(old), _new)) = hook_env::dir_change() {
                    println!(
                        "{}",
                        shell.set_env("MISE_PREVIOUS_DIR", &old.to_string_lossy())
                    );
                }
                if let Some(tools) = installed_tools
                    && let Ok(json) = serde_json::to_string(tools)
                {
                    println!("{}", shell.set_env("MISE_INSTALLED_TOOLS", &json));
                }
            }
            println!("{script}");
        }
        HookAction::Run { .. } => {
            if let Err(e) = execute(config, ts, root, hook, installed_tools).await {
                // Warn but continue running remaining hooks of this type
                warn!("{hook_type} hook in {} failed: {e}", root.display());
            }
        }
    }
}

fn matches_shell(hook: &Hook, shell_name: &str) -> bool {
    if let HookAction::CurrentShell { shell, .. } = &hook.action {
        shell == shell_name
    } else {
        true
    }
}

async fn execute(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    hook: &Hook,
    installed_tools: Option<&[InstalledToolInfo]>,
) -> Result<()> {
    let HookAction::Run {
        run,
        run_windows,
        shell,
        legacy_script,
        ignored_shell,
    } = &hook.action
    else {
        return Ok(());
    };
    if *legacy_script {
        deprecated_at!(
            "2026.9.0",
            "2027.3.0",
            "hook_script_table_spawned_run",
            "hook tables using `script` for spawned commands are deprecated. Use `run` instead."
        );
    }
    if ignored_shell.is_some() && matches!(hook.hook, Hooks::Preinstall | Hooks::Postinstall) {
        let hook_name = hook.hook.to_string().to_lowercase();
        warn!(
            "`shell` is ignored for {} hooks that use `script`/`scripts`; use `run = ...` with `shell = \"bash -c\"` to choose an inline shell command.",
            hook_name
        );
    }
    let run = if cfg!(windows) {
        run_windows.as_deref().or(run.as_deref())
    } else {
        run.as_deref()
    };
    let Some(run) = run else {
        return Ok(());
    };
    let shell = shell
        .as_ref()
        .map(|shell| crate::path::split_shell_command(shell))
        .transpose()?
        .unwrap_or(Settings::get().default_inline_shell()?);

    // Preinstall hooks skip `tools=true` env directives since the tools
    // providing those env vars aren't installed yet (fixes #6162)
    let (tera_ctx, mut env) = if hook.hook == Hooks::Preinstall {
        let env = ts.full_env_without_tools(config).await?;
        let ctx = if contains_template_syntax(run) {
            let mut ctx = config.tera_ctx.clone();
            ctx.insert("env", &env);
            Some(ctx)
        } else {
            None
        };
        (ctx, env)
    } else {
        let env = ts.full_env(config).await?;
        let ctx = if contains_template_syntax(run) {
            Some(ts.tera_ctx(config).await?.clone())
        } else {
            None
        };
        (ctx, env)
    };
    let rendered_script = if let Some(tera_ctx) = tera_ctx {
        let mut tera = get_tera(Some(root));
        render_str(&mut tera, run, &tera_ctx)?
    } else {
        run.to_string()
    };

    let args = shell
        .iter()
        .skip(1)
        .map(|s| s.as_str())
        .chain(once(rendered_script.as_str()))
        .collect_vec();
    if let Some(cwd) = dirs::CWD.as_ref() {
        env.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    env.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    if let Some((Some(old), _new)) = hook_env::dir_change() {
        env.insert(
            "MISE_PREVIOUS_DIR".to_string(),
            old.to_string_lossy().to_string(),
        );
    }
    // Add installed tools info for postinstall hooks
    if let Some(tools) = installed_tools
        && let Ok(json) = serde_json::to_string(tools)
    {
        env.insert("MISE_INSTALLED_TOOLS".to_string(), json);
    }
    env.insert(
        "MISE_CONFIG_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    // Prevent recursive hook execution (e.g. hook runs `mise run` which spawns
    // a shell that activates mise and re-triggers hooks)
    env.insert("MISE_NO_HOOKS".to_string(), "1".to_string());

    // On Windows, when the hook shell is cmd.exe, the rendered command must be
    // passed to cmd *verbatim*. Going through std/duct's MSVCRT-style quoting
    // would escape inner `"` as `\"`, which cmd.exe does not understand, so a
    // hook like `python -c "import x"` is mangled. Unlike tasks, hooks have no
    // `usage`/shebang/file-based escape hatch, so this is the only fix. duct
    // can't emit raw args, so spawn std Command directly with raw command-line
    // args (wrapped in one outer quote pair + `/s`), forwarding stdout to stderr
    // to match the duct path's `stdout_to_stderr()`. See discussion #9355.
    #[cfg(windows)]
    {
        let runs_command = shell
            .iter()
            .skip(1)
            .any(|f| f.eq_ignore_ascii_case("/c") || f.eq_ignore_ascii_case("/k"));
        if crate::path::is_cmd_shell_program(Path::new(&shell[0])) && runs_command {
            use std::os::windows::io::AsHandle;
            use std::os::windows::process::CommandExt;
            let cmd_args = crate::path::cmd_verbatim_args(&shell[1..], &rendered_script, &[]);
            trace!("hook (cmd verbatim): {} {}", shell[0], cmd_args.join(" "));
            let mut c = std::process::Command::new(&shell[0]);
            for a in &cmd_args {
                c.raw_arg(a);
            }
            c.env_clear();
            c.envs(env.iter());
            // Send the hook's stdout to mise's stderr (matching the duct
            // `stdout_to_stderr()` the non-cmd path uses) by handing the child a
            // clone of our stderr handle. Redirecting the descriptor directly —
            // rather than piping through a reader thread — means a hook that
            // spawns a background child holding the write end can't block us
            // waiting for pipe EOF, and stdout/stderr ordering is preserved.
            c.stdout(std::io::stderr().as_handle().try_clone_to_owned()?);
            let status = c.status()?;
            if !status.success() {
                eyre::bail!("hook command failed: {status}");
            }
            return Ok(());
        }
    }

    cmd(&shell[0], args)
        .stdout_to_stderr()
        // .dir(root)
        .full_env(env)
        .run()?;
    Ok(())
}

async fn execute_task(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    hook: &Hook,
    task_name: &str,
    installed_tools: Option<&[InstalledToolInfo]>,
) -> Result<()> {
    let mise_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mise"));

    let mut env = if hook.hook == Hooks::Preinstall {
        ts.full_env_without_tools(config).await?
    } else {
        ts.full_env(config).await?
    };
    if let Some(cwd) = dirs::CWD.as_ref() {
        env.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    env.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    env.insert(
        "MISE_CONFIG_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    if let Some((Some(old), _new)) = hook_env::dir_change() {
        env.insert(
            "MISE_PREVIOUS_DIR".to_string(),
            old.to_string_lossy().to_string(),
        );
    }
    if let Some(tools) = installed_tools
        && let Ok(json) = serde_json::to_string(tools)
    {
        env.insert("MISE_INSTALLED_TOOLS".to_string(), json);
    }
    env.insert("MISE_NO_HOOKS".to_string(), "1".to_string());

    cmd(
        mise_bin,
        ["--cd", &root.to_string_lossy(), "run", task_name],
    )
    .stdout_to_stderr()
    .full_env(env)
    .run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct TestHook {
        hook: HookDef,
    }

    #[test]
    fn run_table_supports_run_windows() {
        let parsed: TestHook = toml::from_str(
            r#"
            hook = { run = "echo unix", run_windows = "echo windows", shell = "bash -c" }
            "#,
        )
        .unwrap();
        let hooks = parsed.hook.into_hooks(Hooks::Postinstall);

        assert_eq!(hooks.len(), 1);
        match &hooks[0].action {
            HookAction::Run {
                run,
                run_windows,
                shell,
                ..
            } => {
                assert_eq!(run.as_deref(), Some("echo unix"));
                assert_eq!(run_windows.as_deref(), Some("echo windows"));
                assert_eq!(shell.as_deref(), Some("bash -c"));
            }
            action => panic!("expected run hook, got {action:?}"),
        }
    }
}
