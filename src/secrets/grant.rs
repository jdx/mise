//! What a task asks for: its `secrets = [...]` list, and the checks that need no source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::de::{self, Deserializer, SeqAccess, Visitor};

use super::SecretName;
use crate::task::Task;

pub(crate) const G4_TRUE: &str = "secrets = true is not supported; tasks receive only the secrets they list, e.g. secrets = [\"DEPLOY_KEY\"]";
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
            _ => Some(Self::Hook),
        }
    }
}

pub(crate) const DENIED_MARKER: &str = "__MISE_SECRETS_DENIED";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GrantOrigin {
    TaskList { file: PathBuf },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SecretGrant {
    pub(crate) keys: BTreeMap<SecretName, Vec<GrantOrigin>>,
}

impl SecretGrant {
    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub(crate) fn names(&self) -> Vec<SecretName> {
        self.keys.keys().cloned().collect()
    }

    pub(crate) fn granted_by(&self) -> String {
        let mut files: Vec<String> = self
            .keys
            .values()
            .flatten()
            .map(|GrantOrigin::TaskList { file }| {
                file.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .collect();
        files.sort();
        files.dedup();
        files.join(", ")
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

/// G3, G6 and the grant of the valid names. Invalid names are reported, not granted.
pub(crate) fn grant_for_task(task: &Task) -> (SecretGrant, Vec<Problem>) {
    let mut grant = SecretGrant::default();
    let mut problems = vec![];
    let Some(list) = &task.secrets else {
        return (grant, problems);
    };
    let file = task.config_source.clone();
    for raw in list.names() {
        if raw.contains('*') {
            problems.push(Problem::new(
                &task.name,
                Some(raw),
                ProblemKind::Wildcard,
                format!(
                    "task {}: \"{raw}\" in secrets: wildcards are not supported; list each key (see mise secrets ls)",
                    task.name
                ),
            ));
            continue;
        }
        let Some(name) = SecretName::new(raw) else {
            problems.push(Problem::new(
                &task.name,
                Some(raw),
                ProblemKind::InvalidName,
                format!(
                    "task {}: \"{raw}\" in secrets is not a valid environment variable name ([A-Za-z_][A-Za-z0-9_]*)",
                    task.name
                ),
            ));
            continue;
        };
        if mise_util::env::is_reserved_secret_name(name.as_str()) {
            problems.push(Problem::new(
                &task.name,
                Some(raw),
                ProblemKind::Reserved,
                format!(
                    "task {}: secret name {raw} is reserved by mise and cannot be granted",
                    task.name
                ),
            ));
            continue;
        }
        let origins = grant.keys.entry(name).or_default();
        let origin = GrantOrigin::TaskList { file: file.clone() };
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    (grant, problems)
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
    if let Some(source) = task.secrets_remote_source() {
        problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::Remote,
            format!(
                "task {} comes from a remote source ({source}) and cannot list secrets",
                task.name
            ),
        ));
    }
    let project_root = task
        .config_root
        .as_deref()
        .is_some_and(|r| super::config::is_project_secrets_root(r, &crate::dirs::HOME));
    if task.secrets_remote_source().is_none() && (task.global || !project_root) {
        problems.push(Problem::new(
            &task.name,
            None,
            ProblemKind::NotProject,
            format!(
                "task {} is defined in {}, which is not project config (global or system config, or a file in or above your home directory), so it cannot list secrets",
                task.name,
                crate::file::display_path(&task.config_source)
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
                    "task {} lists secrets, but it was started by {}",
                    task.name,
                    denied.launcher()
                ),
            )
            .detail(format!(
                "mise secrets are not available there in this version. Run it directly: mise run {}",
                task.name
            )),
        );
    }
    let granted: BTreeSet<&str> = grant.keys.keys().map(|k| k.as_str()).collect();
    let mut refs = BTreeSet::new();
    for script in task.run_script_strings() {
        refs.extend(tera_env_refs(&script));
    }
    for (_, value) in task_env_literals(task) {
        refs.extend(tera_env_refs(&value));
    }
    for key in refs {
        if granted.contains(key.as_str()) {
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
    problems
}

fn task_env_literals(task: &Task) -> Vec<(String, String)> {
    use crate::config::env_directive::EnvDirective;
    task.env
        .0
        .iter()
        .chain(task.inherited_env.0.iter())
        .filter_map(|d| match d {
            EnvDirective::Val(k, v, _) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .collect()
}

/// `{{ env.K }}`, `{{ env["K"] }}` and `get_env(name="K")` inside Tera tags. Lexical on
/// purpose: a false positive only fails a grant that would be unusable anyway.
pub(crate) fn tera_env_refs(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{").or_else(|| rest.find("{%")) {
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

fn scan_tag(tag: &str, out: &mut BTreeSet<String>) {
    // env.NAME and env["NAME"] / env['NAME']
    let bytes: Vec<char> = tag.chars().collect();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let at_boundary = i == 0 || !is_ident(bytes[i - 1]);
        if at_boundary && bytes[i..].starts_with(&['e', 'n', 'v']) {
            let mut j = i + 3;
            if j < bytes.len() && bytes[j] == '.' {
                j += 1;
                let s = j;
                while j < bytes.len() && is_ident(bytes[j]) {
                    j += 1;
                }
                if j > s {
                    out.insert(bytes[s..j].iter().collect());
                }
            } else if j < bytes.len() && bytes[j] == '[' {
                j += 1;
                if j < bytes.len() && (bytes[j] == '"' || bytes[j] == '\'') {
                    let q = bytes[j];
                    j += 1;
                    let s = j;
                    while j < bytes.len() && bytes[j] != q {
                        j += 1;
                    }
                    out.insert(bytes[s..j].iter().collect());
                }
            }
        }
        i += 1;
    }
    // get_env(name="NAME")
    let mut search = tag;
    while let Some(p) = search.find("get_env(") {
        let after = &search[p + "get_env(".len()..];
        let end = after.find(')').unwrap_or(after.len());
        let args = &after[..end];
        if let Some(n) = args.find("name") {
            let v = args[n + 4..].trim_start();
            if let Some(v) = v.strip_prefix('=') {
                let v = v.trim_start();
                if let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let inner = &v[1..];
                    if let Some(e) = inner.find(q) {
                        out.insert(inner[..e].to_string());
                    }
                }
            }
        }
        search = after;
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
    fn grant_reports_bad_names() {
        let mut t = Task::default();
        t.name = "deploy".into();
        t.secrets = Some(TaskSecrets(vec![
            "OK".into(),
            "deploy key".into(),
            "AWS_*".into(),
            "PATH".into(),
            "__MISE_X".into(),
        ]));
        let (grant, problems) = grant_for_task(&t);
        assert_eq!(grant.names().len(), 1);
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
    }
}
