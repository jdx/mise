//! Shell hooks publish desired state; detached workers own slow readiness waits.
use super::{
    load,
    runtime::{self, Runtime},
};
use crate::config::{Config, Settings};
use crate::toolset::Toolset;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Desired {
    cwd: PathBuf,
    roots: Vec<PathBuf>,
    sync_roots: Vec<PathBuf>,
    profile: Vec<String>,
    fingerprint: String,
    bin: Option<PathBuf>,
}

fn session_dir(pid: u32) -> PathBuf {
    crate::dirs::STATE
        .join("daemons/sessions")
        .join(pid.to_string())
}

pub(crate) fn pending() -> bool {
    let args = crate::env::args_safe();
    let pid = args
        .windows(2)
        .find(|p| p[0] == "--shell-pid")
        .and_then(|p| p[1].parse::<u32>().ok());
    pid.is_some_and(|pid| {
        let dir = session_dir(pid);
        dir.join("desired.json").exists()
            && std::fs::read(dir.join("desired.json")).ok()
                != std::fs::read(dir.join("done.json")).ok()
    })
}

pub(crate) async fn publish(config: &Arc<Config>, ts: &Toolset, pid: Option<u32>) {
    if let Err(err) = publish_inner(config, ts, pid).await {
        debug!("daemon hook: {err:#}");
    }
}

async fn publish_inner(config: &Arc<Config>, ts: &Toolset, pid: Option<u32>) -> Result<()> {
    if Settings::no_hooks() || Settings::safe_mode() || !Settings::get().experimental {
        return Ok(());
    }
    let Some(pid) = pid.filter(|p| *p > 0) else {
        return Ok(());
    };
    let dir = session_dir(pid);
    let set = load(&config.config_files)?;
    if set.daemons.is_empty() && !dir.join("desired.json").exists() {
        return Ok(());
    }
    let roots: Vec<_> = set
        .roots()
        .into_iter()
        .filter(|root| set.for_root(root).auto())
        .collect();
    let mut sync_roots: Vec<_> = set
        .roots()
        .into_iter()
        .filter(|r| roots.contains(r) || super::state_dir(r).join("state.json").exists())
        .collect();
    if let Some(root) = &config.project_root
        && super::state_dir(root).join("state.json").exists()
        && !sync_roots.contains(root)
    {
        sync_roots.push(root.clone());
    }
    let bin = Runtime::from_toolset(config, ts, None)
        .await
        .ok()
        .map(|r| r.bin);
    if bin.is_none() && !roots.is_empty() {
        hint!(
            "daemons-pitchfork",
            "[daemons] auto-start requires pitchfork",
            "mise use pitchfork"
        );
    }
    let desired = Desired {
        cwd: crate::dirs::CWD.clone().unwrap_or_default(),
        roots,
        sync_roots,
        profile: crate::env::MISE_ENV.clone(),
        fingerprint: crate::hash::hash_to_str(&format!("{set:?}")),
        bin,
    };
    let content = serde_json::to_vec(&desired)?;
    if std::fs::read(dir.join("done.json")).ok().as_deref() == Some(&content)
        && std::fs::read(dir.join("desired.json")).ok().as_deref() == Some(&content)
    {
        return Ok(());
    }
    runtime::write_if_changed(&dir.join("desired.json"), &content)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("worker.log"))?;
    let mut command = std::process::Command::new(&*crate::env::MISE_BIN);
    command
        .args(["daemons", "__reconcile", &pid.to_string()])
        .env("MISE_OFFLINE", "1")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn()?;
    Ok(())
}

pub(crate) async fn reconcile(pid: u32) -> Result<()> {
    Settings::get().ensure_experimental("daemon hooks")?;
    Settings::ensure_not_safe("managing daemon sessions")?;
    if Settings::no_hooks() {
        return Ok(());
    }
    let dir = session_dir(pid);
    // Workers wait off the prompt path: the newest hook must not lose its update
    // merely because an older worker is still waiting for readiness.
    let _lock = crate::lock_file::LockFile::at(&dir.join("worker.lock")).lock()?;
    let content = std::fs::read(dir.join("desired.json"))?;
    let desired: Desired = serde_json::from_slice(&content)?;
    let actual_path = dir.join("actual.json");
    let mut actual: BTreeMap<PathBuf, PathBuf> = if actual_path.exists() {
        serde_json::from_slice(&std::fs::read(&actual_path)?)?
    } else {
        BTreeMap::new()
    };
    // Departure needs no old config parsing and no executable on the new project's PATH.
    for (root, bin) in actual.clone() {
        if !desired.roots.contains(&root) {
            let runtime = Runtime {
                bin,
                env: Default::default(),
            };
            if runtime.supervisor_up(&root).await? {
                runtime.session(&root, pid, false).await?;
            }
            actual.remove(&root);
            runtime::write_if_changed(&actual_path, &serde_json::to_vec(&actual)?)?;
        }
    }
    if desired.profile != *crate::env::MISE_ENV || Some(&desired.cwd) != crate::dirs::CWD.as_ref() {
        return Ok(());
    }
    let config = Config::get().await?;
    for root in &desired.sync_roots {
        // A newer hook will launch another worker; never enter its old desired state.
        if std::fs::read(dir.join("desired.json"))? != content {
            return Ok(());
        }
        let scoped = runtime::config_for_root(&config, root).await?;
        let set = load(&scoped.config_files)?.for_root(root);
        let ts = crate::toolset::ToolsetBuilder::new()
            .with_resolve_options(crate::toolset::ResolveOptions {
                offline: true,
                ..Default::default()
            })
            .build(&scoped)
            .await?;
        let previous = runtime::read_state(root)?;
        let runtime = Runtime::from_toolset(
            &scoped,
            &ts,
            desired
                .bin
                .as_deref()
                .or_else(|| actual.get(root).map(PathBuf::as_path))
                .or(Some(previous.bin.as_path())),
        )
        .await?;
        runtime::validate_tools(&set, &scoped, &ts).await?;
        let (_state, _project_lock) = runtime.prepare(root, &set).await?;
        if set.auto() {
            runtime.session(root, pid, true).await?;
        }
        if set.auto() {
            actual.insert(root.clone(), runtime.bin.clone());
        }
        runtime::write_if_changed(&actual_path, &serde_json::to_vec(&actual)?)?;
        if std::fs::read(dir.join("desired.json"))? != content {
            let latest: Desired =
                serde_json::from_slice(&std::fs::read(dir.join("desired.json"))?)?;
            if !latest.roots.contains(root) {
                runtime.session(root, pid, false).await?;
                actual.remove(root);
                runtime::write_if_changed(&actual_path, &serde_json::to_vec(&actual)?)?;
            }
            return Ok(());
        }
    }
    runtime::write_if_changed(&dir.join("done.json"), &content)?;
    Ok(())
}
