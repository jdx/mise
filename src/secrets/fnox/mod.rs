//! The fnox source. fnox is named only here and in `mise doctor`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use eyre::{Result, bail};
use indexmap::IndexMap;
use tokio::io::AsyncReadExt;

use super::SecretName;
use super::config::SelectedSource;
use super::source::{Catalog, CatalogEntry, InjectMode, KeyKind, SecretSource, SourceId};
use crate::config::Config;
use crate::env;
use crate::env_diff::EnvMap;
use crate::file::{self, display_path};

mod wire;

/// Message text only: never compare versions (versions are not necessarily semver).
// TODO: confirm once the fnox release containing jdx/fnox#936 is cut
pub(crate) const FNOX_ENV_MIN_VERSION: &str = "1.39.0";

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
    if let Ok(ts) = config.get_toolset().await
        && let Some(bin) = ts.which_bin_spawnable(config, "fnox").await
    {
        return Some(bin);
    }
    // Absolute against mise's cwd: fnox runs from the source root, so a relative PATH hit
    // would resolve somewhere else. Not canonicalized, so a multi-call binary keeps its name.
    crate::backend::which_no_shims_spawnable("fnox").map(|p| std::path::absolute(&p).unwrap_or(p))
}

impl FnoxSource {
    pub(crate) async fn new(config: &Arc<Config>, selected: &SelectedSource) -> Result<Self> {
        let declared_in = &selected.declared_in[0];
        let Some(bin) = find_binary(config).await else {
            bail!(
                "mise secrets: fnox not found\n  [secrets.fnox] in {} needs the fnox CLI. Add it to the project: mise use fnox\n  mise looks in the project's tools first, then on PATH.",
                display_path(declared_in)
            );
        };
        let root = dunce::canonicalize(&selected.root).map_err(|e| {
            eyre::eyre!(
                "mise secrets: cannot access {}: {e}",
                display_path(&selected.root)
            )
        })?;
        let (tool_env, removals) = config
            .get_toolset()
            .await?
            .env_with_path_and_removals(config)
            .await?;
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

    fn argv(&self, rest: &[&str]) -> Vec<String> {
        let mut argv = vec![];
        if let Some(profile) = &self.id.profile {
            argv.push("-P".to_string());
            argv.push(profile.clone());
        }
        argv.push("--non-interactive".to_string());
        argv.push("--no-daemon".to_string());
        argv.extend(rest.iter().map(|s| s.to_string()));
        argv
    }

    fn describe_argv(&self) -> Vec<String> {
        self.argv(&["env", "--json", "--describe"])
    }

    fn command(&self, args: &[String]) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.bin);
        cmd.args(args)
            .env_clear()
            .envs(&self.env)
            .current_dir(&self.id.root)
            .stdin(Stdio::null())
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
