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

pub(crate) fn quote(value: impl AsRef<str>) -> String {
    format!("'{}'", value.as_ref().replace('\'', "'\\''"))
}

pub(crate) fn expand(
    name: &str,
    preset_name: &str,
    version: &str,
    mut overrides: toml::Table,
    source: &Path,
    root: &Path,
) -> Result<Daemon> {
    let mut preset = preset(preset_name)?;
    let port = overrides
        .remove("port")
        .map(|v| {
            v.as_integer()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    eyre::eyre!("[daemons.{name}].port must be an integer from 1 to 65535")
                })
        })
        .transpose()?
        .unwrap_or(preset.port);
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
    let mut context = tera::Context::new();
    context.insert("data", &quote(data.to_string_lossy()));
    context.insert("port", &port);
    context.insert("database", database);
    let mut renderer = crate::tera::get_tera(Some(root));
    let mut table = preset.daemon;
    for (_, value) in table.iter_mut() {
        if let toml::Value::String(text) = value {
            *text = crate::tera::render_str(&mut renderer, text, &context)?;
        }
    }
    if let Some(toml::Value::String(command)) = table.get_mut("ready_cmd") {
        *command = format!(
            "{} x -- sh -c {}",
            quote(crate::env::MISE_BIN.to_string_lossy()),
            quote(command.as_str())
        );
    }
    let mut exports = preset.exports;
    for value in exports.values_mut() {
        *value = crate::tera::render_str(&mut renderer, value, &context)?;
    }
    let run = table.get("run").and_then(toml::Value::as_str).unwrap();
    table.insert(
        "run".into(),
        toml::Value::String(format!(
            "{} daemons __init {} {} {} && exec {run}",
            quote(crate::env::MISE_BIN.to_string_lossy()),
            quote(preset_name),
            quote(data.to_string_lossy()),
            quote(database)
        )),
    );
    table.insert(
        "port".into(),
        toml::Value::Table(toml::Table::from_iter([
            (
                String::from("expect"),
                toml::Value::Array(vec![toml::Value::Integer(i64::from(port))]),
            ),
            (String::from("bump"), toml::Value::Boolean(false)),
        ])),
    );
    table.insert("mise".into(), toml::Value::Boolean(true));
    table.extend(overrides);
    Ok(Daemon {
        name: name.into(),
        source: source.into(),
        root: root.into(),
        table,
        preset: Some(preset_name.into()),
        tool: Some((preset.tool, version.into())),
        exports,
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
            && std::fs::read_to_string(data.join("PG_VERSION"))?.trim() != major
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
    #[test]
    fn presets_render_local_persistent_daemons() {
        for (name, _) in PRESETS {
            let daemon = expand(
                name,
                name,
                "latest",
                toml::Table::new(),
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
        let overrides = toml::toml! { port = 5433 ready_cmd = "echo {{ env.FOO }}" };
        let daemon = expand(
            "analytics",
            "postgres",
            "18",
            overrides,
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
