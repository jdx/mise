//! Register changed definitions and emit non-blocking pitchfork session commands.
use super::runtime::{self, Runtime};
use crate::config::{Config, Settings};
use crate::env_diff::EnvMap;
use crate::shell::Shell;
use crate::toolset::Toolset;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

const SESSION_ENV: &str = "__MISE_DAEMON_SESSIONS";

/// Shell-local bookkeeping travels with the existing activation environment.
/// Pitchfork owns process liveness, sessions, readiness, and autostop.
#[derive(Default, Serialize, Deserialize)]
struct Sessions {
    pid: u32,
    roots: BTreeMap<PathBuf, PathBuf>,
}

pub(crate) async fn emit(
    config: &Arc<Config>,
    ts: &Toolset,
    env: &EnvMap,
    pid: Option<u32>,
    shell: &dyn Shell,
    force: bool,
) -> Result<String> {
    if Settings::no_hooks() || Settings::safe_mode() || !Settings::get().experimental {
        return Ok(String::new());
    }
    let set = config.daemons()?;
    let Some(pid) = pid.filter(|p| *p > 0) else {
        if set.auto() {
            hint!(
                "daemons-activation",
                "[daemons] auto lifecycle requires updated shell activation",
                "restart your shell or evaluate mise activate again"
            );
        }
        return Ok(String::new());
    };
    let previous: Sessions = crate::env::var(SESSION_ENV)
        .ok()
        .and_then(|s| crate::hook_env::deserialize(s).ok())
        .filter(|s: &Sessions| s.pid == pid)
        .unwrap_or_default();
    let mut next = Sessions {
        pid,
        // Retain visited roots so a failed departure can be retried on the next
        // directory/config change or forced hook, without disabling the prompt fast path.
        roots: previous.roots.clone(),
    };
    let mut output = String::new();
    let roots: Vec<_> = set
        .roots()
        .into_iter()
        .filter(|root| {
            let scoped = set.for_root(root);
            // Preparing a root rewrites its generated pitchfork file whole, and
            // this project only knows the daemons it imported from another one.
            // Registering that here would deregister the sibling's own daemons,
            // so automatic lifecycle stops at this project's boundary.
            scoped.auto() && !scoped.daemons.values().any(|daemon| daemon.imported)
        })
        .collect();
    for (root, bin) in &previous.roots {
        if !roots.contains(root) {
            output.push_str(&session_command(bin, root, pid, false));
        }
    }
    for root in roots {
        let result = async {
            let scoped_set = set.for_root(&root);
            let previous_state = runtime::read_state(&root)?;
            let search_path = std::env::join_paths(
                env.get(&*crate::env::PATH_KEY)
                    .into_iter()
                    .flat_map(std::env::split_paths)
                    .filter(|path| path != &crate::dirs::shims()),
            )?;
            let bin = which::which_in("pitchfork", Some(search_path), &root)
                .ok()
                .or_else(|| previous_state.bin.is_file().then_some(previous_state.bin))
                .ok_or_else(|| {
                    eyre::eyre!("auto lifecycle requires pitchfork; run mise use pitchfork")
                })?;
            let runtime = Runtime {
                bin,
                env: env.clone(),
            };
            runtime::validate_tools(&scoped_set, config, ts).await?;
            // Task references belong to the declaring project's task list, the
            // same scope the other two callers validate against. Loading that
            // scope costs a config hierarchy read, so only do it when a daemon
            // actually names a task; this runs on every prompt.
            if scoped_set.daemons.values().any(|d| d.task.is_some()) {
                let scoped_config = runtime::config_for_root(config, &root).await?;
                scoped_set.validate_tasks(&scoped_config).await?;
            }
            // Pitchfork starts the `auto = ["start"]` daemons as soon as this
            // session joins, so the same check the other two registration paths
            // make belongs here: a daemon whose dependency was dropped with an
            // unreadable import would otherwise start without what it declared.
            let auto_starting = scoped_set.auto_starting();
            super::ensure_not_blocked(&scoped_set, &auto_starting, Some(&root))?;
            // The daemons this hook starts, dependencies included: pitchfork
            // brings those up too, so their ports are about to be bound and
            // belong in the conflict check.
            let auto_start: Vec<String> = auto_starting
                .daemons
                .values()
                .map(|daemon| daemon.name.clone())
                .collect();
            // Only this project's own roots reach here, so it owns the profile.
            let (_state, _lock) = runtime
                .prepare(&root, &scoped_set, force, true, &auto_start)
                .await?;
            Ok::<_, eyre::Report>(runtime.bin)
        }
        .await;
        match result {
            Ok(bin) => {
                output.push_str(&session_command(&bin, &root, pid, true));
                next.roots.insert(root, bin);
            }
            Err(err) => {
                hint!(
                    "daemons-auto",
                    "daemon auto lifecycle: {err:#}; inspect with",
                    "mise daemons status"
                );
                // Preserve ownership so departure can still release an existing session.
                if let Some(bin) = previous.roots.get(&root) {
                    next.roots.insert(root, bin.clone());
                }
            }
        }
    }
    if !next.roots.is_empty() || !previous.roots.is_empty() {
        output.push_str(&shell.set_env(SESSION_ENV, &crate::hook_env::serialize(&next)?));
    }
    Ok(output)
}

fn session_command(bin: &std::path::Path, root: &std::path::Path, pid: u32, enter: bool) -> String {
    let action = if enter { "enter" } else { "leave" };
    let command = format!(
        "unset PITCHFORK_CONFIG; {} project {action} --pid {pid} --directory {} >/dev/null 2>&1 || printf '%s\\n' 'mise: daemon session {action} failed; change directories or re-evaluate mise hook-env --force to retry; inspect mise daemons logs' >&2",
        super::presets::quote(bin.to_string_lossy()),
        super::presets::quote(root.to_string_lossy()),
    );
    // Background inside a non-interactive sh, not the interactive activation
    // shell, so it does not add a job or print job-control notifications.
    format!(
        "command sh -c {}\n",
        super::presets::quote(format!("({command}) </dev/null >/dev/null &"))
    )
}
