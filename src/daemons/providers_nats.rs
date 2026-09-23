//! Account provisioning for mise-owned, loopback-only NATS providers.
use super::providers::{Execution, validate_resource};
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Serialize, Deserialize)]
pub(super) struct Credentials {
    user: String,
    password: String,
}

fn private_dir(root: &Path) -> Result<PathBuf> {
    let dir = root.join("nats-accounts");
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    Ok(dir)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| eyre::eyre!("missing credential directory"))?,
    )?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn credentials(root: &Path, resource: &str) -> Result<Credentials> {
    let dir = private_dir(root)?.join("credentials");
    std::fs::create_dir_all(&dir)?;
    let _lock = crate::lock_file::LockFile::at(&dir.join("credentials.lock")).lock()?;
    let path = dir.join(format!("{resource}.json"));
    if path.exists() {
        return Ok(serde_json::from_slice(&std::fs::read(path)?)?);
    }
    let credentials = Credentials {
        user: resource.into(),
        password: hex::encode(rand::random::<[u8; 32]>()),
    };
    write_private(&path, &serde_json::to_vec(&credentials)?)?;
    Ok(credentials)
}

pub(super) fn url(root: &Path, resource: &str, port: u16) -> Result<String> {
    validate_resource(resource)?;
    let credentials = credentials(root, resource)?;
    Ok(format!(
        "nats://{}:{}@127.0.0.1:{port}",
        credentials.user, credentials.password
    ))
}

/// User-supplied files and certificate mapping require an explicit integration;
/// never rewrite those configurations or silently fall back to a shared account.
pub(super) fn configure(table: &mut toml::Table, root: &Path) -> Result<()> {
    let options = table
        .entry("options".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| eyre::eyre!("NATS options must be a table"))?;
    for key in ["config", "tls_ca", "tls_cert", "tls_key"] {
        if options.get(key).is_some_and(|v| v.as_str() != Some("")) {
            bail!(
                "NATS providers require mise-managed loopback configuration; {key} is not supported for automatic accounts"
            );
        }
    }
    if options.get("jetstream").is_some_and(|v| !v.is_bool()) {
        bail!("NATS option jetstream must be a boolean");
    }
    // JetStream belongs in the file so reload cannot discard command-line state.
    options.remove("jetstream");
    options.insert(
        "config".into(),
        root.join("nats-accounts/server.json")
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    Ok(())
}

fn configuration(execution: &Execution) -> Result<Vec<u8>> {
    let dir = private_dir(&execution.root)?;
    let control = credentials(&execution.root, "_mise_control")?;
    let mut accounts = BTreeMap::new();
    accounts.insert(
        "_mise_control".to_string(),
        serde_json::json!({"users": [control]}),
    );
    for entry in std::fs::read_dir(dir.join("credentials"))? {
        let path = entry?.path();
        let Some(resource) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().and_then(|s| s.to_str()) != Some("json")
            || validate_resource(resource).is_err()
        {
            continue;
        }
        let credentials: Credentials = serde_json::from_slice(&std::fs::read(&path)?)?;
        let mut account = serde_json::json!({"users": [credentials]});
        if execution.jetstream {
            account["jetstream"] = "enabled".into();
        }
        accounts.insert(resource.to_string(), account);
    }
    let mut config = serde_json::json!({
        "server_name": "mise-provider",
        "pid_file": execution.root.join("nats-accounts/server.pid"),
        "accounts": accounts,
        "system_account": "_mise_control"
    });
    if execution.jetstream {
        config["jetstream"] = serde_json::json!({"store_dir": execution.data_dir});
    }
    Ok(serde_json::to_vec_pretty(&config)?)
}

async fn command(execution: &Execution, args: &[String]) -> Result<()> {
    let mut cmd = tokio::process::Command::new("nats-server");
    cmd.args(args)
        .env_clear()
        .envs(&execution.env)
        .current_dir(&execution.root)
        .kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await??;
    if !result.status.success() {
        bail!(
            "NATS configuration command failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}

/// Caller holds the resource lock. Validate before replacing a live configuration.
pub(super) async fn prepare(execution: &Execution) -> Result<()> {
    let content = configuration(execution)?;
    let dir = private_dir(&execution.root)?;
    let path = dir.join("server.json");
    if std::fs::read(&path).ok().as_deref() == Some(&content) {
        return Ok(());
    }
    let candidate = dir.join("candidate.conf");
    write_private(&candidate, &content)?;
    let result = command(
        execution,
        &[
            "-t".into(),
            "--config".into(),
            candidate.to_string_lossy().into_owned(),
        ],
    )
    .await;
    let _ = std::fs::remove_file(&candidate);
    result?;
    write_private(&path, &content)?;
    Ok(())
}

async fn ping(execution: &Execution, credentials: &Credentials) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let socket = tokio::net::TcpStream::connect(("127.0.0.1", execution.port)).await?;
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await?;
        if !line.starts_with("INFO ") { bail!("provider did not answer the NATS protocol"); }
        let connect = serde_json::json!({"verbose": false, "user": credentials.user, "pass": credentials.password});
        socket.get_mut().write_all(format!("CONNECT {connect}\r\nPING\r\n").as_bytes()).await?;
        for _ in 0..10 {
            line.clear();
            if socket.read_line(&mut line).await? == 0 { bail!("NATS closed the connection"); }
            match line.trim() {
                "PONG" => return Ok(()),
                "PING" => socket.get_mut().write_all(b"PONG\r\n").await?,
                value if value.starts_with("-ERR") => bail!("NATS account is not ready"),
                _ => {}
            }
        }
        bail!("NATS did not acknowledge account readiness")
    }).await?
}

pub(super) async fn provision(execution: &Execution, resource: &str) -> Result<()> {
    let credentials = credentials(&execution.root, resource)?;
    if ping(execution, &credentials).await.is_ok() {
        return Ok(());
    }
    let path = execution.root.join("nats-accounts/server.json");
    let previous = std::fs::read(&path)?;
    prepare(execution).await?;
    let args = vec![
        "--signal".into(),
        format!(
            "reload={}",
            execution.root.join("nats-accounts/server.pid").display()
        ),
    ];
    // A concurrent first start may not have written its PID file yet.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut reloaded = false;
    loop {
        if !reloaded && command(execution, &args).await.is_ok() {
            reloaded = true;
        }
        if ping(execution, &credentials).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            write_private(&path, &previous)?;
            let _ = command(execution, &args).await;
            bail!("NATS did not accept the new account; restored its previous configuration");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_stable_private_and_distinct() {
        let root = tempfile::tempdir().unwrap();
        let first = credentials(root.path(), "one").unwrap();
        assert_eq!(
            first.password,
            credentials(root.path(), "one").unwrap().password
        );
        assert_ne!(
            first.password,
            credentials(root.path(), "two").unwrap().password
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = root.path().join("nats-accounts/credentials/one.json");
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
