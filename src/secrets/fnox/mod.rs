//! The fnox source. fnox is named only here and in `mise doctor`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use eyre::{Result, bail};
use indexmap::IndexMap;
use tokio::io::AsyncReadExt;

use super::config::SelectedSource;
use super::source::{
    Catalog, CatalogEntry, InjectMode, KeyKind, KeySelection, ResolveError, Resolved, SecretSource,
    SourceCx, SourceId,
};
use super::{SecretName, SecretValue};
use crate::config::Config;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::{self, display_path};
use crate::toolset::Toolset;

mod wire;

/// Message text only: never compare versions (versions are not necessarily semver).
pub(crate) const FNOX_ENV_MIN_VERSION: &str = "1.39.0";

const NOT_FOUND_PREFIX: &str = "mise secrets: fnox not found";

/// Whether an error message is the "fnox CLI not found" one.
pub(crate) fn is_not_found_message(message: &str) -> bool {
    message.contains(NOT_FOUND_PREFIX)
}

const MAX_STDOUT: u64 = 16 << 20;

const DESCRIBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug)]
pub(crate) struct FnoxSource {
    id: SourceId,
    bin: PathBuf,
    /// Built once and reused for every fnox call.
    env: EnvMap,
}

/// The project's own `[tools] fnox`, then PATH without mise shims.
pub(crate) async fn find_binary(config: &Arc<Config>) -> Option<PathBuf> {
    match config.get_toolset().await {
        Ok(ts) => find_binary_in(config, ts).await,
        Err(_) => find_binary_on_path(),
    }
}

/// Like `find_binary`, for a toolset other than the current project's (a monorepo task's).
pub(crate) async fn find_binary_in(config: &Arc<Config>, ts: &Toolset) -> Option<PathBuf> {
    if let Some(bin) = ts.which_bin_spawnable(config, "fnox").await {
        return Some(bin);
    }
    find_binary_on_path()
}

fn find_binary_on_path() -> Option<PathBuf> {
    // Absolute against mise's cwd: fnox runs from the source root, so a relative PATH hit
    // would resolve somewhere else. Not canonicalized, so a multi-call binary keeps its name.
    crate::backend::which_no_shims_spawnable("fnox").map(|p| std::path::absolute(&p).unwrap_or(p))
}

impl FnoxSource {
    /// `ts` is the toolset of whoever the source is for: the current project's, or a
    /// monorepo task's own, so the subproject's fnox is the one that runs.
    pub(crate) async fn new(
        config: &Arc<Config>,
        selected: &SelectedSource,
        ts: &Toolset,
        config_env: Option<(EnvMap, std::collections::BTreeSet<String>)>,
    ) -> Result<Self> {
        let declared_in = &selected.declared_in[0];
        let Some(bin) = find_binary_in(config, ts).await else {
            bail!(
                "{NOT_FOUND_PREFIX}\n  [secrets.fnox] in {} needs the fnox CLI. Add it to the project: mise use fnox\n  mise looks in the project's tools first, then on PATH.",
                display_path(declared_in)
            );
        };
        let root = dunce::canonicalize(&selected.root).map_err(|e| {
            eyre::eyre!(
                "mise secrets: cannot access {}: {e}",
                display_path(&selected.root)
            )
        })?;
        let (mut tool_env, mut removals) = ts.env_with_path_and_removals(config).await?;
        apply_config_env(&mut tool_env, &mut removals, config_env);
        Ok(Self {
            id: SourceId {
                kind: "fnox",
                root,
                profile: selected.profile.clone(),
            },
            bin,
            env: source_env(env::PRISTINE_ENV.clone(), tool_env, &removals),
        })
    }

    pub(crate) fn tool_path(&self) -> &Path {
        &self.bin
    }

    /// Global flags go before `env`. A call that may prompt omits `--non-interactive` and
    /// `--no-daemon`, so fnox follows its own `[daemon]` setting.
    fn argv(&self, rest: &[&str], interactive: bool) -> Vec<String> {
        let mut argv = vec![];
        if let Some(profile) = &self.id.profile {
            argv.push("-P".to_string());
            argv.push(profile.clone());
        }
        if !interactive {
            argv.push("--non-interactive".to_string());
            argv.push("--no-daemon".to_string());
        }
        argv.extend(rest.iter().map(|s| s.to_string()));
        argv
    }

    fn describe_argv(&self) -> Vec<String> {
        self.argv(&["env", "--json", "--describe"], false)
    }

    fn resolve_argv(&self, keys: &KeySelection, interactive: bool) -> Vec<String> {
        let list = keys
            .keys()
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(",");
        self.argv(
            &["env", "--json", "--for", "exec", "--keys", &list],
            interactive,
        )
    }

    fn command(&self, args: &[String]) -> tokio::process::Command {
        self.command_with_stdin(args, Stdio::null())
    }

    fn command_with_stdin(&self, args: &[String], stdin: Stdio) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.bin);
        cmd.args(args)
            .env_clear()
            .envs(&self.env)
            .current_dir(&self.id.root)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        cmd
    }

    /// Only used to fill in the message when an old fnox has no `env` command.
    async fn version(&self) -> String {
        let mut cmd = self.command(&["--version".to_string()]);
        cmd.stderr(Stdio::null());
        let out = tokio::time::timeout(std::time::Duration::from_secs(5), cmd.output()).await;
        let Ok(Ok(out)) = out else {
            return "(unknown version)".to_string();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().next().unwrap_or("").trim();
        let line = line.strip_prefix("fnox ").unwrap_or(line);
        let line: String = line
            .chars()
            .filter(|c| !c.is_ascii_control())
            .take(64)
            .collect();
        if line.is_empty() {
            "(unknown version)".to_string()
        } else {
            line
        }
    }
}

#[async_trait::async_trait]
impl SecretSource for FnoxSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn label(&self) -> String {
        let profile = self
            .id
            .profile
            .as_ref()
            .map(|p| format!(" (profile {p})"))
            .unwrap_or_default();
        format!("fnox{profile} in {}", display_path(&self.id.root))
    }

    /// The binary and the whole source env, hashed: the env can carry credentials.
    fn build_fingerprint(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.bin.to_string_lossy().as_bytes());
        for (key, value) in &self.env {
            hasher.update(b"\0");
            hasher.update(key.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    async fn resolve(
        &self,
        cx: &SourceCx,
        keys: &KeySelection,
        catalog: &Catalog,
    ) -> std::result::Result<Resolved, ResolveError> {
        self.run_resolve(cx, keys, catalog).await
    }

    async fn describe(&self) -> Result<Catalog> {
        let args = self.describe_argv();
        let mut child = self.command(&args).spawn().map_err(|e| {
            eyre::eyre!(
                "mise secrets: could not run fnox ({}): {e}",
                display_path(&self.bin)
            )
        })?;
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut buf = vec![];
        let run = async {
            (&mut stdout)
                .take(MAX_STDOUT + 1)
                .read_to_end(&mut buf)
                .await?;
            if buf.len() as u64 > MAX_STDOUT {
                let _ = child.kill().await;
                return Ok(None);
            }
            Ok::<_, eyre::Report>(Some(child.wait().await?))
        };
        let status = match tokio::time::timeout(DESCRIBE_TIMEOUT, run).await {
            Ok(res) => res?,
            Err(_) => {
                let _ = child.kill().await;
                bail!(
                    "mise secrets: fnox env --json --describe did not finish within {}s in {}; its error output, if any, is above",
                    DESCRIBE_TIMEOUT.as_secs(),
                    display_path(&self.id.root)
                );
            }
        };
        let Some(status) = status else {
            bail!(
                "mise secrets: fnox env --json printed output mise could not parse (over {MAX_STDOUT} bytes; too large). The output is not shown because it can contain secret values."
            );
        };
        match interpret(status.success(), &status.to_string(), &buf, &self.id.root) {
            Ok(catalog) => Ok(catalog),
            Err(Failure::Message(msg)) => bail!("{msg}"),
            Err(Failure::NoJson(status)) => bail!(
                "mise secrets: fnox {} ({}) exited with {status} and no JSON result from `fnox env --json`; its error output is above\n  If this fnox predates `fnox env`, mise secrets needs fnox {FNOX_ENV_MIN_VERSION} or newer: mise use fnox@latest",
                self.version().await,
                display_path(&self.bin),
            ),
        }
    }
}

impl FnoxSource {
    async fn run_resolve(
        &self,
        cx: &SourceCx,
        keys: &KeySelection,
        catalog: &Catalog,
    ) -> std::result::Result<Resolved, ResolveError> {
        let other = |m: String| ResolveError::Other(m);
        let args = self.resolve_argv(keys, cx.interactive);
        // Interactive: fnox gets the terminal's stdin to prompt on, and no timeout (a person
        // is answering). Otherwise stdin is null. Never `DESCRIBE_TIMEOUT`.
        let stdin = if cx.interactive {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        let mut child = self.command_with_stdin(&args, stdin).spawn().map_err(|e| {
            other(format!(
                "mise secrets: could not run fnox ({}): {e}",
                display_path(&self.bin)
            ))
        })?;
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut buf = vec![];
        let read = (&mut stdout)
            .take(MAX_STDOUT + 1)
            .read_to_end(&mut buf)
            .await;
        if let Err(e) = read {
            let _ = child.kill().await;
            return Err(other(format!(
                "mise secrets: reading fnox output failed: {e}"
            )));
        }
        if buf.len() as u64 > MAX_STDOUT {
            let _ = child.kill().await;
            return Err(other(format!(
                "mise secrets: fnox env --json printed output mise could not parse (over {MAX_STDOUT} bytes; too large). The output is not shown because it can contain secret values."
            )));
        }
        let status = child
            .wait()
            .await
            .map_err(|e| other(format!("mise secrets: waiting for fnox failed: {e}")))?;
        match interpret_resolve(
            status.success(),
            &status.to_string(),
            &buf,
            keys.keys(),
            catalog,
            &self.id.root,
        ) {
            Ok(resolved) => Ok(resolved),
            Err(ResolveFailure::Error(e)) => Err(e),
            Err(ResolveFailure::NoJson(status)) => Err(other(format!(
                "mise secrets: fnox {} ({}) exited with {status} and no JSON result from `fnox env --json`; its error output is above\n  If this fnox predates `fnox env`, mise secrets needs fnox {FNOX_ENV_MIN_VERSION} or newer: mise use fnox@latest",
                self.version().await,
                display_path(&self.bin),
            ))),
        }
    }
}

enum ResolveFailure {
    Error(ResolveError),
    NoJson(String),
}

fn interpret_resolve(
    success: bool,
    status: &str,
    buf: &[u8],
    requested: &std::collections::BTreeSet<SecretName>,
    catalog: &Catalog,
    root: &Path,
) -> std::result::Result<Resolved, ResolveFailure> {
    use ResolveFailure::{Error, NoJson};
    let other = |f: Failure| match f {
        Failure::Message(m) => Error(ResolveError::Other(m)),
        Failure::NoJson(s) => NoJson(s),
    };
    if !success {
        let doc: wire::ErrorDocument = match serde_json::from_slice(buf) {
            Ok(doc) => doc,
            Err(_) => return Err(NoJson(status.to_string())),
        };
        let e = doc.error;
        let message = mise_util::redactions::redact_global(&e.message);
        return Err(Error(match e.kind.as_str() {
            "invalid_keys" => ResolveError::Invalid {
                unknown: e.unknown,
                suggestions: e.suggestions,
                not_injectable: e.not_injectable.into_iter().map(|n| n.key).collect(),
            },
            "resolution" => ResolveError::Resolution(message),
            "config" => ResolveError::Other(format!(
                "mise secrets: fnox could not load its config in {}: {message}",
                display_path(root)
            )),
            kind => ResolveError::Other(format!(
                "mise secrets: fnox env failed ({kind}) in {}: {message}",
                display_path(root)
            )),
        }));
    }
    let head: wire::Head = serde_json::from_slice(buf).map_err(|e| other(unparsable(buf, &e)))?;
    if head.schema != 1 {
        return Err(Error(ResolveError::Other(format!(
            "mise secrets: fnox {} sent env schema {}; this mise understands schema 1. Upgrade mise (mise self-update) or pin an older fnox.",
            head.fnox_version.as_deref().unwrap_or("(unknown version)"),
            head.schema
        ))));
    }
    let doc: wire::EnvDocument =
        serde_json::from_slice(buf).map_err(|e| other(unparsable(buf, &e)))?;
    let mut out = Resolved::default();
    let accept = |into_files: bool, key: String, value: String, out: &mut Resolved| {
        let Some(name) = SecretName::new(&key).filter(|n| requested.contains(n)) else {
            out.unrequested.insert(key);
            return;
        };
        if catalog.entries.get(&name).is_some_and(|e| !e.injectable) {
            out.not_injectable.insert(name);
            return;
        }
        let value = SecretValue::new(value);
        if into_files {
            out.files.insert(name, value);
        } else {
            out.set.insert(name, value);
        }
    };
    for (key, value) in doc.set {
        accept(false, key, value, &mut out);
    }
    for (key, value) in doc.files {
        accept(true, key, value, &mut out);
    }
    out.remove = doc.remove.into_iter().collect();
    out.missing = doc
        .missing
        .iter()
        .filter_map(|k| SecretName::new(k))
        .filter(|n| requested.contains(n))
        .collect();
    Ok(out)
}

/// A monorepo task's source runs with its own subproject's `[env]` on top of the toolset's.
fn apply_config_env(
    tool_env: &mut EnvMap,
    removals: &mut std::collections::BTreeSet<String>,
    config_env: Option<(EnvMap, std::collections::BTreeSet<String>)>,
) {
    let Some((values, unset)) = config_env else {
        return;
    };
    for key in &unset {
        tool_env.remove(key);
    }
    for key in values.keys() {
        removals.remove(key);
    }
    tool_env.extend(values);
    removals.extend(unset);
}

/// An activated shell's env for this directory: pristine env plus the toolset's, without mise
/// bookkeeping, shims or dispatch dirs. Never task env or resolved values.
fn source_env(
    mut base: EnvMap,
    overlay: EnvMap,
    removals: &std::collections::BTreeSet<String>,
) -> EnvMap {
    base.extend(overlay);
    for key in removals {
        base.remove(key);
    }
    base.retain(|k, _| !k.starts_with("__MISE_"));
    if let Some(path) = base.get_mut(&*env::PATH_KEY) {
        *path = file::strip_dispatch_dirs_from_path(&file::strip_shims_from_path(path));
    }
    base
}

enum Failure {
    /// Failed without a JSON document: a crash, or an fnox without `env`. Carries the exit status.
    NoJson(String),
    Message(String),
}

/// Never put stdout, or a `serde_json::Error`'s `Display`, into a message: the latter embeds
/// the input.
fn unparsable(buf: &[u8], e: &serde_json::Error) -> Failure {
    Failure::Message(format!(
        "mise secrets: fnox env --json printed output mise could not parse ({} bytes; {:?} at line {} column {}). The output is not shown because it can contain secret values.",
        buf.len(),
        e.classify(),
        e.line(),
        e.column()
    ))
}

fn interpret(
    success: bool,
    status: &str,
    buf: &[u8],
    root: &Path,
) -> std::result::Result<Catalog, Failure> {
    if !success {
        return match serde_json::from_slice::<wire::ErrorDocument>(buf) {
            Ok(doc) => {
                let message = mise_util::redactions::redact_global(&doc.error.message);
                let root = display_path(root);
                Err(Failure::Message(if doc.error.kind == "config" {
                    format!("mise secrets: fnox could not load its config in {root}: {message}")
                } else {
                    format!(
                        "mise secrets: fnox env failed ({}) in {root}: {message}",
                        doc.error.kind
                    )
                }))
            }
            Err(_) => Err(Failure::NoJson(status.to_string())),
        };
    }
    let head: wire::Head = serde_json::from_slice(buf).map_err(|e| unparsable(buf, &e))?;
    if head.schema != 1 {
        return Err(Failure::Message(format!(
            "mise secrets: fnox {} sent env schema {}; this mise understands schema 1. Upgrade mise (mise self-update) or pin an older fnox.",
            head.fnox_version.as_deref().unwrap_or("(unknown version)"),
            head.schema
        )));
    }
    let doc: wire::DescribeDocument =
        serde_json::from_slice(buf).map_err(|e| unparsable(buf, &e))?;
    Ok(catalog(doc))
}

/// Names and descriptions come from fnox config and reach the terminal.
fn strip_control(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_control()).collect()
}

fn catalog(doc: wire::DescribeDocument) -> Catalog {
    let mut entries = IndexMap::new();
    for key in doc.keys {
        let Some(name) = SecretName::new(&key.key) else {
            debug!("ignoring a key with an invalid name from fnox");
            continue;
        };
        let (kind, mode) = if key.kind == "lease" {
            (
                KeyKind::Lease {
                    name: strip_control(&key.lease.unwrap_or_default()),
                },
                None,
            )
        } else {
            let mode = match key.env {
                Some(serde_json::Value::Bool(true)) => InjectMode::Shell,
                Some(serde_json::Value::String(s)) if s == "exec" => InjectMode::Exec,
                _ => InjectMode::Never,
            };
            (KeyKind::Secret, Some(mode))
        };
        entries.insert(
            name,
            CatalogEntry {
                kind,
                mode,
                as_file: key.as_file,
                injectable: key.injectable.exec,
                description: key.description.map(|d| strip_control(&d)),
            },
        );
    }
    Catalog {
        entries,
        profile: doc.profile,
        dynamic_leases: doc.dynamic_leases,
        tool_version: doc.fnox_version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const DOC: &str = r#"{"schema":1,"fnox_version":"1.38.0","profile":["dev"],"keys":[{"key":"DATABASE_URL","kind":"secret","env":true,"as_file":false,"description":"app\u0007 database\n","injectable":{"exec":true,"shell":true},"future":1},{"key":"SIGNING_KEY","kind":"secret","env":false,"as_file":false,"injectable":{"exec":false,"shell":false}},{"key":"AWS_ACCESS_KEY_ID","kind":"lease","lease":"aws\u0007x","injectable":{"exec":true,"shell":false}},{"key":"NEW_MODE","kind":"secret","env":"later","injectable":{"exec":false}},{"key":"bad-name","kind":"secret","env":true}],"dynamic_leases":[]}"#;

    fn source(profile: Option<&str>) -> FnoxSource {
        FnoxSource {
            id: SourceId {
                kind: "fnox",
                root: PathBuf::from("/p"),
                profile: profile.map(String::from),
            },
            bin: PathBuf::from("/bin/fnox"),
            env: EnvMap::new(),
        }
    }

    fn message(f: Failure) -> String {
        match f {
            Failure::Message(m) => m,
            Failure::NoJson(s) => format!("nojson {s}"),
        }
    }

    #[test]
    fn describe_argv_puts_global_flags_before_env() {
        assert_eq!(
            source(None).describe_argv(),
            [
                "--non-interactive",
                "--no-daemon",
                "env",
                "--json",
                "--describe"
            ]
        );
        assert_eq!(
            source(Some("prod")).describe_argv(),
            [
                "-P",
                "prod",
                "--non-interactive",
                "--no-daemon",
                "env",
                "--json",
                "--describe"
            ]
        );
    }

    #[test]
    fn parses_schema_1() {
        let c = interpret(true, "exit status: 0", DOC.as_bytes(), Path::new("/p"))
            .ok()
            .unwrap();
        assert_eq!(c.tool_version, "1.38.0");
        assert_eq!(c.profile, ["dev"]);
        assert_eq!(c.entries.len(), 4);
        let db = &c.entries[&SecretName::new("DATABASE_URL").unwrap()];
        assert_eq!(db.mode, Some(InjectMode::Shell));
        assert_eq!(db.description.as_deref(), Some("app database"));
        let lease = &c.entries[&SecretName::new("AWS_ACCESS_KEY_ID").unwrap()];
        assert_eq!(lease.mode, None);
        assert!(lease.injectable);
        assert_eq!(
            lease.kind,
            KeyKind::Lease {
                name: "awsx".into()
            }
        );
        assert_eq!(
            c.entries[&SecretName::new("SIGNING_KEY").unwrap()].mode,
            Some(InjectMode::Never)
        );
        let unknown = &c.entries[&SecretName::new("NEW_MODE").unwrap()];
        assert_eq!(unknown.mode, Some(InjectMode::Never));
        assert!(!unknown.injectable);
    }

    #[test]
    fn schema_2_is_s4() {
        let m = message(
            interpret(
                true,
                "exit status: 0",
                br#"{"schema":2,"fnox_version":"9.0.0"}"#,
                Path::new("/p"),
            )
            .err()
            .unwrap(),
        );
        assert!(m.contains("fnox 9.0.0 sent env schema 2"), "{m}");
    }

    #[test]
    fn error_documents_are_s6() {
        let m = message(
            interpret(
                false,
                "exit status: 1",
                br#"{"schema":1,"error":{"kind":"config","message":"bad toml"}}"#,
                Path::new("/p"),
            )
            .err()
            .unwrap(),
        );
        assert!(
            m.starts_with(&format!(
                "mise secrets: fnox could not load its config in {}: bad toml",
                display_path(Path::new("/p"))
            )),
            "{m}"
        );
        let m = message(
            interpret(
                false,
                "exit status: 1",
                br#"{"schema":1,"error":{"kind":"resolution","message":"nope"}}"#,
                Path::new("/p"),
            )
            .err()
            .unwrap(),
        );
        assert!(
            m.contains(&format!(
                "fnox env failed (resolution) in {}: nope",
                display_path(Path::new("/p"))
            )),
            "{m}"
        );
    }

    #[test]
    fn non_json_failure_carries_the_exit_status() {
        assert!(matches!(
            interpret(
                false,
                "exit status: 1",
                b"error: unrecognized subcommand env\n",
                Path::new("/p")
            ),
            Err(Failure::NoJson(s)) if s == "exit status: 1"
        ));
    }

    #[test]
    fn unparsable_output_never_echoes_input() {
        let bad = br#"{"schema":"s3cr3t-value"}"#;
        let m = message(
            interpret(true, "exit status: 0", bad, Path::new("/p"))
                .err()
                .unwrap(),
        );
        assert!(!m.contains("s3cr3t"), "{m}");
        assert!(m.contains("could not parse"), "{m}");
        let m = message(
            interpret(
                true,
                "exit status: 0",
                br#"{"schema":1,"keys":"s3cr3t"}"#,
                Path::new("/p"),
            )
            .err()
            .unwrap(),
        );
        assert!(!m.contains("s3cr3t"), "{m}");
    }

    fn sel(keys: &[&str]) -> KeySelection {
        KeySelection::Keys(keys.iter().map(|k| SecretName::new(k).unwrap()).collect())
    }

    #[test]
    fn resolve_argv_is_sorted_and_flags_depend_on_interactivity() {
        let s = source(Some("prod"));
        assert_eq!(
            s.resolve_argv(&sel(&["PEM_KEY", "DEPLOY_KEY", "DATABASE_URL"]), false),
            [
                "-P",
                "prod",
                "--non-interactive",
                "--no-daemon",
                "env",
                "--json",
                "--for",
                "exec",
                "--keys",
                "DATABASE_URL,DEPLOY_KEY,PEM_KEY"
            ]
        );
        assert_eq!(
            source(None).resolve_argv(&sel(&["A"]), true),
            ["env", "--json", "--for", "exec", "--keys", "A"]
        );
    }

    fn resolved(doc: &str, requested: &[&str]) -> Resolved {
        let requested = requested
            .iter()
            .map(|k| SecretName::new(k).unwrap())
            .collect();
        match interpret_resolve(
            true,
            "exit status: 0",
            doc.as_bytes(),
            &requested,
            &catalog_with_signing(),
            Path::new("/p"),
        ) {
            Ok(r) => r,
            Err(_) => panic!("resolve failed"),
        }
    }

    fn catalog_with_signing() -> Catalog {
        interpret(true, "exit status: 0", DOC.as_bytes(), Path::new("/p"))
            .ok()
            .unwrap()
    }

    #[test]
    fn unrequested_keys_are_dropped_g17_and_non_injectable_g19() {
        let r = resolved(
            r#"{"schema":1,"set":{"DATABASE_URL":"v1","EXTRA":"v2","SIGNING_KEY":"v3"},"files":{"OTHER":"v4"},"remove":["X"],"missing":["DATABASE_URL"]}"#,
            &["DATABASE_URL", "SIGNING_KEY"],
        );
        assert_eq!(
            r.set.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
            ["DATABASE_URL"]
        );
        assert_eq!(
            r.unrequested,
            BTreeSet::from(["EXTRA".to_string(), "OTHER".to_string()])
        );
        assert_eq!(
            r.not_injectable
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>(),
            ["SIGNING_KEY"]
        );
        assert!(r.files.is_empty());
        assert_eq!(r.remove, BTreeSet::from(["X".to_string()]));
        assert!(!format!("{r:?}").contains("v1"));
    }

    #[test]
    fn resolve_error_documents() {
        let requested = BTreeSet::new();
        let catalog = catalog_with_signing();
        let err = |doc: &str| match interpret_resolve(
            false,
            "exit status: 1",
            doc.as_bytes(),
            &requested,
            &catalog,
            Path::new("/p"),
        ) {
            Err(ResolveFailure::Error(e)) => e,
            _ => panic!("expected an error document"),
        };
        match err(
            r#"{"schema":1,"error":{"kind":"invalid_keys","message":"m","unknown":["A"],"suggestions":{"A":["B"]},"not_injectable":[{"key":"S","env":false}]}}"#,
        ) {
            ResolveError::Invalid {
                unknown,
                suggestions,
                not_injectable,
            } => {
                assert_eq!(unknown, ["A"]);
                assert_eq!(suggestions["A"], ["B"]);
                assert_eq!(not_injectable, ["S"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            err(r#"{"schema":1,"error":{"kind":"resolution","message":"nope"}}"#),
            ResolveError::Resolution(m) if m == "nope"
        ));
        assert!(matches!(
            err(r#"{"schema":1,"error":{"kind":"config","message":"bad"}}"#),
            ResolveError::Other(m) if m.contains("could not load its config")
        ));
    }

    #[test]
    fn fingerprint_covers_the_whole_env_without_embedding_it() {
        let with = |key: &str, value: &str| {
            let mut s = source(None);
            s.env.insert("PATH".into(), "/bin".into());
            s.env.insert(key.into(), value.into());
            s
        };
        let a = with("AWS_PROFILE", "staging-s3cr3t");
        assert_eq!(
            a.build_fingerprint(),
            with("AWS_PROFILE", "staging-s3cr3t").build_fingerprint()
        );
        assert_ne!(
            a.build_fingerprint(),
            with("AWS_PROFILE", "prod-s3cr3t").build_fingerprint()
        );
        assert!(!a.build_fingerprint().contains("s3cr3t"));
    }

    #[test]
    fn subproject_env_reaches_the_source_env() {
        let mut tool_env = EnvMap::from([("ROOT".into(), "1".into()), ("GONE".into(), "1".into())]);
        let mut removals = BTreeSet::from(["AWS_PROFILE".to_string()]);
        let sub = (
            EnvMap::from([("AWS_PROFILE".into(), "staging".into())]),
            BTreeSet::from(["GONE".to_string()]),
        );
        apply_config_env(&mut tool_env, &mut removals, Some(sub));
        let out = source_env(EnvMap::new(), tool_env, &removals);
        assert_eq!(out.get("AWS_PROFILE").map(String::as_str), Some("staging"));
        assert_eq!(out.get("ROOT").map(String::as_str), Some("1"));
        assert!(!out.contains_key("GONE"));
        let mut untouched = EnvMap::from([("ROOT".into(), "1".into())]);
        apply_config_env(&mut untouched, &mut BTreeSet::new(), None);
        assert_eq!(untouched.len(), 1);
    }

    #[test]
    fn source_env_drops_mise_keys_and_shims() {
        let key = env::PATH_KEY.clone();
        let shims = crate::dirs::shims().to_string_lossy().to_string();
        let base = EnvMap::from([
            (
                key.clone(),
                format!("{shims}{}/usr/bin", if cfg!(windows) { ';' } else { ':' }),
            ),
            ("__MISE_DIFF".into(), "x".into()),
            ("__MISE_SESSION".into(), "x".into()),
            ("KEEP".into(), "1".into()),
            ("DROP".into(), "1".into()),
        ]);
        let overlay = EnvMap::from([("__MISE_ENV_CACHE_KEY".into(), "x".into())]);
        let removals = BTreeSet::from(["DROP".to_string()]);
        let out = source_env(base, overlay, &removals);
        assert_eq!(out.get("KEEP").map(String::as_str), Some("1"));
        assert!(!out.keys().any(|k| k.starts_with("__MISE_")));
        assert!(!out.contains_key("DROP"));
        assert_eq!(out[&key], "/usr/bin");
    }
}
