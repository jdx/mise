//! What a task asks for: its `secrets = [...]` list, and the checks that need no source.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use serde::de::{self, Deserializer, SeqAccess, Visitor};

use super::SecretName;
use crate::task::{Task, TaskKey, TaskRunPhase, task_key};

pub(crate) const G4_TRUE: &str = "secrets = true is not supported; tasks receive only the secrets they list, e.g. secrets = [\"DEPLOY_KEY\"]; grant for one run with mise run --secrets-all <task> or --secrets KEY";
pub(crate) const G4_FALSE: &str =
    "secrets = false is not needed: tasks receive no secrets unless they list them";
pub(crate) const G5_TEXT: &str = "per-task source options (secrets = { fnox = ... }) are not supported yet; list key names: secrets = [\"DEPLOY_KEY\"]";
pub const G7_TEXT: &str = "secrets is not allowed in [task_templates.<t>] or monorepo.task_defaults; list secrets on each task so every grant is visible on the task and in mise secrets ls";

/// String or list of names. Shape errors fail at parse (G4/G5), like any wrong-typed task
/// field. Name validity (G3/G6) is checked in preflight so a typo cannot break hook-env.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TaskSecrets(pub Vec<String>);

impl TaskSecrets {
    pub fn names(&self) -> &[String] {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for TaskSecrets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = TaskSecrets;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("secrets must be a key name or a list of key names")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                Ok(TaskSecrets(vec![s.to_string()]))
            }
            fn visit_bool<E: de::Error>(self, b: bool) -> Result<Self::Value, E> {
                Err(E::custom(if b { G4_TRUE } else { G4_FALSE }))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
                Err(de::Error::custom(G5_TEXT))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = vec![];
                loop {
                    match seq.next_element::<toml::Value>() {
                        Ok(Some(toml::Value::String(s))) => out.push(s),
                        Ok(Some(_)) | Err(_) => {
                            return Err(de::Error::custom("secrets entries must be key names"));
                        }
                        Ok(None) => break,
                    }
                }
                Ok(TaskSecrets(out))
            }
        }
        d.deserialize_any(V)
    }
}

/// Launchers that start tasks without a person asking for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretsDenied {
    Hook,
    /// A hook that runs in the user's own shell (`shell = ...`).
    ShellHook,
    WatchFiles,
    PitchforkDaemon,
    Bootstrap,
}

impl SecretsDenied {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::ShellHook => "shell_hook",
            Self::WatchFiles => "watch_files",
            Self::PitchforkDaemon => "pitchfork_daemon",
            Self::Bootstrap => "bootstrap",
        }
    }

    pub(crate) fn launcher(self) -> &'static str {
        match self {
            Self::Hook => "a mise hook",
            Self::ShellHook => "a mise shell hook",
            Self::WatchFiles => "watch_files",
            Self::PitchforkDaemon => "a pitchfork daemon",
            Self::Bootstrap => "mise bootstrap",
        }
    }

    /// Parses `__MISE_SECRETS_DENIED`. Any other non-empty value fails closed.
    pub(crate) fn from_marker(value: &str) -> Option<Self> {
        match value {
            "" => None,
            "shell_hook" => Some(Self::ShellHook),
            "watch_files" => Some(Self::WatchFiles),
            "pitchfork_daemon" => Some(Self::PitchforkDaemon),
            "bootstrap" => Some(Self::Bootstrap),
            _ => Some(Self::Hook),
        }
    }
}

pub(crate) const DENIED_MARKER: &str = "__MISE_SECRETS_DENIED";

/// `__MISE_SECRETS_DENIED` first (an unknown value fails closed), then the pitchfork daemon
/// marker.
pub(crate) fn denied_from_env() -> Option<SecretsDenied> {
    std::env::var(DENIED_MARKER)
        .ok()
        .and_then(|v| SecretsDenied::from_marker(&v))
        .or_else(|| {
            mise_util::env::var_is_true(crate::daemons::DAEMON_TASK_MARKER)
                .then_some(SecretsDenied::PitchforkDaemon)
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GrantOrigin {
    /// `secrets = [...]` on the task itself
    TaskList { file: PathBuf },
    /// `--secrets KEY` on the command line
    CliFlag,
    /// `--secrets-all` on the command line
    CliAll,
    /// `{{ secrets.X }}` in the value of the task's own `env.<var>`
    Template { var: String, file: PathBuf },
}

impl GrantOrigin {
    /// A grant the task's own config wrote, which is the only kind the G8/G9 rules police.
    fn is_self(&self) -> bool {
        matches!(self, Self::TaskList { .. } | Self::Template { .. })
    }

    /// Whether the grant exports the key itself. A reference in an env value only reads it.
    fn exports(&self) -> bool {
        !matches!(self, Self::Template { .. })
    }

    fn describe(&self, key: &SecretName) -> String {
        match self {
            Self::Template { var, .. } => format!("{{{{ secrets.{key} }}}} in env.{var}"),
            Self::TaskList { file } => format!(
                "secrets = [...] in {}",
                file.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
            Self::CliFlag => "--secrets".to_string(),
            Self::CliAll => "--secrets-all".to_string(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SecretGrant {
    pub(crate) keys: BTreeMap<SecretName, Vec<GrantOrigin>>,
    /// Every key the source can inject (`--secrets-all`). The keys are only known once the
    /// source is described, so they are not listed in `keys`.
    pub(crate) all: Option<GrantOrigin>,
    /// The task's own env values that use `{{ secrets.X }}`. Each is rendered once its
    /// references are resolved and exported as `key`. Their references are in `keys`.
    pub(crate) late: Vec<super::LateSecretEnv>,
}

impl SecretGrant {
    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.all.is_none() && self.late.is_empty()
    }

    /// Whether the grant itself hands `key` over under its own name. A key that only an env
    /// value references is not exported by the grant, so it never meets the listed-key checks;
    /// under `--secrets-all` it is still handed over by the `all` path in `grant_values`, after
    /// the C4 skips.
    pub(crate) fn exports(&self, key: &SecretName) -> bool {
        self.keys
            .get(key)
            .is_some_and(|origins| origins.iter().any(GrantOrigin::exports))
    }

    /// The keys handed over under their own names.
    pub(crate) fn exported_keys(&self) -> impl Iterator<Item = &SecretName> {
        self.keys.keys().filter(|k| self.exports(k))
    }

    /// Every key the task names, once per way it names it (`mise secrets ls`).
    pub(crate) fn inventory_uses(&self) -> Vec<super::InventoryUse> {
        let mut uses = vec![];
        for (key, origins) in &self.keys {
            let mut list = false;
            for origin in origins {
                match origin {
                    GrantOrigin::Template { var, .. } => uses.push(super::InventoryUse {
                        key: key.to_string(),
                        via: super::UseVia::Template { var: var.clone() },
                    }),
                    _ if !list => {
                        list = true;
                        uses.push(super::InventoryUse {
                            key: key.to_string(),
                            via: super::UseVia::List,
                        });
                    }
                    _ => {}
                }
            }
        }
        uses
    }

    /// Whether `key` is built from secrets by one of the task's env values.
    pub(crate) fn is_late_key(&self, key: &str) -> bool {
        self.late
            .iter()
            .any(|l| mise_util::env::env_key_eq(&l.key, key))
    }

    /// Whether the task's own config grants anything. A command-line grant is the person
    /// running mise asking, so it never trips the rules about where a task may be defined.
    pub(crate) fn has_self_grant(&self) -> bool {
        self.keys.values().flatten().any(GrantOrigin::is_self)
    }

    /// `other`'s origins are added to ours; its `all` wins.
    pub(crate) fn merged(mut self, other: SecretGrant) -> SecretGrant {
        for (key, origins) in other.keys {
            let mine = self.keys.entry(key).or_default();
            for origin in origins {
                if !mine.contains(&origin) {
                    mine.push(origin);
                }
            }
        }
        self.all = other.all.or(self.all);
        for late in other.late {
            if !self.late.contains(&late) {
                self.late.push(late);
            }
        }
        self
    }

    /// Where `key` came from, for G1 and G2.
    pub(crate) fn granted_by(&self, key: &SecretName) -> String {
        let mut by: Vec<String> = self
            .keys
            .get(key)
            .into_iter()
            .flatten()
            .map(|o| o.describe(key))
            .collect();
        by.sort();
        by.dedup();
        by.join(", ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    Unknown,
    NotInjectable,
    InvalidName,
    Wildcard,
    Remote,
    NotProject,
    Denied,
    Collision,
    Reserved,
    Sandbox,
    Template,
    Source,
    NoProcess,
}

impl ProblemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::NotInjectable => "not_injectable",
            Self::InvalidName => "invalid_name",
            Self::Wildcard => "wildcard",
            Self::Remote => "remote",
            Self::NotProject => "not_project_config",
            Self::Denied => "denied_launcher",
            Self::Collision => "collision",
            Self::Reserved => "reserved",
            Self::Sandbox => "sandbox",
            Self::Template => "template",
            Self::Source => "source",
            Self::NoProcess => "no_process",
        }
    }
}

/// One thing wrong with a grant. `text` is the first line, which starts with the task name;
/// `detail` lines are indented under it.
#[derive(Clone, Debug)]
pub struct Problem {
    pub task: String,
    pub key: Option<String>,
    pub kind: ProblemKind,
    pub text: String,
    pub detail: Vec<String>,
    pub suggestion: Option<String>,
}

impl Problem {
    pub(crate) fn new(task: &str, key: Option<&str>, kind: ProblemKind, text: String) -> Self {
        Self {
            task: task.to_string(),
            key: key.map(String::from),
            kind,
            text,
            detail: vec![],
            suggestion: None,
        }
    }

    pub(crate) fn detail(mut self, line: impl Into<String>) -> Self {
        self.detail.push(line.into());
        self
    }

    pub(crate) fn suggestion(mut self, s: Option<String>) -> Self {
        self.suggestion = s;
        self
    }

    /// For a single error.
    pub fn render(&self) -> String {
        let mut out = self.text.clone();
        for line in &self.detail {
            out.push_str("\n  ");
            out.push_str(line);
        }
        out
    }

    fn render_indented(&self, indent: &str) -> String {
        let mut out = format!("{indent}{}", self.text);
        for line in &self.detail {
            out.push_str(&format!("\n{indent}  {line}"));
        }
        out
    }
}

/// G18: every problem, with nothing run first.
pub(crate) fn aggregate_error(problems: &[Problem]) -> eyre::Report {
    let mut out = format!("cannot grant secrets ({} problems):", problems.len());
    if problems.len() == 1 {
        out = "cannot grant secrets (1 problem):".to_string();
    }
    for p in problems {
        out.push('\n');
        out.push_str(&p.render_indented("  "));
    }
    out.push_str("\nRun `mise secrets ls` to see which keys tasks can receive.");
    eyre::eyre!("{out}")
}

/// Why a name cannot be granted: G3, G6 or G12.
pub(crate) enum NameError {
    Invalid,
    Wildcard,
    Reserved,
}

impl NameError {
    fn kind(&self) -> ProblemKind {
        match self {
            Self::Invalid => ProblemKind::InvalidName,
            Self::Wildcard => ProblemKind::Wildcard,
            Self::Reserved => ProblemKind::Reserved,
        }
    }

    /// The text after "<who>: ", shared by `secrets = [...]` and the command-line flags.
    fn text(&self, raw: &str, list: &str) -> String {
        match self {
            Self::Invalid => format!(
                "\"{raw}\" in {list} is not a valid environment variable name ([A-Za-z_][A-Za-z0-9_]*)"
            ),
            Self::Wildcard => format!(
                "\"{raw}\" in {list}: wildcards are not supported; list each key (see mise secrets ls)"
            ),
            Self::Reserved => {
                format!("secret name {raw} is reserved by mise and cannot be granted")
            }
        }
    }
}

pub(crate) fn validate_name(raw: &str) -> std::result::Result<SecretName, NameError> {
    if raw.contains('*') {
        return Err(NameError::Wildcard);
    }
    let name = SecretName::new(raw).ok_or(NameError::Invalid)?;
    if mise_util::env::is_reserved_secret_name(name.as_str()) {
        return Err(NameError::Reserved);
    }
    Ok(name)
}

/// G3, G6 and the grant of the valid names. Invalid names are reported, not granted. The
/// task's `{{ secrets.X }}` env values add their references: the reference is the grant.
pub(crate) fn grant_for_task(task: &Task) -> (SecretGrant, Vec<Problem>) {
    let mut grant = SecretGrant::default();
    let mut problems = vec![];
    let file = task.config_source.clone();
    if let Some(list) = &task.secrets {
        for raw in list.names() {
            let name = match validate_name(raw) {
                Ok(name) => name,
                Err(e) => {
                    problems.push(Problem::new(
                        &task.name,
                        Some(raw),
                        e.kind(),
                        format!("task {}: {}", task.name, e.text(raw, "secrets")),
                    ));
                    continue;
                }
            };
            let origins = grant.keys.entry(name).or_default();
            let origin = GrantOrigin::TaskList { file: file.clone() };
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
    }
    for late in task.live_late_secret_env() {
        // the exported name must survive the marker M2 reads back in a nested mise
        let key_problem = match validate_name(&late.key) {
            Ok(_) => None,
            Err(NameError::Reserved) => Some((
                ProblemKind::Reserved,
                super::template::reserved_key_message(&task.name, &late.key),
            )),
            Err(_) => Some((
                ProblemKind::InvalidName,
                super::template::invalid_key_message(&task.name, &late.key),
            )),
        };
        if let Some((kind, text)) = key_problem {
            problems.push(Problem::new(&task.name, Some(&late.key), kind, text));
            continue;
        }
        let mut usable = true;
        for name in &late.refs {
            if let Err(e) = validate_name(name.as_str()) {
                usable = false;
                problems.push(Problem::new(
                    &task.name,
                    Some(name.as_str()),
                    e.kind(),
                    format!(
                        "task {}: env.{}: {}",
                        task.name,
                        late.key,
                        e.text(name.as_str(), "{{ secrets.* }}")
                    ),
                ));
            }
        }
        if !usable {
            continue;
        }
        for name in &late.refs {
            let origins = grant.keys.entry(name.clone()).or_default();
            let origin = GrantOrigin::Template {
                var: late.key.clone(),
                file: late.file.clone(),
            };
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
        grant.late.push(late.clone());
    }
    (grant, problems)
}

/// What `mise run --secrets` and `--secrets-all` grant, and to which tasks: only the ones
/// named on the command line, never their dependencies or subtasks.
pub struct CliSecretGrant {
    keys: BTreeSet<SecretName>,
    all: bool,
    named: HashSet<TaskKey>,
}

impl CliSecretGrant {
    /// `named` is every task the command line selected (explicit names, globs, `default`,
    /// `--all`), before dependencies are resolved. Fails on a name that cannot be granted.
    pub fn new(keys: &[String], all: bool, named: &[Task]) -> eyre::Result<Self> {
        let mut names = BTreeSet::new();
        let mut errors = vec![];
        for raw in keys {
            match validate_name(raw) {
                Ok(name) => {
                    names.insert(name);
                }
                Err(e) => errors.push(format!("--secrets: {}", e.text(raw, "--secrets"))),
            }
        }
        if !errors.is_empty() {
            eyre::bail!("{}", errors.join("\n"));
        }
        Ok(Self {
            keys: names,
            all,
            named: named
                .iter()
                .map(|t| task_key(&t.clone().with_run_phase(TaskRunPhase::Normal)))
                .collect(),
        })
    }

    /// `injected` tasks come from a run entry of another task, so they never receive a
    /// command-line grant even when they look identical to a named one.
    pub(crate) fn for_task(&self, task: &Task, injected: bool) -> Option<SecretGrant> {
        if injected || !self.named.contains(&task_key(task)) {
            return None;
        }
        if orchestrates_only(task) {
            // nothing to inject into: it only starts other tasks, which are not named
            warn_once!(
                "--secrets: task {} starts no process of its own, so it receives nothing; name the tasks that run commands",
                task.name
            );
            return None;
        }
        Some(SecretGrant {
            keys: self
                .keys
                .iter()
                .map(|k| (k.clone(), vec![GrantOrigin::CliFlag]))
                .collect(),
            all: self.all.then_some(GrantOrigin::CliAll),
            late: vec![],
        })
    }
}

/// The task's own grant plus whatever the command line gave it. The one grant every
/// decision about the task (cache, dry run, preflight, spawn) must use.
pub(crate) fn effective_grant(
    task: &Task,
    cli: Option<&CliSecretGrant>,
    injected: bool,
) -> (SecretGrant, Vec<Problem>) {
    let (own, problems) = grant_for_task(task);
    match cli.and_then(|c| c.for_task(task, injected)) {
        Some(from_cli) => (own.merged(from_cli), problems),
        None => (own, problems),
    }
}

/// `--secrets` and `--secrets-all` for `mise x`.
pub(crate) fn exec_grant(keys: &[String]) -> eyre::Result<SecretGrant> {
    let mut grant = SecretGrant::default();
    let mut errors = vec![];
    for raw in keys {
        match validate_name(raw) {
            Ok(name) => {
                grant.keys.insert(name, vec![GrantOrigin::CliFlag]);
            }
            Err(e) => errors.push(format!("mise x --secrets: {}", e.text(raw, "--secrets"))),
        }
    }
    if !errors.is_empty() {
        eyre::bail!("{}", errors.join("\n"));
    }
    Ok(grant)
}

/// Who a grant is for, in the words of G1, G2, G11 and G13.
#[derive(Clone, Copy)]
pub(crate) enum Subject<'a> {
    Task(&'a str),
    Exec,
}

impl Subject<'_> {
    /// "task deploy" or "mise x"
    pub(crate) fn text(self) -> String {
        match self {
            Self::Task(name) => format!("task {name}"),
            Self::Exec => "mise x".to_string(),
        }
    }

    /// The name `Problem::task` carries.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Task(name) => name.to_string(),
            Self::Exec => "mise x".to_string(),
        }
    }

    fn this(self) -> &'static str {
        match self {
            Self::Task(_) => "this task",
            Self::Exec => "this command",
        }
    }
}

/// Everything that needs neither a source nor a process: G8, G9, G10 and G16 for the grant.
pub(crate) fn static_problems(
    task: &Task,
    grant: &SecretGrant,
    denied: Option<SecretsDenied>,
    view: &EnvView,
) -> Vec<Problem> {
    let mut problems = vec![];
    if grant.is_empty() {
        return problems;
    }
    // Only a task's own `secrets = [...]` is held to where the task is defined. A person who
    // names the task and its secrets on the command line is the one granting.
    let own = grant.has_self_grant();
    // what the task's own config does to ask: list keys, or name them in env values
    let asks = if grant
        .keys
        .values()
        .flatten()
        .any(|o| matches!(o, GrantOrigin::TaskList { .. }))
    {
        "list secrets"
    } else {
        "use {{ secrets.* }}"
    };
    if own && let Some(source) = task.secrets_remote_source() {
        problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::Remote,
            format!(
                "task {} comes from a remote source ({source}) and cannot {asks}; grant for one run with mise run --secrets-all {} or --secrets KEY",
                task.name, task.name
            ),
        ));
    }
    let project_root = task
        .config_root
        .as_deref()
        .is_some_and(|r| super::config::is_project_secrets_root(r, &crate::dirs::HOME));
    if own && task.secrets_remote_source().is_none() && (task.global || !project_root) {
        problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::NotProject,
            format!(
                "task {} is defined in {}, which is not project config (global or system config, or a file in or above your home directory), so it cannot {asks}; grant for one run with mise run --secrets-all {} or --secrets KEY",
                task.name,
                crate::file::display_path(&task.config_source),
                task.name
            ),
        ));
    }
    if let Some(denied) = denied {
        problems.push(
            Problem::new(
                &task.name,
                None,
                ProblemKind::Denied,
                format!(
                    "task {} {}, but it was started by {}",
                    task.name,
                    if own {
                        if asks == "list secrets" {
                            "lists secrets"
                        } else {
                            "uses {{ secrets.* }}"
                        }
                    } else {
                        "was granted secrets on the command line"
                    },
                    denied.launcher()
                ),
            )
            .detail(format!(
                "mise secrets are not available there in this version. Run it directly: mise run {}",
                task.name
            )),
        );
        if denied == SecretsDenied::ShellHook
            && let Some(p) = problems.last_mut()
        {
            // only a shell hook can leave a stale mark: it returned early or was interrupted
            p.detail.push(
                "If no hook is running, an interrupted shell hook left __MISE_SECRETS_DENIED set in this shell; open a new shell or unset it."
                    .to_string(),
            );
        }
    }
    // a command-line grant to such a task is dropped with a warning instead (`for_task`)
    if own && orchestrates_only(task) {
        problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::NoProcess,
            format!(
                "task {} lists secrets but starts no process of its own; list them on the tasks that run commands",
                task.name
            ),
        ));
    }
    // a key that the task exports under its own name is not in the environment when a
    // template is rendered
    let granted: BTreeSet<&str> = grant.exported_keys().map(|k| k.as_str()).collect();
    let mut run_refs = BTreeSet::new();
    for script in task.run_script_strings() {
        run_refs.extend(tera_env_refs(&script));
    }
    let env_texts = task.non_late_env_texts();
    for key in view.template_refs(task) {
        // a key built from secrets is read from env only through T5 below
        let late = grant.is_late_key(&key) && run_refs.contains(&key);
        if granted.contains(key.as_str()) || late {
            problems.push(
                Problem::new(
                    &task.name,
                    Some(&key),
                    ProblemKind::Template,
                    format!(
                        "task {}: {{{{ env.{key} }}}} in run cannot see the secret {key}",
                        task.name
                    ),
                )
                .detail(
                    "Secrets are added to the task's environment when it starts, after run is rendered. Read it from the environment instead: \"$".to_string()
                        + &key
                        + "\"",
                ),
            );
        }
    }
    problems.extend(tera_read_problems(task, grant, &env_texts));
    problems.extend(shell_expansion_problems(
        task,
        grant,
        &env_texts,
        crate::config::Settings::try_get().is_ok_and(|s| s.env_shell_expand),
    ));
    problems
}

/// What `env_shell_expand` means for a task with composed values. With it on, mise expands
/// `$NAME` in other env values, so a read of a composed key (T5) is detected through the
/// expander, and the literal text of a composed value may not hold `$` syntax, because mise
/// does not expand after substituting a secret. With it off a `$` is literal.
fn shell_expansion_problems(
    task: &Task,
    grant: &SecretGrant,
    env_texts: &[(String, String)],
    expand: bool,
) -> Vec<Problem> {
    let mut problems = vec![];
    if !expand || grant.late.is_empty() {
        return problems;
    }
    for late in &grant.late {
        if super::template::literal_has_shell_expansion(&late.template) {
            problems.push(Problem::new(
                &task.name,
                Some(&late.key),
                ProblemKind::Template,
                super::template::shell_expansion_message(&task.name, &late.key),
            ));
        }
    }
    problems.extend(shell_read_problems(task, grant, env_texts, expand));
    problems
}

/// T5: an env value reads, through Tera, a key that is built from secrets when the task
/// starts. Names only the env key and the composed key, never a text.
fn tera_read_problems(
    task: &Task,
    grant: &SecretGrant,
    env_texts: &[(String, String)],
) -> Vec<Problem> {
    let mut problems = vec![];
    for (name, value) in env_texts {
        let (refs, dynamic) = tera_env_scan(value);
        if dynamic {
            // the name is only known when it renders, so any composed key could be the one
            for late in &grant.late {
                problems.push(Problem::new(
                    &task.name,
                    Some(&late.key),
                    ProblemKind::Template,
                    format!(
                        "task {}: env.{name} calls get_env() with a name that is not a string literal, so it may read {}, which is rendered from secrets when the task starts; use a literal name (get_env(name=\"X\")) or build {name} from secrets directly",
                        task.name, late.key
                    ),
                ));
            }
        }
        for key in refs.iter().filter(|k| grant.is_late_key(k)) {
            problems.push(Problem::new(
                &task.name,
                Some(key),
                ProblemKind::Template,
                format!(
                    "task {}: env.{name} uses {{{{ env.{key} }}}}, but {key} is rendered from secrets when the task starts; build {name} from secrets directly",
                    task.name
                ),
            ));
        }
    }
    problems
}

/// T5 through a shell-style `$KEY` that `env_shell_expand` expands. Same rule on names only.
fn shell_read_problems(
    task: &Task,
    grant: &SecretGrant,
    env_texts: &[(String, String)],
    expand: bool,
) -> Vec<Problem> {
    let mut problems = vec![];
    if !expand || grant.late.is_empty() {
        return problems;
    }
    for (name, value) in env_texts.iter().filter(|(_, v)| v.contains('$')) {
        for late in &grant.late {
            let key = &late.key;
            if name == key {
                continue;
            }
            let sentinel = format!("\u{1}{key}\u{1}");
            let vars = BTreeMap::from([(key.clone(), sentinel.clone())]);
            let expanded =
                crate::config::env_directive::shell_expand_env(value, &vars, &mut vec![]);
            if expanded.contains(&sentinel) {
                problems.push(Problem::new(
                    &task.name,
                    Some(key),
                    ProblemKind::Template,
                    format!(
                        "task {}: env.{name} uses ${key}, but {key} is rendered from secrets when the task starts; build {name} from secrets directly",
                        task.name
                    ),
                ));
            }
        }
    }
    problems
}

/// Both T5 checks over texts that are not the task's own config text.
pub(crate) fn composed_read_problems(
    task: &Task,
    grant: &SecretGrant,
    texts: &[(String, String)],
    expand: bool,
) -> Vec<Problem> {
    let mut problems = tera_read_problems(task, grant, texts);
    problems.extend(shell_read_problems(task, grant, texts, expand));
    problems
}

/// T5 for age-encrypted env values: mise renders the plaintext after decrypting it, so it
/// can read a composed key, and only a decrypted text shows that. Nothing is returned unless
/// the task composes a value and has an age value, a decrypt error is left for the render to
/// report (`age.strict`), and the plaintext lives in a local that is dropped here. The
/// problems name env keys only.
pub(crate) async fn age_read_problems(task: &Task, grant: &SecretGrant) -> Vec<Problem> {
    use crate::config::env_directive::EnvDirective;
    if grant.late.is_empty() {
        return vec![];
    }
    let mut texts = vec![];
    for (directive, _) in task.render_env_directives() {
        if let EnvDirective::Age { key, .. } = &directive
            && let Ok(plain) = crate::agecrypt::decrypt_age_directive(&directive).await
        {
            texts.push((key.clone(), plain));
        }
    }
    composed_read_problems(
        task,
        grant,
        &texts,
        crate::config::Settings::try_get().is_ok_and(|s| s.env_shell_expand),
    )
}

/// G1 and G2 for a grant, against what the source describes.
pub(crate) fn key_problems(
    subject: Subject<'_>,
    grant: &SecretGrant,
    catalog: &super::Catalog,
    source_label: &str,
) -> Vec<Problem> {
    let who = subject.text();
    let name = subject.label();
    let mut problems = vec![];
    for key in grant.keys.keys() {
        match catalog.entries.get(key) {
            None => {
                let suggestion = suggest(key.as_str(), catalog.entries.keys().map(|k| k.as_str()));
                let mut hint = format!("{source_label} has no secret named {key}.");
                if let Some(s) = &suggestion {
                    hint.push_str(&format!(" Did you mean {s}?"));
                }
                problems.push(
                    Problem::new(
                        &name,
                        Some(key.as_str()),
                        ProblemKind::Unknown,
                        format!("{who}: unknown secret {key}"),
                    )
                    .detail(hint)
                    .detail(format!("Granted by: {}", grant.granted_by(key)))
                    .detail("See the available keys with `mise secrets ls`.")
                    .suggestion(suggestion),
                );
            }
            Some(entry) if !entry.injectable => {
                problems.push(
                    Problem::new(
                        &name,
                        Some(key.as_str()),
                        ProblemKind::NotInjectable,
                        format!("{who}: {key} cannot be injected"),
                    )
                    .detail(format!(
                        "fnox config sets env = false for {key}, so fnox never hands it to processes. Read it with `fnox get {key}` in your script, or set env = \"exec\" for it in fnox.toml if processes should receive it."
                    ))
                    .detail(format!("Granted by: {}", grant.granted_by(key))),
                );
            }
            Some(_) => {}
        }
    }
    problems
}

/// A close name: transposition-aware edit distance of at most 2, else a fuzzy match.
pub(crate) fn suggest<'a>(typo: &str, names: impl Iterator<Item = &'a str>) -> Option<String> {
    use crate::fuzzy::{FuzzyMatcher, FuzzyPattern};
    let names: Vec<&str> = names.collect();
    let typo_lc = typo.to_lowercase();
    let near = names
        .iter()
        .map(|n| (osa_distance(&typo_lc, &n.to_lowercase()), *n))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d);
    if let Some((_, n)) = near {
        return Some(n.to_string());
    }
    let mut matcher = FuzzyMatcher::default();
    let pattern = FuzzyPattern::new(&typo_lc);
    names
        .iter()
        .filter_map(|n| {
            matcher
                .score_pattern(&n.to_lowercase(), &pattern)
                .map(|score| (score, *n))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, n)| n.to_string())
}

fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// A task whose `run` only injects other tasks and that has no file: it would resolve secrets
/// and then hold them (and any secret files) while ungranted tasks run. It is refused instead.
/// A task with no run entries at all is skipped without resolving anything.
pub(crate) fn orchestrates_only(task: &Task) -> bool {
    use crate::task::RunEntry;
    task.file.is_none()
        && !task.run().is_empty()
        && !task.run().iter().any(|e| matches!(e, RunEntry::Script(_)))
}

/// G13: the sandbox would drop the granted key.
pub(crate) fn sandbox_problem(subject: Subject<'_>, key: &str) -> Problem {
    let who = subject.text();
    let hint = match subject {
        Subject::Task(_) => {
            format!("add allow_env = [\"{key}\"] to the task or pass --allow-env {key}")
        }
        Subject::Exec => format!("pass --allow-env {key}"),
    };
    Problem::new(
        &subject.label(),
        Some(key),
        ProblemKind::Sandbox,
        format!("{who} is granted {key}, but its sandbox denies env vars; {hint}"),
    )
}

/// G11: mise itself sets the granted key for this task or command.
pub(crate) fn collision_problem(subject: Subject<'_>, key: &str) -> Problem {
    let who = subject.text();
    let this = subject.this();
    let (sets, starts) = match subject {
        Subject::Task(_) => ("([env], the task's env, a tool, or a setting)", "the task"),
        Subject::Exec => ("([env], a tool, or a setting)", "the command"),
    };
    Problem::new(
        &subject.label(),
        Some(key),
        ProblemKind::Collision,
        format!("{who}: {key} is both a secret and a mise env var"),
    )
    .detail(format!(
        "mise sets {key} for {this} {sets}. Tools {starts} starts through mise shims recompute it and would replace the secret."
    ))
    .detail(format!(
        "Keep one: move the default into fnox.toml ({key} = {{ ..., default = \"...\" }}) or rename the mise variable."
    ))
}

/// G13 and G11 from what is known before anything runs: the sandbox, and the env names the
/// config and the task's other env declare. A key a composed value builds is exported under
/// its own name, so it is checked like an exported one; the spawn repeats the exact check.
pub(crate) fn sandbox_and_collision_problems(
    task: &Task,
    grant: &SecretGrant,
    sandbox: &crate::sandbox::SandboxConfig,
    declared: &BTreeSet<String>,
) -> Vec<Problem> {
    let subject = Subject::Task(&task.name);
    let mut problems = vec![];
    for key in grant
        .exported_keys()
        .map(|k| k.as_str())
        .chain(grant.late.iter().map(|l| l.key.as_str()))
    {
        if !sandbox.keeps_env_key(key) {
            problems.push(sandbox_problem(subject, key));
        }
    }
    let is_declared = |key: &str| declared.iter().any(|d| mise_util::env::env_key_eq(d, key));
    let mut colliding: BTreeSet<&str> = BTreeSet::new();
    for key in grant.exported_keys().map(|k| k.as_str()) {
        if grant.is_late_key(key) || is_declared(key) {
            colliding.insert(key);
        }
    }
    for late in &grant.late {
        if is_declared(&late.key) {
            colliding.insert(&late.key);
        }
    }
    for key in colliding {
        problems.push(collision_problem(subject, key));
    }
    problems
}

/// The environment task `[env]` directives are evaluated against, as the resolver sees it:
/// the process env minus what config `_.unset`s, plus config `[env]` results. Every static
/// check that depends on whether a `default` applies reads this one view, so they cannot drift
/// apart. The spawn repeats the exact check against the real environment and stays
/// authoritative.
#[derive(Default, Clone)]
pub(crate) struct EnvView {
    base: crate::env_diff::EnvMap,
    /// keys config `[env]` assigns (plain values and defaults that applied)
    config_keys: BTreeSet<String>,
    /// scripts were not run, so whether a task `default` applies is unknown: the static checks
    /// leave defaults to the spawn-time check
    defer_defaults: bool,
    /// At spawn: the keys whose task-env `default` directive the resolver reported as rendered
    /// (not yielded to a value). A `default` counts only if its key is among them; nothing is
    /// evaluated again, so no script runs twice.
    resolved_assigned: Option<BTreeSet<String>>,
}

impl EnvView {
    pub(crate) async fn load(config: &Arc<crate::config::Config>) -> Self {
        let mut base: crate::env_diff::EnvMap =
            crate::env::PRISTINE_ENV.clone().into_iter().collect();
        let mut config_keys = BTreeSet::new();
        if let Ok(results) = config.env_results().await {
            for key in &results.env_remove {
                base.retain(|k, _| !mise_util::env::env_key_eq(k, key));
            }
            for (key, (value, _)) in &results.env {
                base.insert(key.clone(), value.clone());
                config_keys.insert(key.clone());
            }
        }
        Self {
            base,
            config_keys,
            defer_defaults: false,
            resolved_assigned: None,
        }
    }

    /// The view at spawn, judged by what the task env preparation already resolved.
    pub(crate) fn resolved(rendered: &BTreeSet<String>) -> Self {
        Self {
            resolved_assigned: Some(rendered.clone()),
            ..Self::default()
        }
    }

    /// The view for `task`: a monorepo task's defaults are judged against its own config
    /// hierarchy's `[env]`, not the current project's. Everything else keeps `self`.
    /// `skip_scripts` keeps a static preflight from running `_.source` scripts (keys they
    /// touch are then unknown); the spawn-time check runs them, since the task env does too.
    pub(crate) async fn for_task(
        &self,
        config: &Arc<crate::config::Config>,
        ctx: &crate::task::task_context_builder::TaskContextBuilder,
        task: &Task,
        skip_scripts: bool,
    ) -> Self {
        let Some(task_cf) = task.cf.as_ref().filter(|_| !task.is_remote()) else {
            return self.clone();
        };
        let overlay = async {
            let ts = ctx
                .build_toolset_for_task(config, task, Some(task_cf), &[])
                .await?;
            ctx.config_env_for_source(config, task, &ts, skip_scripts)
                .await
        };
        match overlay.await {
            Ok(Some(env)) => {
                let mut view = self.with_config_env(env.values, env.unset);
                view.defer_defaults = env.skipped_scripts;
                view
            }
            _ => self.clone(),
        }
    }

    fn with_config_env(
        &self,
        values: std::collections::BTreeMap<String, String>,
        unset: BTreeSet<String>,
    ) -> Self {
        let mut view = self.clone();
        for key in &unset {
            view.base.retain(|k, _| !mise_util::env::env_key_eq(k, key));
            // an unset key is no longer assigned by config
            view.config_keys
                .retain(|k| !mise_util::env::env_key_eq(k, key));
        }
        for (key, value) in values {
            view.base.insert(key.clone(), value);
            view.config_keys.insert(key);
        }
        view
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_config_env(&self, values: &[(&str, &str)], unset: &[&str]) -> Self {
        self.with_config_env(
            values
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            unset.iter().map(|k| k.to_string()).collect(),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test(base: crate::env_diff::EnvMap, config_keys: &[&str]) -> Self {
        Self {
            base,
            config_keys: config_keys.iter().map(|k| k.to_string()).collect(),
            defer_defaults: false,
            resolved_assigned: None,
        }
    }

    /// Walks the task's env directives in the order the resolver applies them. Returns the
    /// keys they assign and the texts they render (a `default` only when it applies).
    fn walk(&self, task: &Task) -> (BTreeSet<String>, Vec<(String, String)>) {
        use crate::config::env_directive::EnvDirective;
        use mise_util::env::env_key_eq;
        let mut env = self.base.clone();
        let mut declared = BTreeSet::new();
        let mut texts = vec![];
        // in the order the env resolves, without the values rendered at spawn (a composite is
        // not something that could collide with itself, and a superseded one never applies)
        let directives = task.render_env_directives();
        for (d, _) in &directives {
            match d {
                EnvDirective::Val(k, v, _) => {
                    env.retain(|e, _| !env_key_eq(e, k));
                    // A templated value is not rendered here and may well render empty, which
                    // would not satisfy a later `default`. Treat it as unknown (empty) so the
                    // default is scanned: a conservative choice that can only add a refusal.
                    let known = if v.contains("{{") || v.contains("{%") {
                        String::new()
                    } else {
                        v.clone()
                    };
                    env.insert(k.clone(), known);
                    declared.insert(k.clone());
                    texts.push((k.clone(), v.clone()));
                }
                EnvDirective::Default(k, v, _) => {
                    let satisfied = env
                        .iter()
                        .any(|(e, val)| env_key_eq(e, k) && !val.is_empty());
                    let applies = match &self.resolved_assigned {
                        // assigned by the task env, and not by an earlier literal value of its own
                        Some(assigned) => !satisfied && assigned.iter().any(|a| env_key_eq(a, k)),
                        None => !satisfied && !self.defer_defaults,
                    };
                    if applies {
                        env.insert(k.clone(), v.clone());
                        declared.insert(k.clone());
                        texts.push((k.clone(), v.clone()));
                    }
                }
                EnvDirective::Rm(k, _) => env.retain(|e, _| !env_key_eq(e, k)),
                _ => {}
            }
        }
        (declared, texts)
    }

    /// Keys the task's env and the config's `[env]` set.
    pub(crate) fn declared_keys(&self, task: &Task) -> BTreeSet<String> {
        let (mut declared, _) = self.walk(task);
        declared.extend(self.config_keys.iter().cloned());
        declared
    }

    fn texts(&self, task: &Task) -> Vec<(String, String)> {
        self.walk(task).1
    }

    /// Env names the task's `run` scripts and `env` values read through a template. Those
    /// render before secrets exist, so a granted key among them would render without its value.
    pub(crate) fn template_refs(&self, task: &Task) -> BTreeSet<String> {
        let mut refs = BTreeSet::new();
        for script in task.run_script_strings() {
            refs.extend(tera_env_refs(&script));
        }
        for (_, value) in self.texts(task) {
            refs.extend(tera_env_refs(&value));
        }
        refs
    }
}

/// `{{ env.K }}`, `{{ env["K"] }}` and `get_env(name="K")` inside Tera tags. Lexical, but
/// aware of string literals: quoted text such as `"env.K"` is data and is not an env read.
pub(crate) fn tera_env_refs(s: &str) -> BTreeSet<String> {
    tera_env_scan(s).0
}

/// What `tera_env_refs` cannot name: a `get_env()` whose `name` is not a string literal
/// (`get_env(name=vars.KEY)`) reads some env var that is only known when it renders.
const DYNAMIC_GET_ENV: &str = "\u{0}get_env";

/// The names read, and whether any `get_env()` call computes its name.
pub(crate) fn tera_env_scan(s: &str) -> (BTreeSet<String>, bool) {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(start) = [rest.find("{{"), rest.find("{%"), rest.find("{#")]
        .into_iter()
        .flatten()
        .min()
    {
        let tag = &rest[start..];
        // Tera does not evaluate comments and raw blocks, so text in them reads nothing
        if let Some(after) = super::template::skip_inert(tag) {
            rest = after;
            continue;
        }
        if tag.starts_with("{#") {
            // an unterminated comment: scan it as text, since Tera rejects it anyway
            scan_tag(tag, &mut out);
            break;
        }
        let close = if tag.starts_with("{{") { "}}" } else { "%}" };
        let end = tag.find(close).map(|e| e + 2).unwrap_or(tag.len());
        scan_tag(&tag[..end], &mut out);
        rest = &tag[end..];
    }
    let dynamic = out.remove(DYNAMIC_GET_ENV);
    (out, dynamic)
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Reads a Tera string literal (no escapes) starting at the opening quote `chars[i]`.
/// Returns the contents and the index after the closing quote.
fn read_literal(chars: &[char], i: usize) -> (String, usize) {
    let q = chars[i];
    let mut j = i + 1;
    while j < chars.len() && chars[j] != q {
        j += 1;
    }
    let text = chars[i + 1..j.min(chars.len())].iter().collect();
    (text, (j + 1).min(chars.len()))
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}

fn scan_tag(tag: &str, out: &mut BTreeSet<String>) {
    let chars: Vec<char> = tag.chars().collect();
    let at = |i: usize, word: &str| {
        word.chars()
            .enumerate()
            .all(|(k, w)| chars.get(i + k) == Some(&w))
    };
    let mut i = 0;
    while i < chars.len() {
        // text inside a string literal is data, not an env read
        if is_quote(chars[i]) {
            i = read_literal(&chars, i).1;
            continue;
        }
        let boundary = i == 0 || !is_ident(chars[i - 1]);
        if boundary && at(i, "env") {
            let j = i + 3;
            if chars.get(j) == Some(&'.') {
                let s = j + 1;
                let mut e = s;
                while e < chars.len() && is_ident(chars[e]) {
                    e += 1;
                }
                if e > s {
                    out.insert(chars[s..e].iter().collect());
                }
                i = e;
                continue;
            }
            if chars.get(j) == Some(&'[') && chars.get(j + 1).is_some_and(|c| is_quote(*c)) {
                let (name, next) = read_literal(&chars, j + 1);
                out.insert(name);
                i = next;
                continue;
            }
        }
        if boundary && at(i, "get_env(") {
            // the key, then the main loop goes on through the other arguments, which are
            // expressions of their own (`default=env.K`)
            i += "get_env(".len();
            scan_get_env_name(&chars, i, out);
            continue;
        }
        i += 1;
    }
}

/// `name="K"` among the arguments of a `get_env(` call; strings elsewhere are skipped.
fn scan_get_env_name(chars: &[char], mut i: usize, out: &mut BTreeSet<String>) {
    while i < chars.len() && chars[i] != ')' {
        if is_quote(chars[i]) {
            i = read_literal(chars, i).1;
            continue;
        }
        let boundary = i == 0 || !is_ident(chars[i - 1]);
        if boundary && chars[i..].starts_with(&['n', 'a', 'm', 'e']) {
            let mut j = i + 4;
            while chars.get(j).is_some_and(|c| c.is_whitespace()) {
                j += 1;
            }
            if chars.get(j) == Some(&'=') {
                j += 1;
                while chars.get(j).is_some_and(|c| c.is_whitespace()) {
                    j += 1;
                }
                if chars.get(j).is_some_and(|c| is_quote(*c)) {
                    let (name, mut next) = read_literal(chars, j);
                    while chars.get(next).is_some_and(|c| c.is_whitespace()) {
                        next += 1;
                    }
                    // `'A' ~ 'B'` and the like are computed, not the literal `A`
                    if chars.get(next).is_none_or(|c| matches!(c, ',' | ')')) {
                        out.insert(name);
                    } else {
                        out.insert(DYNAMIC_GET_ENV.to_string());
                    }
                } else {
                    out.insert(DYNAMIC_GET_ENV.to_string());
                }
                return;
            }
        }
        i += 1;
    }
    // no `name=` at all: nothing to name
    out.insert(DYNAMIC_GET_ENV.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<TaskSecrets, String> {
        #[derive(serde::Deserialize)]
        struct W {
            secrets: TaskSecrets,
        }
        toml::from_str::<W>(s)
            .map(|w| w.secrets)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn parses_shapes() {
        assert_eq!(parse("secrets = \"A\"").unwrap().0, ["A"]);
        assert_eq!(parse("secrets = [\"A\", \"B\"]").unwrap().0, ["A", "B"]);
        assert!(parse("secrets = []").unwrap().0.is_empty());
        assert!(
            parse("secrets = true")
                .unwrap_err()
                .contains("secrets = true is not supported")
        );
        assert!(
            parse("secrets = false")
                .unwrap_err()
                .contains("secrets = false is not needed")
        );
        assert!(
            parse("secrets = { fnox = 1 }")
                .unwrap_err()
                .contains("per-task source options")
        );
        assert!(
            parse("secrets = [1]")
                .unwrap_err()
                .contains("secrets entries must be key names")
        );
        assert!(
            parse("secrets = 3")
                .unwrap_err()
                .contains("secrets must be a key name or a list of key names")
        );
    }

    #[test]
    fn finds_tera_env_refs() {
        let r = tera_env_refs(
            "echo {{ env.A }} {{ env[\"B\"] }} {{ get_env(name='C') }} {{ get_env(name=\"D\", default=\"x\") }} $E {{ other.F }} {{ environ.G }}",
        );
        assert_eq!(r, BTreeSet::from(["A", "B", "C", "D"].map(String::from)));
    }

    #[test]
    fn quoted_text_is_not_an_env_read() {
        for none in [
            "{% set label = \"env.DEPLOY_KEY\" %} echo {{ label }}",
            "{{ \"env.X\" }}",
            "{{ 'get_env(name=\"Y\")' }}",
        ] {
            assert!(tera_env_refs(none).is_empty(), "{none}");
        }
        let r =
            tera_env_refs("{{ env[\"B\"] }} {{ get_env(name='C') }} {% if env.KEY %}x{% endif %}");
        assert_eq!(r, BTreeSet::from(["B", "C", "KEY"].map(String::from)));
    }

    #[test]
    fn overlay_env_templates_are_checked() {
        use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions};
        let task = Task {
            name: "deploy".into(),
            secrets: Some(TaskSecrets(vec!["SECRET".into()])),
            overlay_env: vec![(
                EnvDirective::Val(
                    "TOKEN".into(),
                    "{{ env.SECRET }}".into(),
                    EnvDirectiveOptions::default(),
                ),
                PathBuf::from("/p/mise.toml"),
            )],
            ..Default::default()
        };
        let (grant, _) = grant_for_task(&task);
        let problems = static_problems(&task, &grant, None, &EnvView::default());
        assert!(
            problems.iter().any(|p| p.kind == ProblemKind::Template),
            "{problems:?}"
        );
    }

    #[test]
    fn default_values_are_checked_for_secret_reads() {
        use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions};
        let task = Task {
            name: "deploy".into(),
            secrets: Some(TaskSecrets(vec!["DEPLOY_KEY".into()])),
            env: crate::config::config_file::mise_toml::EnvList(vec![EnvDirective::Default(
                "TOKEN".into(),
                "{{ env.DEPLOY_KEY }}".into(),
                EnvDirectiveOptions::default(),
            )]),
            ..Default::default()
        };
        let (grant, _) = grant_for_task(&task);
        let problems = static_problems(&task, &grant, None, &EnvView::default());
        assert!(
            problems
                .iter()
                .any(|p| p.kind == ProblemKind::Template && !p.render().contains("s3cr3t")),
            "{problems:?}"
        );
    }

    fn default_task(default: &str) -> Task {
        use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions};
        Task {
            env: crate::config::config_file::mise_toml::EnvList(vec![EnvDirective::Default(
                "TOKEN".into(),
                default.into(),
                EnvDirectiveOptions::default(),
            )]),
            ..Default::default()
        }
    }

    fn map(pairs: &[(&str, &str)]) -> crate::env_diff::EnvMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// One view decides both checks: whether a default renders and whether it declares a key.
    #[test]
    fn defaults_follow_the_resolved_env_for_both_checks() {
        let task = default_task("{{ env.DEPLOY_KEY }}");
        let check = |view: EnvView, applies: bool| {
            assert_eq!(view.texts(&task).len(), usize::from(applies));
            assert_eq!(view.declared_keys(&task).contains("TOKEN"), applies);
        };
        // shell-only value: the default does not apply
        check(EnvView::for_test(map(&[("TOKEN", "ready")]), &[]), false);
        // config `[env] TOKEN = ""` over a shell value: the resolved env holds an empty TOKEN,
        // which does not satisfy the default
        check(EnvView::for_test(map(&[("TOKEN", "")]), &["TOKEN"]), true);
        // an empty value does not satisfy it; nor does an absent one
        check(EnvView::for_test(map(&[("TOKEN", "")]), &[]), true);
        check(EnvView::for_test(map(&[]), &[]), true);
        // config `[env]` assigned TOKEN before the task: the default does not apply (the key is
        // still declared, by config)
        let view = EnvView::for_test(map(&[("TOKEN", "from-config")]), &["TOKEN"]);
        assert!(view.texts(&task).is_empty());
        assert!(view.declared_keys(&task).contains("TOKEN"));
    }

    #[test]
    fn a_templated_earlier_value_may_render_empty() {
        use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions};
        let opts = EnvDirectiveOptions::default;
        let mut task = default_task("{{ env.DEPLOY_KEY }}");
        let view = EnvView::for_test(map(&[]), &[]);
        // TOKEN = "{{ '' }}" renders empty, so the default still applies
        task.env.0.insert(
            0,
            EnvDirective::Val("TOKEN".into(), "{{ '' }}".into(), opts()),
        );
        assert_eq!(view.texts(&task).len(), 2);
        // a plain non-empty literal satisfies it
        task.env.0[0] = EnvDirective::Val("TOKEN".into(), "plain".into(), opts());
        assert_eq!(view.texts(&task).len(), 1);
    }

    #[test]
    fn a_subproject_env_overlay_decides_the_default() {
        let task = default_task("{{ env.DEPLOY_KEY }}");
        let root = EnvView::for_test(map(&[]), &[]);
        // the subproject's [env] assigns TOKEN: the default does not apply
        let sub = root.for_test_with_config_env(&[("TOKEN", "x")], &[]);
        assert!(sub.texts(&task).is_empty());
        // the root view alone would have let it render
        assert_eq!(root.texts(&task).len(), 1);
        // the subproject unsets a shell TOKEN: the default applies
        let shell = EnvView::for_test(map(&[("TOKEN", "ready")]), &[]);
        assert!(shell.texts(&task).is_empty());
        assert_eq!(
            shell
                .for_test_with_config_env(&[], &["TOKEN"])
                .texts(&task)
                .len(),
            1
        );
    }

    #[test]
    fn resolved_view_trusts_the_rendered_defaults() {
        let task = default_task("{{ env.DEPLOY_KEY }}");
        let assigned = BTreeSet::from(["TOKEN".to_string()]);
        assert_eq!(EnvView::resolved(&assigned).texts(&task).len(), 1);
        // the default yielded to a caller value: nothing assigned, nothing scanned
        assert!(EnvView::resolved(&BTreeSet::new()).texts(&task).is_empty());
    }

    #[test]
    fn defaults_are_deferred_when_scripts_were_skipped() {
        let task = default_task("{{ env.DEPLOY_KEY }}");
        let mut view = EnvView::for_test(map(&[]), &[]);
        assert_eq!(view.texts(&task).len(), 1);
        view.defer_defaults = true;
        assert!(view.texts(&task).is_empty());
        assert!(!view.declared_keys(&task).contains("TOKEN"));
    }

    #[test]
    fn unset_in_the_task_env_lets_a_later_default_apply() {
        use crate::config::env_directive::{EnvDirective, EnvDirectiveOptions};
        let mut task = default_task("{{ env.DEPLOY_KEY }}");
        task.env.0.insert(
            0,
            EnvDirective::Rm("TOKEN".into(), EnvDirectiveOptions::default()),
        );
        let view = EnvView::for_test(map(&[("TOKEN", "ready")]), &[]);
        assert_eq!(view.texts(&task).len(), 1);
        assert!(view.declared_keys(&task).contains("TOKEN"));
    }

    #[test]
    fn get_env_arguments_are_scanned() {
        let r = tera_env_refs("{{ get_env(name='OTHER', default=env.DEPLOY_KEY) }}");
        assert_eq!(r, BTreeSet::from(["OTHER", "DEPLOY_KEY"].map(String::from)));
        let r = tera_env_refs("{{ get_env(name=\"X\", default=\"env.Y\") }}");
        assert_eq!(r, BTreeSet::from(["X".to_string()]));
    }

    #[test]
    fn earliest_tag_is_scanned() {
        let r = tera_env_refs("{% if env.KEY %}x{% endif %} {{ foo }}");
        assert!(r.contains("KEY"), "{r:?}");
    }

    #[test]
    fn grant_reports_bad_names() {
        let t = Task {
            name: "deploy".into(),
            secrets: Some(TaskSecrets(vec![
                "OK".into(),
                "deploy key".into(),
                "AWS_*".into(),
                "PATH".into(),
                "__MISE_X".into(),
            ])),
            ..Default::default()
        };
        let (grant, problems) = grant_for_task(&t);
        assert_eq!(grant.keys.len(), 1);
        let kinds: Vec<_> = problems.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            [
                ProblemKind::InvalidName,
                ProblemKind::Wildcard,
                ProblemKind::Reserved,
                ProblemKind::Reserved
            ]
        );
        assert!(
            problems[0]
                .text
                .contains("\"deploy key\" in secrets is not a valid environment variable name")
        );
    }

    #[test]
    fn shell_hook_refusal_carries_the_stale_mark_hint() {
        let task = Task {
            name: "deploy".into(),
            secrets: Some(TaskSecrets(vec!["A".into()])),
            ..Default::default()
        };
        let (grant, _) = grant_for_task(&task);
        let problems = static_problems(
            &task,
            &grant,
            Some(SecretsDenied::ShellHook),
            &EnvView::default(),
        );
        let text = problems
            .iter()
            .find(|p| p.kind == ProblemKind::Denied)
            .map(|p| p.render())
            .unwrap();
        assert!(text.contains("started by a mise shell hook"), "{text}");
        assert!(text.contains("left __MISE_SECRETS_DENIED set"), "{text}");
        let plain = static_problems(
            &task,
            &grant,
            Some(SecretsDenied::Hook),
            &EnvView::default(),
        );
        assert!(
            !plain
                .iter()
                .any(|p| p.render().contains("left __MISE_SECRETS_DENIED")),
        );
    }

    #[test]
    fn denied_marker_fails_closed() {
        assert_eq!(SecretsDenied::from_marker(""), None);
        assert_eq!(
            SecretsDenied::from_marker("hook"),
            Some(SecretsDenied::Hook)
        );
        assert_eq!(
            SecretsDenied::from_marker("watch_files"),
            Some(SecretsDenied::WatchFiles)
        );
        assert_eq!(SecretsDenied::from_marker("zzz"), Some(SecretsDenied::Hook));
        for d in [
            SecretsDenied::Hook,
            SecretsDenied::ShellHook,
            SecretsDenied::WatchFiles,
            SecretsDenied::PitchforkDaemon,
            SecretsDenied::Bootstrap,
        ] {
            assert_eq!(SecretsDenied::from_marker(d.marker()), Some(d));
        }
    }

    fn named(name: &str, args: &[&str]) -> Task {
        Task {
            name: name.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        }
    }

    fn composed(more: Vec<crate::config::env_directive::EnvDirective>) -> Task {
        use crate::config::env_directive::EnvDirective;
        let mut env = vec![EnvDirective::Val(
            "PGURL".into(),
            "postgres://{{ secrets.A }}@h".into(),
            Default::default(),
        )];
        env.extend(more);
        let mut task = Task {
            name: "migrate".into(),
            env: crate::config::config_file::mise_toml::EnvList(env),
            ..Default::default()
        };
        task.record_late_secret_env(std::path::Path::new("/p/mise.toml"))
            .unwrap();
        task
    }

    fn val(key: &str, value: &str) -> crate::config::env_directive::EnvDirective {
        crate::config::env_directive::EnvDirective::Val(
            key.into(),
            value.into(),
            Default::default(),
        )
    }

    fn t5(task: &Task) -> Vec<String> {
        let (grant, _) = grant_for_task(task);
        static_problems(task, &grant, None, &EnvView::default())
            .into_iter()
            .filter(|p| p.kind == ProblemKind::Template)
            .map(|p| p.render())
            .collect()
    }

    #[test]
    fn declared_env_keys_omit_a_composite_only_key() {
        let task = composed(vec![val("OTHER", "x")]);
        let keys = EnvView::default().declared_keys(&task);
        assert!(
            keys.contains("OTHER") && !keys.contains("PGURL"),
            "{keys:?}"
        );
    }

    #[test]
    fn every_env_reader_of_a_composed_key_is_t5() {
        use crate::config::env_directive::EnvDirective;
        let default =
            EnvDirective::Default("X".into(), "{{ env.PGURL }}".into(), Default::default());
        let file = EnvDirective::File("{{ env.PGURL }}.env".into(), Default::default());
        for (task, reader) in [
            (
                composed(vec![val("X", "{{ env.PGURL }}/x")]),
                "env.X uses {{ env.PGURL }}",
            ),
            (composed(vec![default]), "env.X uses {{ env.PGURL }}"),
            (composed(vec![file]), "env._.file uses {{ env.PGURL }}"),
        ] {
            let found = t5(&task);
            assert!(
                found
                    .iter()
                    .any(|m| m.contains(reader) && m.contains("PGURL is rendered from secrets")),
                "{reader}: {found:?}"
            );
        }
        // an overlay on a file task reads it too
        let mut overlaid = composed(vec![]);
        overlaid.merge_toml_overlay(Task {
            env: crate::config::config_file::mise_toml::EnvList(vec![val("X", "{{ env.PGURL }}")]),
            ..named("migrate", &[])
        });
        assert!(
            t5(&overlaid)
                .iter()
                .any(|m| m.contains("env.X uses {{ env.PGURL }}")),
            "overlay"
        );
        // unrelated reads are fine
        assert!(t5(&composed(vec![val("X", "{{ env.HOME }}")])).is_empty());
    }

    #[test]
    fn dollar_syntax_in_a_composed_value_follows_env_shell_expand() {
        let task = composed(vec![]);
        let mut task = task;
        task.env.0[0] = val("PGURL", "postgres://{{ secrets.A }}@h?x=$user");
        task.late_secret_env.clear();
        task.record_late_secret_env(std::path::Path::new("/p/mise.toml"))
            .unwrap();
        let (grant, _) = grant_for_task(&task);
        let texts = task.non_late_env_texts();
        let on = shell_expansion_problems(&task, &grant, &texts, true);
        assert_eq!(on.len(), 1, "{on:?}");
        assert!(on[0].render().contains("uses $VAR expansion together with"));
        assert!(shell_expansion_problems(&task, &grant, &texts, false).is_empty());
    }

    #[test]
    fn a_computed_get_env_name_may_read_any_composed_key() {
        let task = composed(vec![]);
        let (grant, _) = grant_for_task(&task);
        let check = |text: &str| {
            let texts = vec![("OTHER".to_string(), text.to_string())];
            composed_read_problems(&task, &grant, &texts, true)
                .into_iter()
                .map(|p| p.render())
                .collect::<Vec<_>>()
        };
        for dynamic in [
            "{{ get_env(name=vars.KEY) }}",
            "{{ get_env(name=vars.KEY, default='x') }}",
            "{{ get_env(name='A' ~ 'B') }}",
        ] {
            let found = check(dynamic);
            assert!(
                found
                    .iter()
                    .any(|m| m.contains("not a string literal") && m.contains("PGURL")),
                "{dynamic}: {found:?}"
            );
        }
        // whitespace of any kind around the name is still a literal name
        for literal in [
            "{{ get_env(name='HOME'\n) }}",
            "{{ get_env(\tname\t=\t'HOME'\t) }}",
            "{{ get_env(\n  name = 'HOME',\n  default = 'x'\n) }}",
        ] {
            assert!(check(literal).is_empty(), "{literal:?}");
        }
        assert_eq!(check("{{ get_env(name =\n'PGURL'\n) }}").len(), 1);
        // a literal name is an ordinary read, and unrelated literals are fine
        assert!(check("{{ get_env(name='HOME') }}").is_empty());
        assert_eq!(check("{{ get_env(name='PGURL') }}").len(), 1);
        // nothing composed, nothing to refuse
        let plain = named("m", &[]);
        let (none, _) = grant_for_task(&plain);
        let texts = vec![("O".to_string(), "{{ get_env(name=vars.K) }}".to_string())];
        assert!(composed_read_problems(&plain, &none, &texts, true).is_empty());
    }

    #[test]
    fn raw_blocks_and_comments_read_nothing() {
        let task = composed(vec![]);
        let (grant, _) = grant_for_task(&task);
        let check = |text: &str| {
            let texts = vec![("OTHER".to_string(), text.to_string())];
            composed_read_problems(&task, &grant, &texts, true).len()
        };
        assert_eq!(check("{% raw %}{{ env.PGURL }}{% endraw %}"), 0);
        assert_eq!(check("{%- raw -%}{{ env.PGURL }}{%- endraw -%}"), 0);
        assert_eq!(check("a{# {{ env.PGURL }} #}b"), 0);
        // a real read next to one still counts, before or after
        assert_eq!(check("{% raw %}x{% endraw %}{{ env.PGURL }}"), 1);
        assert_eq!(check("{{ env.PGURL }}{# c #}"), 1);
        assert_eq!(
            check("{# c #}{% raw %}{{ env.HOME }}{% endraw %}{{ env.PGURL }}"),
            1
        );
        // an unterminated block is a Tera error anyway; it is scanned, not trusted
        assert_eq!(check("{% raw %}{{ env.PGURL }}"), 1);
        assert_eq!(check("{# {{ env.PGURL }}"), 1);
        // the run-script scanner is the same one
        assert!(tera_env_refs("{% raw %}{{ env.PGURL }}{% endraw %}").is_empty());
    }

    #[test]
    fn composed_reads_in_given_texts_are_t5_and_value_free() {
        let task = composed(vec![]);
        let (grant, _) = grant_for_task(&task);
        let texts = vec![
            ("OTHER".to_string(), "x-s3cr3t-{{ env.PGURL }}".to_string()),
            ("THIRD".to_string(), "$PGURL".to_string()),
            ("FINE".to_string(), "{{ env.HOME }}-s3cr3t".to_string()),
        ];
        let found = composed_read_problems(&task, &grant, &texts, true);
        let text: Vec<String> = found.iter().map(|p| p.render()).collect();
        assert_eq!(found.len(), 2, "{text:?}");
        assert!(
            text.iter()
                .any(|m| m.contains("env.OTHER uses {{ env.PGURL }}"))
        );
        assert!(text.iter().any(|m| m.contains("env.THIRD uses $PGURL")));
        assert!(text.iter().all(|m| !m.contains("s3cr3t")), "{text:?}");
        // with expansion off only the Tera read counts
        assert_eq!(
            composed_read_problems(&task, &grant, &texts, false).len(),
            1
        );
        // nothing composed, nothing to check
        let plain = named("m", &[]);
        let (none, _) = grant_for_task(&plain);
        assert!(composed_read_problems(&plain, &none, &texts, true).is_empty());
    }

    #[test]
    fn a_venv_path_reading_a_composed_key_is_t5() {
        use crate::config::env_directive::EnvDirective;
        let venv = |path: &str| EnvDirective::PythonVenv {
            path: path.into(),
            create: false,
            python: None,
            uv_create_args: None,
            python_create_args: None,
            options: Default::default(),
        };
        let mut task = composed(vec![venv("{{ env.PGURL }}/venv")]);
        let found = t5(&task);
        assert!(
            found
                .iter()
                .any(|m| m.contains("env._.python.venv uses {{ env.PGURL }}")),
            "{found:?}"
        );
        task = composed(vec![venv("$PGURL/venv")]);
        let (grant, _) = grant_for_task(&task);
        let texts = task.non_late_env_texts();
        let on = shell_expansion_problems(&task, &grant, &texts, true);
        assert!(
            on.iter()
                .any(|p| p.render().contains("env._.python.venv uses $PGURL")),
            "{on:?}"
        );
        assert!(shell_expansion_problems(&task, &grant, &texts, false).is_empty());
        // a path that reads nothing composed is fine
        let task = composed(vec![venv("{{ env.HOME }}/venv")]);
        assert!(t5(&task).is_empty());
    }

    #[test]
    fn shell_reads_of_a_composed_key_are_t5_unless_escaped() {
        // env_shell_expand is on by default
        let found = t5(&composed(vec![val("X", "$PGURL?s")]));
        assert!(
            found
                .iter()
                .any(|m| m.contains("env.X uses $PGURL, but PGURL is rendered from secrets")),
            "{found:?}"
        );
        assert!(!t5(&composed(vec![val("X", "${PGURL}")])).is_empty());
        assert!(t5(&composed(vec![val("X", "$$PGURL")])).is_empty());
        assert!(t5(&composed(vec![val("X", "$PGURLX")])).is_empty());
    }

    #[test]
    fn all_does_not_export_what_templates_only_read() {
        let a = SecretName::new("A").unwrap();
        let file = PathBuf::from("/p/mise.toml");
        let read = SecretGrant {
            keys: BTreeMap::from([(
                a.clone(),
                vec![GrantOrigin::Template {
                    var: "PGURL".into(),
                    file: file.clone(),
                }],
            )]),
            all: Some(GrantOrigin::CliAll),
            late: vec![],
        };
        assert!(read.exported_keys().next().is_none());
        let listed = SecretGrant {
            keys: BTreeMap::from([(a.clone(), vec![GrantOrigin::TaskList { file }])]),
            all: Some(GrantOrigin::CliAll),
            late: vec![],
        };
        assert_eq!(listed.exported_keys().collect::<Vec<_>>(), [&a]);
    }

    #[test]
    fn cli_grant_reaches_only_the_tasks_named_on_the_command_line() {
        let build = named("build", &[]);
        let deploy = named("deploy", &[]);
        // `mise run --secrets A deploy`, where deploy depends on build
        let cli = CliSecretGrant::new(&["A".into()], false, std::slice::from_ref(&deploy)).unwrap();
        let grant = cli.for_task(&deploy, false).unwrap();
        assert_eq!(
            grant.keys.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
            ["A"]
        );
        assert_eq!(
            grant.keys[&SecretName::new("A").unwrap()],
            [GrantOrigin::CliFlag]
        );
        assert!(cli.for_task(&build, false).is_none());
        // `mise run --secrets A build ::: deploy`
        let cli =
            CliSecretGrant::new(&["A".into()], false, &[build.clone(), deploy.clone()]).unwrap();
        assert!(cli.for_task(&build, false).is_some());
        assert!(cli.for_task(&deploy, false).is_some());
        // a post-phase occurrence, another invocation, and a task a run entry started
        assert!(
            cli.for_task(&deploy.clone().with_run_phase(TaskRunPhase::Post), false)
                .is_none()
        );
        assert!(cli.for_task(&named("deploy", &["--prod"]), false).is_none());
        assert!(cli.for_task(&deploy, true).is_none());
        // a task that only starts other tasks has nothing to receive the grant
        let orchestrator = Task {
            run: vec![crate::task::RunEntry::SingleTask {
                task: "build".into(),
                args: vec![],
                env: Default::default(),
            }],
            ..named("parent", &[])
        };
        let cli =
            CliSecretGrant::new(&["A".into()], false, std::slice::from_ref(&orchestrator)).unwrap();
        assert!(cli.for_task(&orchestrator, false).is_none());
        // every task a glob, `default` or `--all` selected is named
        let cli =
            CliSecretGrant::new(&[], true, &[named("lint", &[]), named("test", &[])]).unwrap();
        let all = cli.for_task(&named("test", &[]), false).unwrap();
        assert!(all.keys.is_empty());
        assert_eq!(all.all, Some(GrantOrigin::CliAll));
    }

    #[test]
    fn cli_grant_rejects_names_that_cannot_be_granted() {
        let tasks = [named("deploy", &[])];
        let err = CliSecretGrant::new(
            &["ok".into(), "a b".into(), "AWS_*".into(), "PATH".into()],
            false,
            &tasks,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            err.contains("--secrets: \"a b\" in --secrets is not a valid"),
            "{err}"
        );
        assert!(
            err.contains("\"AWS_*\" in --secrets: wildcards are not supported"),
            "{err}"
        );
        assert!(
            err.contains("secret name PATH is reserved by mise"),
            "{err}"
        );
    }

    #[test]
    fn command_line_grants_are_not_held_to_where_the_task_is_defined() {
        // a global task: its own list is G9, the command line is the person asking
        let task = Task {
            global: true,
            ..named("deploy", &[])
        };
        let a = SecretName::new("A").unwrap();
        let own = SecretGrant {
            keys: BTreeMap::from([(
                a.clone(),
                vec![GrantOrigin::TaskList {
                    file: PathBuf::from("/h/.config/mise/config.toml"),
                }],
            )]),
            all: None,
            late: vec![],
        };
        assert!(
            static_problems(&task, &own, None, &EnvView::default())
                .iter()
                .any(|p| p.kind == ProblemKind::NotProject)
        );
        let cli = SecretGrant {
            keys: BTreeMap::from([(a.clone(), vec![GrantOrigin::CliFlag])]),
            all: None,
            late: vec![],
        };
        assert!(static_problems(&task, &cli, None, &EnvView::default()).is_empty());
        let all = SecretGrant {
            keys: BTreeMap::new(),
            all: Some(GrantOrigin::CliAll),
            late: vec![],
        };
        assert!(!all.is_empty());
        assert!(static_problems(&task, &all, None, &EnvView::default()).is_empty());
        // both: the task's own list still counts
        let both = own.merged(cli);
        assert!(
            static_problems(&task, &both, None, &EnvView::default())
                .iter()
                .any(|p| p.kind == ProblemKind::NotProject)
        );
        assert_eq!(
            both.granted_by(&a),
            "--secrets, secrets = [...] in config.toml"
        );
        // a launcher that was not a person is refused either way
        assert!(
            static_problems(&task, &all, Some(SecretsDenied::Hook), &EnvView::default())
                .iter()
                .any(|p| p.kind == ProblemKind::Denied)
        );
    }

    #[test]
    fn template_refs_cover_run_and_env_literals() {
        use crate::config::env_directive::EnvDirective;
        let mut task = Task {
            name: "pay".into(),
            run: vec![crate::task::RunEntry::Script(
                "echo {{ env.STRIPE_KEY }} {{ get_env(name='B') }}".into(),
            )],
            ..Default::default()
        };
        task.env.0.push(EnvDirective::Val(
            "X".into(),
            "{{ env[\"C\"] }}".into(),
            Default::default(),
        ));
        assert_eq!(
            EnvView::default().template_refs(&task),
            BTreeSet::from(["STRIPE_KEY", "B", "C"].map(String::from))
        );
    }
}
