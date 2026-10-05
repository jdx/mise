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

/// G1 and G2 for a grant, against what the source describes.
pub(crate) fn key_problems(
    task: &Task,
    grant: &SecretGrant,
    catalog: &super::Catalog,
    source_label: &str,
) -> Vec<Problem> {
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
                        &task.name,
                        Some(key.as_str()),
                        ProblemKind::Unknown,
                        format!("task {}: unknown secret {key}", task.name),
                    )
                    .detail(hint)
                    .detail(format!(
                        "Granted by: secrets = [...] in {}",
                        grant.granted_by()
                    ))
                    .detail("See the available keys with `mise secrets ls`.")
                    .suggestion(suggestion),
                );
            }
            Some(entry) if !entry.injectable => {
                problems.push(
                    Problem::new(
                        &task.name,
                        Some(key.as_str()),
                        ProblemKind::NotInjectable,
                        format!("task {}: {key} cannot be injected", task.name),
                    )
                    .detail(format!(
                        "fnox config sets env = false for {key}, so fnox never hands it to processes. Read it with `fnox get {key}` in your script, or set env = \"exec\" for it in fnox.toml if processes should receive it."
                    )),
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

/// G13: the task's sandbox would drop the granted key.
pub(crate) fn sandbox_problem(label: &str, key: &str) -> Problem {
    Problem::new(
        label,
        Some(key),
        ProblemKind::Sandbox,
        format!(
            "task {label} is granted {key}, but its sandbox denies env vars; add allow_env = [\"{key}\"] to the task or pass --allow-env {key}"
        ),
    )
}

/// G11: mise itself sets the granted key for this task.
pub(crate) fn collision_problem(label: &str, key: &str) -> Problem {
    Problem::new(
        label,
        Some(key),
        ProblemKind::Collision,
        format!("task {label}: {key} is both a secret and a mise env var"),
    )
    .detail(format!(
        "mise sets {key} for this task ([env], the task's env, a tool, or a setting). Tools the task starts through mise shims recompute it and would replace the secret."
    ))
    .detail(format!(
        "Keep one: move the default into fnox.toml ({key} = {{ ..., default = \"...\" }}) or rename the mise variable."
    ))
}

/// Keys that task `[env]` and the config's `[env]` set by name, for the preflight. The spawn
/// repeats the exact check against the real environment.
pub(crate) fn declared_env_keys(task: &Task, config: &crate::config::Config) -> BTreeSet<String> {
    use crate::config::env_directive::EnvDirective;
    let directives = task
        .env
        .0
        .iter()
        .chain(task.inherited_env.0.iter())
        .chain(task.overlay_env.iter().map(|(d, _)| d))
        .cloned()
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
            i = scan_get_env_args(&chars, i + "get_env(".len(), out);
            continue;
        }
        i += 1;
    }
}

/// `name="K"` among the arguments of a `get_env(` call; strings elsewhere are skipped.
fn scan_get_env_args(chars: &[char], mut i: usize, out: &mut BTreeSet<String>) -> usize {
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
                    let (name, next) = read_literal(chars, j);
                    out.insert(name);
                    i = next;
                    continue;
                }
            }
        }
        i += 1;
    }
    i
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
}
