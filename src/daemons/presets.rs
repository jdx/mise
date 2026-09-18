use super::ports::PortClaim;
use super::{Daemon, state_dir};
use eyre::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PRESETS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/daemon_presets.rs"));

#[derive(Deserialize)]
struct Preset {
    tool: String,
    port: u16,
    binary: String,
    options: toml::Table,
    daemon: toml::Table,
    exports: IndexMap<String, String>,
}

fn preset(name: &str) -> Result<Preset> {
    let content = PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| s)
        .ok_or_else(|| {
            eyre::eyre!(
                "unknown daemon preset {name:?}; available presets: {}",
                PRESETS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    Ok(toml::from_str(content)?)
}

/// The well-known port a preset binds when nothing overrides it, and the base
/// that `port = "auto"` offsets per worktree.
pub(crate) fn default_port(name: &str) -> Result<u16> {
    Ok(preset(name)?.port)
}

pub(crate) fn quote(value: impl AsRef<str>) -> String {
    format!("'{}'", value.as_ref().replace('\'', "'\\''"))
}

/// Wrap a command so it runs inside the project's tool environment. Needed
/// wherever pitchfork is told not to wrap the daemon itself in `mise x`.
pub(crate) fn in_tool_env(command: &str) -> String {
    format!(
        "{} x -- sh -c {}",
        quote(crate::env::MISE_BIN.to_string_lossy()),
        quote(command)
    )
}

/// Chain idempotent setup steps in front of the long-running command, using
/// pitchfork's shell-command semantics: every step must succeed before the
/// process that keeps running is reached. Returns `run` unchanged when there is
/// nothing to set up.
pub(crate) fn with_init(steps: &[String], run: &str) -> String {
    if steps.is_empty() {
        return run.to_string();
    }
    format!("{} && {run}", steps.join(" && "))
}

/// The declaration fields mise interprets itself rather than forwarding to
/// pitchfork: setup steps to run before the daemon, the port already resolved
/// for this project root, and the hostname components that root contributes.
pub(crate) struct Extras<'a> {
    pub init: &'a [String],
    pub port: Option<PortClaim>,
    pub labels: &'a super::urls::RootLabels,
    /// True when this daemon belongs to a project referenced with `project =`.
    /// Its exports go to that project's environment, not the one loading it.
    pub imported: bool,
}

pub(crate) fn expand(
    name: &str,
    preset_name: &str,
    version: &str,
    mut overrides: toml::Table,
    extras: Extras<'_>,
    source: &Path,
    root: &Path,
) -> Result<Daemon> {
    let mut preset = preset(preset_name)?;
    let tool = super::take_string(&mut overrides, "tool")?.unwrap_or_else(|| preset.tool.clone());
    // `port` is already parsed and resolved by the caller, which knows the
    // persisted allocation for this project root.
    let claim = extras.port.unwrap_or_else(|| PortClaim::fixed(preset.port));
    let port = claim.port;
    if let Some(options) = overrides.remove("options") {
        let options = options
            .as_table()
            .ok_or_else(|| eyre::eyre!("daemon options must be a table"))?;
        for (key, value) in options {
            if !preset.options.contains_key(key) {
                bail!("unknown {preset_name} option {key:?}");
            }
            preset.options.insert(key.clone(), value.clone());
        }
    }
    let database = preset
        .options
        .get("database")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| eyre::eyre!("options.database must be a string"))
        })
        .transpose()?
        .unwrap_or("postgres");
    // This name is also used as a URI component and SQL identifier.
    if database.is_empty()
        || !database
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        bail!("Postgres database must contain only letters, numbers, and underscores");
    }
    let data = state_dir(root).join("data").join(name);
    // Resolve the proxy before rendering, so a preset's own default label and a
    // user override both reach `{{ url }}`. `proxy` and `proxy_tls` are the two
    // override keys that have to be applied early; everything else in
    // `overrides` still wins at the end, where it cannot change the URL.
    let mut proxied = preset.daemon.clone();
    for key in ["proxy", "proxy_tls"] {
        if let Some(value) = overrides.get(key) {
            proxied.insert(key.into(), value.clone());
        }
    }
    // The resolved port has to be on the table before the proxy is read: a
    // daemon without one is never routed, and a preset always has one.
    proxied.insert("port".into(), super::expected_port(port));
    let proxy = super::urls::proxy_settings();
    let super::urls::Applied { host, .. } =
        super::urls::apply(name, &mut proxied, extras.labels, &proxy.tld)?;
    let mut context = tera::Context::new();
    context.insert("data", &quote(data.to_string_lossy()));
    context.insert("port", &port);
    context.insert("database", database);
    if let Some(host) = &host {
        context.insert("url", &proxy.url(host));
        context.insert("host", host);
    }
    let mut renderer = crate::tera::get_tera(Some(root));
    let mut table = proxied;
    for (_, value) in table.iter_mut() {
        if let toml::Value::String(text) = value {
            *text = crate::tera::render_str(&mut renderer, text, &context)?;
        }
    }
    if let Some(toml::Value::String(command)) = table.get_mut("ready_cmd") {
        *command = in_tool_env(command.as_str());
    }
    let mut exports = preset.exports;
    for (key, value) in exports.iter_mut() {
        *value = crate::tera::render_str(&mut renderer, value, &context).map_err(|err| {
            // `url` and `host` are the only context values a declaration can
            // take away, so name that cause rather than reporting a bare
            // template error about an undefined variable.
            if host.is_none() {
                eyre::eyre!(
                    "[daemons.{name}] sets proxy = false, but the {preset_name} preset derives {key} from the proxy hostname; drop proxy = false, or set {key} yourself in [env]"
                )
            } else {
                eyre::Report::from(err)
            }
        })?;
    }
    // Overrides use pitchfork's shell-command semantics verbatim. In particular,
    // callers can use `setup && exec server` without an extra shell or an `exec`
    // prefix that would terminate the shell before the second command.
    let run = super::take_string(&mut overrides, "run")?.unwrap_or_else(|| {
        format!(
            "exec {}",
            table.get("run").and_then(toml::Value::as_str).unwrap()
        )
    });
    // Database initialization always comes first; user `init` steps run after
    // it, once the data directory exists.
    let mut steps = vec![format!(
        "{} daemons __init {} {} {}",
        quote(crate::env::MISE_BIN.to_string_lossy()),
        quote(preset_name),
        quote(data.to_string_lossy()),
        quote(database)
    )];
    steps.extend(extras.init.iter().cloned());
    table.insert("run".into(), toml::Value::String(with_init(&steps, &run)));
    table.insert("port".into(), super::expected_port(port));
    table.insert("mise".into(), toml::Value::Boolean(true));
    // `proxy` and `proxy_tls` were already normalized above; re-applying the raw
    // override here would undo that.
    overrides.remove("proxy");
    overrides.remove("proxy_tls");
    table.extend(overrides);
    Ok(Daemon {
        name: name.into(),
        source: source.into(),
        root: root.into(),
        table,
        preset: Some(preset_name.into()),
        task: None,
        tool: Some((tool, version.into())),
        exports,
        imported: extras.imported,
        port: Some(claim),
        host,
    })
}

/// Database compatibility is preset-specific; tool request strings remain opaque.
fn major(preset: &str, output: &str) -> Result<String> {
    let version = match preset {
        "postgres" => output.trim().strip_prefix("postgres (PostgreSQL) "),
        "redis" => output
            .split_whitespace()
            .find_map(|part| part.strip_prefix("v=")),
        _ => None,
    }
    .ok_or_else(|| eyre::eyre!("unrecognized {preset} version output: {output}"))?;
    let major = version.split('.').next().unwrap_or_default();
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        bail!("unrecognized {preset} version: {version}");
    }
    Ok(major.into())
}

pub(crate) fn initialize(preset_name: &str, data: &Path, database: &str) -> Result<()> {
    crate::config::Settings::get().ensure_experimental("daemon presets")?;
    crate::config::Settings::ensure_not_safe("initializing daemon data")?;
    if cfg!(windows) {
        bail!("daemon presets are not supported on Windows yet");
    }
    let preset = preset(preset_name)?;
    let parent = data
        .parent()
        .ok_or_else(|| eyre::eyre!("daemon data must have a parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let _lock = crate::lock_file::LockFile::new(data).lock()?;
    let output = Command::new(&preset.binary).arg("--version").output()?;
    if !output.status.success() {
        bail!("unable to determine {} version", preset.binary);
    }
    let major = major(preset_name, &String::from_utf8(output.stdout)?)?;
    let marker = format!("{preset_name}:{major}\n");
    if data.exists() {
        let existing = std::fs::read_to_string(data.join(".mise-daemon-version"))
            .wrap_err("daemon data was not initialized completely; inspect it and migrate or reset it explicitly")?;
        if existing != marker {
            bail!(
                "incompatible {preset_name} data in {} (stored {}, current {}); migrate or reset it explicitly",
                data.display(),
                existing.trim(),
                marker.trim()
            );
        }
        if preset_name == "postgres"
            && std::fs::read_to_string(data.join("PG_VERSION"))
                .wrap_err("daemon data was not initialized completely; missing PG_VERSION")?
                .trim()
                != major
        {
            bail!("Postgres data version does not match its initialization marker");
        }
        return Ok(());
    }
    let staging = tempfile::Builder::new()
        .prefix(".mise-init-")
        .tempdir_in(parent)?;
    let staged_data: PathBuf = staging.path().join("data");
    if preset_name == "postgres" {
        let status = Command::new("initdb")
            .args(["-D"])
            .arg(&staged_data)
            .args(["-U", "postgres", "--auth=trust"])
            .status()?;
        if !status.success() {
            bail!("Postgres initialization failed; existing data was preserved");
        }
        if database != "postgres" {
            if database.is_empty()
                || !database
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                bail!("invalid database name");
            }
            let mut child = Command::new("postgres")
                .arg("--single")
                .arg("-D")
                .arg(&staged_data)
                .arg("postgres")
                .stdin(Stdio::piped())
                .spawn()?;
            writeln!(
                child.stdin.take().unwrap(),
                "CREATE DATABASE \"{database}\";"
            )?;
            if !child.wait()?.success() {
                bail!("failed to create database {database}");
            }
        }
        if std::fs::read_to_string(staged_data.join("PG_VERSION"))?.trim() != major {
            bail!("initdb and postgres have incompatible versions");
        }
    } else {
        std::fs::create_dir(&staged_data)?;
    }
    std::fs::write(staged_data.join(".mise-daemon-version"), marker)?;
    std::fs::rename(staged_data, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> super::super::urls::RootLabels {
        super::super::urls::RootLabels {
            project: Some("shop".into()),
            worktree: Some("main".into()),
        }
    }
    #[test]
    fn presets_render_local_persistent_daemons() {
        for (name, _) in PRESETS {
            let daemon = expand(
                name,
                name,
                "latest",
                toml::Table::new(),
                Extras {
                    init: &[],
                    port: None,
                    labels: &labels(),
                    imported: false,
                },
                Path::new("/project/mise.toml"),
                Path::new("/project"),
            )
            .unwrap();
            let run = daemon.table["run"].as_str().unwrap();
            assert!(run.contains(" daemons __init "));
            assert!(run.contains("127.0.0.1"));
            assert!(!run.contains("/latest"));
            assert_eq!(daemon.table["port"]["bump"].as_bool(), Some(false));
        }
    }
    #[test]
    fn named_instances_override_ports_and_keep_templates() {
        let overrides = toml::toml! { ready_cmd = "echo {{ env.FOO }}" };
        let daemon = expand(
            "analytics",
            "postgres",
            "18",
            overrides,
            Extras {
                init: &[],
                port: Some(PortClaim::fixed(5433)),
                labels: &labels(),
                imported: false,
            },
            Path::new("/p/mise.toml"),
            Path::new("/p"),
        )
        .unwrap();
        assert_eq!(daemon.exports["PGPORT"], "5433");
        assert_eq!(
            daemon.table["ready_cmd"].as_str(),
            Some("echo {{ env.FOO }}")
        );
        assert!(daemon.table["run"].as_str().unwrap().contains("analytics"));
    }
    #[test]
    fn compatibility_uses_actual_database_versions() {
        assert_eq!(
            major("postgres", "postgres (PostgreSQL) 18.6\n").unwrap(),
            "18"
        );
        assert_eq!(
            major("redis", "Redis server v=8.10.1 sha=abc").unwrap(),
            "8"
        );
        assert!(major("postgres", "nightly").is_err());
        assert_eq!(quote("a'b"), "'a'\\''b'");
    }
}
