//! What a task asks for: its `secrets = [...]` list, and the checks that need no source.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

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
    WatchFiles,
    PitchforkDaemon,
    Bootstrap,
}

impl SecretsDenied {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::WatchFiles => "watch_files",
            Self::PitchforkDaemon => "pitchfork_daemon",
            Self::Bootstrap => "bootstrap",
        }
    }

    pub(crate) fn launcher(self) -> &'static str {
        match self {
            Self::Hook => "a mise hook",
            Self::WatchFiles => "watch_files",
            Self::PitchforkDaemon => "a pitchfork daemon",
            Self::Bootstrap => "mise bootstrap",
        }
    }

    /// Parses `__MISE_SECRETS_DENIED`. Any other non-empty value fails closed.
    pub(crate) fn from_marker(value: &str) -> Option<Self> {
        match value {
            "" => None,
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
    }
    // a key that the task exports under its own name is not in the environment when a
    // template is rendered
    let granted: BTreeSet<&str> = grant.exported_keys().map(|k| k.as_str()).collect();
    let mut run_refs = BTreeSet::new();
    for script in task.run_script_strings() {
        run_refs.extend(tera_env_refs(&script));
    }
    let env_texts = task.non_late_env_texts();
    let env_refs: Vec<(String, BTreeSet<String>)> = env_texts
        .iter()
        .map(|(name, value)| (name.clone(), tera_env_refs(value)))
        .collect();
    for key in task_template_refs(task) {
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
    // T5: an env value reads a key that is built from secrets when the task starts
    for (name, keys) in &env_refs {
        for key in keys.iter().filter(|k| grant.is_late_key(k)) {
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

/// Keys that task `[env]` and the config's `[env]` set by name, for the preflight. The spawn
/// repeats the exact check against the real environment.
pub(crate) fn declared_env_keys(task: &Task, config: &crate::config::Config) -> BTreeSet<String> {
    use crate::config::env_directive::EnvDirective;
    // the task's part leaves out the values rendered at spawn: they are the composites, not
    // something that could collide with them
    let directives = task
        .render_env_directives()
        .into_iter()
        .map(|(d, _)| d)
        .chain(
            config
                .config_files
                .values()
                .filter_map(|cf| cf.env_entries().ok())
                .flatten(),
        );
    directives
        .filter_map(|d| match d {
            EnvDirective::Val(k, ..) | EnvDirective::Default(k, ..) => Some(k),
            _ => None,
        })
        .collect()
}

/// Env names the task's `run` scripts and `env` values read through a template. Those render
/// before secrets exist, so a granted key among them would render without its value. Env
/// values that use `{{ secrets.X }}` are not scanned: they are rendered at spawn.
pub(crate) fn task_template_refs(task: &Task) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for script in task.run_script_strings() {
        refs.extend(tera_env_refs(&script));
    }
    for (_, value) in task.plain_env_vals() {
        refs.extend(tera_env_refs(&value));
    }
    refs
}

/// `{{ env.K }}`, `{{ env["K"] }}` and `get_env(name="K")` inside Tera tags. Lexical, but
/// aware of string literals: quoted text such as `"env.K"` is data and is not an env read.
pub(crate) fn tera_env_refs(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(start) = [rest.find("{{"), rest.find("{%")]
        .into_iter()
        .flatten()
        .min()
    {
        let tag = &rest[start..];
        let close = if tag.starts_with("{{") { "}}" } else { "%}" };
        let end = tag.find(close).map(|e| e + 2).unwrap_or(tag.len());
        scan_tag(&tag[..end], &mut out);
        rest = &tag[end..];
    }
    out
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
            while chars.get(j) == Some(&' ') {
                j += 1;
            }
            if chars.get(j) == Some(&'=') {
                j += 1;
                while chars.get(j) == Some(&' ') {
                    j += 1;
                }
                if chars.get(j).is_some_and(|c| is_quote(*c)) {
                    out.insert(read_literal(chars, j).0);
                    return;
                }
            }
        }
        i += 1;
    }
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
        static_problems(task, &grant, None)
            .into_iter()
            .filter(|p| p.kind == ProblemKind::Template)
            .map(|p| p.render())
            .collect()
    }

    #[tokio::test]
    async fn declared_env_keys_omit_a_composite_only_key() {
        let task = composed(vec![val("OTHER", "x")]);
        let config = crate::config::Config::get().await.unwrap();
        let keys = declared_env_keys(&task, &config);
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
            static_problems(&task, &own, None)
                .iter()
                .any(|p| p.kind == ProblemKind::NotProject)
        );
        let cli = SecretGrant {
            keys: BTreeMap::from([(a.clone(), vec![GrantOrigin::CliFlag])]),
            all: None,
            late: vec![],
        };
        assert!(static_problems(&task, &cli, None).is_empty());
        let all = SecretGrant {
            keys: BTreeMap::new(),
            all: Some(GrantOrigin::CliAll),
            late: vec![],
        };
        assert!(!all.is_empty());
        assert!(static_problems(&task, &all, None).is_empty());
        // both: the task's own list still counts
        let both = own.merged(cli);
        assert!(
            static_problems(&task, &both, None)
                .iter()
                .any(|p| p.kind == ProblemKind::NotProject)
        );
        assert_eq!(
            both.granted_by(&a),
            "--secrets, secrets = [...] in config.toml"
        );
        // a launcher that was not a person is refused either way
        assert!(
            static_problems(&task, &all, Some(SecretsDenied::Hook))
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
            task_template_refs(&task),
            BTreeSet::from(["STRIPE_KEY", "B", "C"].map(String::from))
        );
    }
}
