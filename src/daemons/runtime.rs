use super::ports::PortClaim;
use super::{DaemonSet, DaemonSettings, state_dir};
use crate::cli::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::env_diff::EnvMap;
use crate::toolset::{ToolRequest, ToolSource, Toolset, ToolsetBuilder};
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

/// What mise knows about one project's daemons, as written beside them.
///
/// Every field defaults, so a state file written by another version still
/// parses. The alternative is that adding a field turns every existing
/// `state.json` into something `mise daemons prune` cannot read, and therefore
/// into data nothing will ever clean up. What the missing fields cost is
/// bounded: an empty root is never selected for removal, and an empty id list
/// only means there is nothing to stop.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct State {
    pub root: PathBuf,
    pub profile: Vec<String>,
    pub namespace: String,
    pub ids: Vec<String>,
    pub bin: PathBuf,
    pub config_hash: String,
    /// Ports allocated per daemon name. Persisting them keeps an `auto`
    /// allocation stable across a change to the slot derivation, and lets other
    /// project roots on this machine detect a conflict before starting.
    #[serde(default)]
    pub ports: BTreeMap<String, PortClaim>,
    /// Fingerprint of the other projects' state files as of the last conflict
    /// check, recorded only when that check found no neighbour claiming any of
    /// this project's ports. A shell hook runs on every prompt, so recognising
    /// that unchanged case from metadata avoids re-reading and parsing each
    /// file. Empty means the last check was not clear and must be repeated.
    #[serde(default)]
    pub ports_scan: String,
}

/// Cheap summary of the other projects' state files: their names, sizes and
/// modification times, without opening any of them.
///
/// This is a hint for skipping redundant work on the shell-hook path, not a
/// guarantee: a rewrite of the same length within one timestamp tick looks
/// unchanged. Missing one costs a delayed hint rather than a wrong port, and an
/// explicit start scans regardless.
fn siblings_fingerprint(mine: &Path) -> String {
    let entries = std::fs::read_dir(crate::dirs::STATE.join("daemons"));
    let mut seen: Vec<String> = entries
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path() != mine)
        .filter_map(|e| {
            let state = e.path().join("state.json");
            let meta = std::fs::metadata(&state).ok()?;
            let stamp = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            Some(format!("{}:{}:{stamp}", state.display(), meta.len()))
        })
        .collect();
    seen.sort();
    crate::hash::hash_to_str(&seen)
}

pub(crate) struct Runtime {
    pub bin: PathBuf,
    pub env: EnvMap,
}

pub(crate) fn read_state(root: &Path) -> Result<State> {
    let path = state_dir(root).join("state.json");
    if !path.exists() {
        return Ok(State {
            root: root.into(),
            ..State::default()
        });
    }
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

/// Fail before registering configuration when another project root on this
/// machine is *running* a daemon on one of our ports. Two checkouts landing on
/// the same port is otherwise silent: the second daemon fails to bind, or worse,
/// connects to the first one's data.
///
/// Liveness matters. A `state.json` outlives the daemon it describes, so a
/// stopped project must not hold a port hostage; before this check existed, two
/// projects could take turns on the default 5432 and that has to keep working.
/// The cheap scan therefore only selects candidates, and the liveness probe runs
/// solely for a root whose port actually matches.
///
/// This is a diagnostic, not a reservation. Because a recorded claim confers
/// nothing until its daemon is actually serving, two projects starting at the
/// same instant can both see the port free. Binding is the real arbiter, and the
/// loser still gets its own bind error; the check exists to replace that opaque
/// failure with one naming the other project whenever it can.
/// The claims to record for a project, carrying forward those of daemons that
/// are no longer declared.
///
/// A daemon dropped from the configuration keeps its id so it can still be
/// queried and stopped, which means it may still be running and holding its
/// port. Discarding its claim would hide that port from every other project's
/// scan, and the next project to resolve it would be told the port is free. A
/// claim is dropped only when the daemon is still declared and no longer has a
/// port mise resolves, since nothing manages one for it any more. Stale entries
/// cost nothing because a conflict is only reported once the daemon holding the
/// port answers as running.
fn carry_port_claims(
    previous: &BTreeMap<String, PortClaim>,
    set: &DaemonSet,
) -> BTreeMap<String, PortClaim> {
    let mut ports: BTreeMap<String, PortClaim> = previous
        .iter()
        .filter(|(name, _)| !set.daemons.contains_key(*name))
        .map(|(name, claim)| (name.clone(), *claim))
        .collect();
    ports.extend(
        set.daemons
            .values()
            .filter_map(|d| d.port.map(|claim| (d.name.clone(), claim))),
    );
    ports
}

/// The subset of a project's claims belonging to daemons this operation will
/// launch. Registration covers the whole project, so checking every claim would
/// let one project's Postgres on 5432 block `mise daemons start redis`, which
/// never binds that port.
fn ports_being_started(
    ports: &BTreeMap<String, PortClaim>,
    starting: &[String],
) -> BTreeMap<String, PortClaim> {
    ports
        .iter()
        .filter(|(name, _)| starting.iter().any(|s| s == *name))
        .map(|(name, claim)| (name.clone(), *claim))
        .collect()
}

fn claimed_ports(mine: &Path) -> Vec<(State, String, u16)> {
    let Ok(entries) = std::fs::read_dir(crate::dirs::STATE.join("daemons")) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir == mine || !dir.is_dir() {
            continue;
        }
        let Ok(other) = std::fs::read(dir.join("state.json")) else {
            continue;
        };
        let Ok(other) = serde_json::from_slice::<State>(&other) else {
            continue;
        };
        // A removed checkout keeps no claim; its ports are free to reuse.
        if !other.root.is_dir() {
            continue;
        }
        for (name, claim) in other.ports.clone() {
            found.push((other.clone(), name, claim.port));
        }
    }
    found
}

pub(crate) fn write_if_changed(path: &Path, content: &[u8]) -> Result<bool> {
    if std::fs::read(path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| eyre::eyre!("missing parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(content)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(true)
}

pub(crate) async fn config_for_root(config: &Arc<Config>, root: &Path) -> Result<Arc<Config>> {
    let (paths, idiomatic) = crate::config::load_config_hierarchy_from_dir(root).await?;
    let files = crate::config::load_config_files_from_paths(&paths, &idiomatic).await?;
    Ok(config.with_config_files(files))
}

/// Build the daemon toolset without installing anything.
///
/// Kept separate from [`toolset`] so callers that must not install can avoid the
/// install path entirely. That path reaches code which is not `Send`, and a
/// caller inside a spawned task would not compile if this future contained it.
pub(crate) async fn toolset_resolved(
    config: &Arc<Config>,
    include_pitchfork: bool,
) -> Result<Toolset> {
    let pitchfork: ToolArg = "pitchfork".parse()?;
    let args = if include_pitchfork
        && !config
            .get_tool_request_set()
            .await?
            .tools
            .contains_key(&pitchfork.ba)
    {
        vec![pitchfork]
    } else {
        vec![]
    };
    // Resolve from what is on disk. `install_missing_versions` re-resolves the
    // specific requests it has to fetch, so an already-satisfied project costs
    // no network round trip -- which matters when this runs on every `mise run`
    // of a task that requires daemons.
    ToolsetBuilder::new()
        .with_args(&args)
        .with_default_to_latest(true)
        .with_resolve_options(crate::toolset::ResolveOptions {
            offline: true,
            ..Default::default()
        })
        .build(config)
        .await
}

pub(crate) async fn toolset(config: &Arc<Config>, install: bool) -> Result<(Arc<Config>, Toolset)> {
    let mut config = config.clone();
    let mut ts = toolset_resolved(&config, install).await?;
    if install {
        let (_, missing) = ts
            .install_missing_versions(&mut config, &Default::default())
            .await?;
        ts.notify_missing_versions(missing);
    }
    Ok((config, ts))
}

impl Runtime {
    pub(crate) async fn from_toolset(
        config: &Arc<Config>,
        ts: &Toolset,
        fallback: Option<&Path>,
    ) -> Result<Self> {
        let env = ts.env_with_path(config).await?;
        let bin = which::which_in(
            "pitchfork",
            env.get(&*crate::env::PATH_KEY),
            crate::dirs::CWD.as_deref().unwrap_or(Path::new(".")),
        )
        .ok()
        .or_else(|| fallback.filter(|p| p.is_file()).map(Path::to_path_buf))
        .ok_or_else(|| {
            eyre::eyre!(
                "daemons require pitchfork; run `mise use pitchfork` or `mise daemons start`"
            )
        })?;
        Ok(Self { bin, env })
    }

    pub(crate) async fn output(&self, root: &Path, args: &[String]) -> Result<String> {
        let output = self.raw_output(root, args).await?;
        if !output.status.success() {
            bail!(
                "pitchfork {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    /// Like [`Self::output`], but hands a failing command's output back instead
    /// of turning it into an error. For callers that can tell one failure from
    /// another, such as prune deciding whether pitchfork simply does not know
    /// about a daemon it was asked to forget.
    pub(crate) async fn raw_output(
        &self,
        root: &Path,
        args: &[String],
    ) -> Result<std::process::Output> {
        let mut command = Command::new(&self.bin);
        command
            .args(args)
            .envs(&self.env)
            .env_remove("PITCHFORK_CONFIG")
            .current_dir(root)
            .kill_on_drop(true);
        Ok(tokio::time::timeout(Duration::from_secs(15), command.output()).await??)
    }

    pub(crate) async fn supports_external_config(&self, root: &Path) -> Result<()> {
        // Probe read-only usage metadata, never an unknown command (pitchfork's fallback starts daemons).
        let usage = self.output(root, &["usage".into()]).await?;
        if !usage.lines().any(|line| {
            line.trim_start().starts_with("cmd config ")
                || line.trim_start().starts_with("cmd \"config\" ")
        }) {
            bail!(
                "pitchfork lacks external configuration support; upgrade to pitchfork 2.25.0 or later"
            );
        }
        Ok(())
    }

    pub(crate) async fn status(&self, root: &Path, id: &str) -> Result<serde_json::Value> {
        let out = self
            .output(root, &["status".into(), id.into(), "--json".into()])
            .await?;
        Ok(serde_json::from_str(&out)?)
    }

    pub(crate) async fn supervisor_up(&self, root: &Path) -> Result<bool> {
        let out = self
            .output(
                root,
                &["supervisor".into(), "status".into(), "--json".into()],
            )
            .await?;
        let status: serde_json::Value = serde_json::from_str(&out)?;
        match status["status"].as_str() {
            Some("up") => Ok(true),
            Some("down") => Ok(false),
            _ => bail!("cannot establish pitchfork supervisor status: {status}"),
        }
    }

    pub(crate) async fn active(&self, root: &Path, state: &State) -> Result<bool> {
        for id in &state.ids {
            if let Ok(value) = self.status(root, id).await
                && matches!(
                    value["status"].as_str(),
                    Some("running" | "waiting" | "stopping")
                )
            {
                return Ok(true);
            }
        }
        if self.supervisor_up(root).await? {
            let sessions: Vec<serde_json::Value> = serde_json::from_str(
                &self
                    .output(root, &["project".into(), "list".into(), "--json".into()])
                    .await?,
            )?;
            if sessions.iter().any(|s| {
                s["directory"]
                    .as_str()
                    .is_some_and(|d| Path::new(d) == root)
                    && s["liveness_status"] != "dead"
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// See [`claimed_ports`]: only a port that another root is actively serving
    /// is a conflict.
    /// `starting` names the daemons this operation will actually launch. Only
    /// their ports are checked: registration covers every daemon in the project,
    /// so checking all of them would let one project's Postgres on 5432 block
    /// `mise daemons start redis`, which never binds that port.
    async fn check_port_conflicts(
        &self,
        root: &Path,
        ports: &BTreeMap<String, PortClaim>,
        starting: &[String],
    ) -> Result<bool> {
        let claimed: Vec<_> = claimed_ports(&state_dir(root))
            .into_iter()
            .filter(|(other, _, _)| other.root != root)
            .collect();
        // Over every claim this project holds, not only the ones starting: when
        // no neighbour names any of them, no subset can conflict and no daemon
        // starting or stopping elsewhere can change that. It is the one answer
        // that stays true without re-asking, so it is the only one cached.
        let clear = !ports
            .values()
            .any(|claim| claimed.iter().any(|(_, _, port)| *port == claim.port));
        let ports = &ports_being_started(ports, starting);
        if ports.is_empty() {
            return Ok(clear);
        }
        for (other, other_name, port) in claimed {
            let Some((name, _)) = ports.iter().find(|(_, claim)| claim.port == port) else {
                continue;
            };
            // Ask about the daemon holding the port, not the project. A
            // project-wide probe would report a stopped Postgres as running
            // merely because its Redis, or an open shell session, is alive.
            //
            // Probing costs a pitchfork call per matching root, so it runs only
            // here. An unreachable supervisor leaves the port available rather
            // than blocking a start that used to work.
            let id = other
                .ids
                .iter()
                .find(|id| id.rsplit('/').next() == Some(other_name.as_str()))
                .cloned()
                .unwrap_or_else(|| format!("{}/{other_name}", other.namespace));
            let serving = self
                .status(&other.root, &id)
                .await
                .ok()
                .and_then(|value| value["status"].as_str().map(String::from))
                .is_some_and(|status| {
                    matches!(status.as_str(), "running" | "waiting" | "stopping")
                });
            if !serving {
                continue;
            }
            bail!(
                "daemon {name} would use port {port}, in use by {other_name} running in {}. \
                 Stop it, set an explicit port on one of them, or use \
                 port = {{ auto = true, base = <port> }} to move this project's range.",
                other.root.display()
            );
        }
        Ok(clear)
    }

    /// Register this project's daemons with pitchfork.
    ///
    /// `starting` names the daemons the caller is about to launch, and only
    /// their ports are conflict checked. Passing an empty slice checks nothing,
    /// which is correct when a caller launches nothing but silently drops the
    /// diagnostic if a future caller forgets to fill it in.
    ///
    /// `owns_profile` is false when another project is preparing this root
    /// because it imported a daemon from it. The profile recorded is always the
    /// one the configuration was read under, since that is what the rendered
    /// definitions reflect; the flag only says who is asking, so a profile
    /// conflict can be explained in terms of the two projects involved.
    pub(crate) async fn prepare(
        &self,
        root: &Path,
        set: &DaemonSet,
        force_registration: bool,
        owns_profile: bool,
        starting: &[String],
    ) -> Result<(State, super::ProjectLock)> {
        let lock = super::ProjectLock::acquire(root)?;
        let previous = read_state(root)?;
        // This root's configuration was read under the current profile, whoever
        // asked for it, so that is what the rendered definitions reflect and
        // what has to be recorded. Claiming the profile the root last used would
        // leave the generated file and the state describing different things.
        let profile = crate::env::MISE_ENV.clone();
        if !previous.namespace.is_empty()
            && previous.profile != profile
            && self.active(root, &previous).await?
        {
            // The guard protects the owner either way. Rewriting a running
            // project's registration from another profile's definitions is worse
            // than refusing, so an importer is refused too, and told why.
            if owns_profile {
                bail!(
                    "another daemon profile is active for {}; stop its daemons and leave its shell sessions before switching MISE_ENV",
                    root.display()
                );
            }
            bail!(
                "{} has daemons running under MISE_ENV {:?}, and this project would register it under {:?}; stop them, or match that profile, before starting a daemon imported from it",
                root.display(),
                previous.profile.join(","),
                profile.join(",")
            );
        }
        let desired = match set.namespace_for(root) {
            Some(namespace) => namespace.to_string(),
            None => namespace(root)?,
        };
        let changed = !previous.namespace.is_empty() && previous.namespace != desired;
        if changed && self.active(root, &previous).await? {
            bail!(
                "daemons for {} are registered under namespace {:?} but the configuration now asks for {:?}; stop them before changing the namespace",
                root.display(),
                previous.namespace,
                desired
            );
        }
        // A daemon dropped from the configuration keeps its id so it can still
        // be queried and stopped, which means it may still be running and
        // holding its port. Its claim is kept for the same reason: discarding it
        // would hide that port from every other project's scan, and the next
        // project to resolve it would be told the port is free. A claim is
        // dropped only when the daemon is still declared and no longer has a
        // port mise resolves, since nothing manages one for it any more. Stale
        // entries cost nothing because a conflict is only reported once the
        // daemon holding the port answers as running.
        let ports = carry_port_claims(&previous.ports, set);
        let mut state = State {
            // Canonical, like the directory this state lives in: recording an
            // alias against a directory named for its target leaves a record
            // that cannot be tied back to the path that created it, which is
            // what `mise daemons prune` needs before it deletes anything.
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            profile,
            namespace: desired,
            // IDs from the previous namespace name daemons that are no longer
            // reachable; nothing is running under them, so drop them here rather
            // than forwarding unresolvable IDs to pitchfork.
            ids: if changed { Vec::new() } else { previous.ids },
            bin: self.bin.clone(),
            config_hash: String::new(),
            ports,
            ports_scan: String::new(),
        };
        for daemon in set.daemons.values() {
            let id = format!("{}/{}", state.namespace, daemon.name);
            if !state.ids.contains(&id) {
                state.ids.push(id);
            }
        }
        let content = render(set, &state)?;
        let file = state_dir(root).join("pitchfork.toml");
        state.config_hash = crate::hash::hash_to_str(&content);
        // Before the fast path, because an unchanged configuration is exactly
        // when another project can take this one's port: `auto` lifecycle then
        // hands pitchfork a session command and the daemon fails to bind in the
        // background, where an opaque error is easiest to miss. Explicit starts
        // force registration and would be covered either way.
        //
        // A shell hook reaches this on every prompt, so the neighbourhood is
        // fingerprinted from metadata first: unchanged siblings and unchanged
        // claims of our own can only give the answer they gave last time. The
        // liveness probe beyond that runs only once a port actually matches.
        let scan = siblings_fingerprint(&state_dir(root));
        // A recorded fingerprint means the last scan found no neighbour naming
        // any of this project's ports. That is the only answer safe to reuse:
        // it does not depend on whether anybody's daemon is running, nor on
        // which daemons this command starts, so neither a neighbour starting
        // one nor a change of selection can invalidate it. Anything else is
        // re-checked, because liveness is not visible in a state file.
        let reusable = !force_registration
            && !previous.ports_scan.is_empty()
            && scan == previous.ports_scan
            && state.ports == previous.ports;
        state.ports_scan = if reusable
            || self
                .check_port_conflicts(root, &state.ports, starting)
                .await?
        {
            scan
        } else {
            String::new()
        };
        if !force_registration
            && !changed
            && state.config_hash == previous.config_hash
            && std::fs::read(&file).ok().as_deref() == Some(content.as_bytes())
        {
            // Profile and executable ownership may change without affecting the
            // rendered daemon configuration (for example with mise = false).
            write_if_changed(
                &state_dir(root).join("state.json"),
                &serde_json::to_vec_pretty(&state)?,
            )?;
            return Ok((state, lock));
        }
        self.supports_external_config(root).await?;
        // Pitchfork binds a registered file to its namespace. Detach the old
        // mapping before registering the same file under a different name.
        // The active check above ensures this cannot orphan running daemons.
        if changed || set.daemons.is_empty() {
            self.output(
                root,
                &[
                    "config".into(),
                    "remove".into(),
                    file.to_string_lossy().into_owned(),
                ],
            )
            .await?;
        }
        write_if_changed(&file, content.as_bytes())?;
        if !set.daemons.is_empty() {
            self.output(
                root,
                &[
                    "config".into(),
                    "add".into(),
                    file.to_string_lossy().into_owned(),
                    "--dir".into(),
                    root.to_string_lossy().into_owned(),
                    "--namespace".into(),
                    state.namespace.clone(),
                ],
            )
            .await?;
        }
        write_if_changed(
            &state_dir(root).join("state.json"),
            &serde_json::to_vec_pretty(&state)?,
        )?;
        Ok((state, lock))
    }

    pub(crate) async fn exec(&self, root: &Path, args: Vec<String>) -> Result<()> {
        let mut runner = CmdLineRunner::new(&self.bin)
            .args(args)
            .envs(&self.env)
            .env_remove("PITCHFORK_CONFIG")
            .current_dir(root)
            .raw(true);
        runner.with_pass_signals();
        runner.execute_async().await
    }
}

/// Resolve the pitchfork namespace for a project root.
///
/// An explicit `[daemons_settings] namespace` wins so daemons in other projects
/// can name this one's by qualified ID. Everything else keeps the hashed default,
/// which cannot collide between unrelated checkouts.
pub(crate) fn resolve_namespace(root: &Path, settings: Option<&DaemonSettings>) -> Result<String> {
    let Some(explicit) = settings.and_then(|s| s.namespace.as_deref()) else {
        return namespace(root);
    };
    crate::daemons::validate_namespace(explicit)?;
    // The same check `port = "auto"` uses. A local one accepted any `.git` file
    // whose path contained `worktrees`, so a lookalike could take a per-worktree
    // namespace without the matching port offset.
    if settings.is_none_or(|s| s.namespace_per_worktree()) && crate::git::in_linked_worktree(root) {
        // Linked worktrees of one repository share the configuration that names
        // the namespace, so an unsuffixed namespace would make two checkouts
        // fight over the same pitchfork daemon IDs and state directory.
        return Ok(format!(
            "{explicit}-{}",
            crate::hash::hash_to_str(&root.canonicalize().unwrap_or_else(|_| root.to_path_buf()))
        ));
    }
    Ok(explicit.into())
}

pub(crate) fn namespace(root: &Path) -> Result<String> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut has_native = false;
    for name in [
        "pitchfork.local.toml",
        "pitchfork.toml",
        ".config/pitchfork.local.toml",
        ".config/pitchfork.toml",
    ] {
        let path = root.join(name);
        if path.exists() {
            let doc: toml::Value = toml::from_str(&std::fs::read_to_string(path)?)?;
            if let Some(namespace) = doc.get("namespace").and_then(toml::Value::as_str) {
                return Ok(namespace.into());
            }
            has_native = true;
        }
    }
    if has_native {
        return Ok(root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned());
    }
    let base: String = root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let base = base
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() { "mise" } else { &base };
    Ok(format!("{base}-{}", crate::hash::hash_to_str(&root)))
}

fn render(set: &DaemonSet, state: &State) -> Result<String> {
    let mut daemons = toml::Table::new();
    let mut header =
        String::from("# Generated by mise; edit [daemons] in the source configuration.\n");
    // Hostname routing is what an older supervisor silently lacks: it starts
    // the daemons either way, so the only symptom is a URL that never
    // resolves. Say which release understands these keys.
    // Only when a daemon actually has one. Labels are derived for every root
    // whether or not anything is routed, so testing them would put the notice
    // on a project where every daemon opted out or configured no port, claiming
    // a feature the file does not use.
    if set.daemons.values().any(|d| d.host.is_some()) {
        header.push_str(&format!(
            "# Hostname routing (per-daemon proxy labels) needs pitchfork {}.\n",
            crate::daemons::urls::REQUIRED_PITCHFORK
        ));
    }
    for daemon in set.daemons.values() {
        header.push_str(&format!(
            "# {} from {}\n",
            daemon.name,
            daemon.source.to_string_lossy().replace(['\r', '\n'], " ")
        ));
        let mut table = daemon.table.clone();
        // Pitchfork wraps the main process in mise, but runs readiness probes
        // directly. Resolve custom probes in this checkout too: the supervisor
        // may have inherited another worktree's tools and endpoint variables.
        // Preset probes already carry their own environment wrapper.
        if cfg!(unix)
            && daemon.preset.is_none()
            && (daemon.task.is_some()
                || table.get("mise").and_then(toml::Value::as_bool) != Some(false))
        {
            super::presets::wrap_probe_commands(&mut table);
        }
        // Ensure mise sees the profile that generated this definition, even at
        // boot. A task-backed daemon runs mise itself rather than being wrapped
        // in `mise x`, so it needs the profile even though it sets mise = false;
        // without it a supervisor restart would resolve the task against the
        // default configuration.
        if !state.profile.is_empty()
            && (daemon.task.is_some()
                || table.get("mise").and_then(toml::Value::as_bool) != Some(false))
        {
            let env = table
                .entry("env".to_string())
                .or_insert(toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or_else(|| eyre::eyre!("daemon env must be a table"))?;
            env.insert(
                "MISE_ENV".into(),
                toml::Value::String(state.profile.join(",")),
            );
        }
        daemons.insert(daemon.name.clone(), toml::Value::Table(table));
    }
    let mut groups = toml::Table::new();
    for group in &set.groups {
        // Qualified IDs keep the group bound to this project's namespace. A member
        // this project no longer owns, because a nearer config redefined that name,
        // has no ID here, and pitchfork rejects a group naming an undefined daemon.
        let members = group
            .daemons
            .iter()
            .filter(|name| set.daemons.contains_key(*name))
            .map(|name| toml::Value::String(format!("{}/{name}", state.namespace)))
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        groups.insert(
            group.name.clone(),
            toml::Value::Table(toml::Table::from_iter([(
                "daemons".into(),
                toml::Value::Array(members),
            )])),
        );
    }
    let mut doc = toml::Table::new();
    doc.insert(
        "settings".into(),
        toml::Value::Table(toml::Table::from_iter([(
            "general".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "mise_bin".into(),
                toml::Value::String(crate::env::MISE_BIN.to_string_lossy().into_owned()),
            )])),
        )])),
    );
    doc.insert("daemons".into(), toml::Value::Table(daemons));
    if !groups.is_empty() {
        doc.insert("groups".into(), toml::Value::Table(groups));
    }
    Ok(header + &toml::to_string_pretty(&doc)?)
}

pub(crate) async fn validate_tools(
    set: &DaemonSet,
    config: &Arc<Config>,
    ts: &Toolset,
) -> Result<()> {
    for daemon in set.daemons.values() {
        // Imported daemons resolve their tool against the project that declares
        // them, which happens when that root is prepared.
        let Some((tool, version)) = daemon.tool.as_ref().filter(|_| !daemon.imported) else {
            continue;
        };
        if cfg!(windows) {
            bail!("daemon presets are not supported on Windows yet");
        }
        let ba: crate::cli::args::BackendArg = tool.as_str().into();
        let Some(versions) = ts.versions.get(&ba) else {
            bail!("daemon {} requires {tool}@{version}", daemon.name);
        };
        let actual = versions
            .versions
            .first()
            .ok_or_else(|| eyre::eyre!("missing {tool}"))?;
        let compatible = ba
            .backend()?
            .list_installed_versions_matching(version)
            .contains(&actual.version);
        if compatible {
            continue;
        }
        let requested = ToolRequest::new(
            ba.into(),
            version,
            ToolSource::MiseTomlDaemon(daemon.source.clone()),
        )?
        .resolve(
            config,
            &crate::toolset::ResolveOptions {
                offline: true,
                ..Default::default()
            },
        )
        .await?;
        if actual.version != requested.version {
            bail!(
                "daemon {} requires {tool}@{version} ({}), but [tools] selects {}",
                daemon.name,
                requested.version,
                actual.version
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn symlinked_roots_share_namespace_and_state() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let link = tmp.path().join("alias");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert_eq!(namespace(&root).unwrap(), namespace(&link).unwrap());
        assert_eq!(state_dir(&root), state_dir(&link));
    }

    fn settings(namespace: &str, per_worktree: bool) -> DaemonSettings {
        DaemonSettings {
            namespace: Some(namespace.to_string()),
            namespace_per_worktree: Some(per_worktree),
        }
    }

    #[test]
    fn explicit_namespace_wins_over_the_hashed_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("app");
        std::fs::create_dir(&root).unwrap();
        assert_eq!(
            resolve_namespace(&root, Some(&settings("entiredb", true))).unwrap(),
            "entiredb"
        );
        assert_eq!(
            resolve_namespace(&root, None).unwrap(),
            namespace(&root).unwrap()
        );
        assert!(
            resolve_namespace(&root, Some(&settings("entire--db", true)))
                .unwrap_err()
                .to_string()
                .contains("invalid daemon namespace")
        );
    }

    #[test]
    fn linked_worktrees_get_their_own_namespace_unless_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        let private = main.join(".git").join("worktrees").join("feature");
        std::fs::create_dir_all(&private).unwrap();
        // A real linked worktree's private directory points back at the shared
        // git dir; without that pointer this is a lookalike, and namespacing
        // now agrees with port allocation in rejecting one.
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        std::fs::write(main.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let worktree = tmp.path().join("feature");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();

        let suffixed = resolve_namespace(&worktree, Some(&settings("entiredb", true))).unwrap();
        assert!(
            suffixed.starts_with("entiredb-") && suffixed != "entiredb",
            "{suffixed}"
        );
        // A nested directory inside the worktree resolves the same way.
        let nested = worktree.join("services");
        std::fs::create_dir(&nested).unwrap();
        assert!(
            resolve_namespace(&nested, Some(&settings("entiredb", true)))
                .unwrap()
                .starts_with("entiredb-")
        );
        assert_eq!(
            resolve_namespace(&worktree, Some(&settings("entiredb", false))).unwrap(),
            "entiredb"
        );
        // The main checkout keeps the unsuffixed name.
        assert_eq!(
            resolve_namespace(&main, Some(&settings("entiredb", true))).unwrap(),
            "entiredb"
        );
        // A submodule uses a .git file too, but it is not a worktree.
        let submodule = tmp.path().join("sub");
        std::fs::create_dir(&submodule).unwrap();
        std::fs::write(submodule.join(".git"), "gitdir: ../repo/.git/modules/sub\n").unwrap();
        assert_eq!(
            resolve_namespace(&submodule, Some(&settings("entiredb", true))).unwrap(),
            "entiredb"
        );
        // A `.git` file that only looks like a worktree's is not one, and the
        // same check decides this for namespaces and for `port = "auto"`.
        let forged = tmp.path().join("forged");
        std::fs::create_dir(&forged).unwrap();
        std::fs::write(
            forged.join(".git"),
            format!("gitdir: {}\n", main.join(".git/worktrees/pruned").display()),
        )
        .unwrap();
        assert_eq!(
            resolve_namespace(&forged, Some(&settings("entiredb", true))).unwrap(),
            "entiredb"
        );
    }

    #[test]
    fn only_live_roots_with_a_matching_port_become_conflict_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let mine = tmp.path().join("mine");
        let theirs = tmp.path().join("theirs");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&theirs).unwrap();
        let write_state = |root: &Path, port: u16| {
            let state = State {
                root: root.to_path_buf(),
                ports: BTreeMap::from([("db".to_string(), PortClaim::fixed(port))]),
                ..State::default()
            };
            let dir = state_dir(root);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("state.json"),
                serde_json::to_vec_pretty(&state).unwrap(),
            )
            .unwrap();
        };
        write_state(&theirs, 5432);
        write_state(&mine, 5432);

        // Our own state directory never reports against us.
        let found = claimed_ports(&state_dir(&mine));
        let ours: Vec<_> = found.iter().filter(|(s, _, _)| s.root == mine).collect();
        assert!(ours.is_empty(), "own claim must be skipped");
        let theirs_found: Vec<_> = found
            .iter()
            .filter(|(s, name, port)| s.root == theirs && name == "db" && *port == 5432)
            .collect();
        assert_eq!(theirs_found.len(), 1, "live sibling claim must be reported");

        // A checkout that no longer exists holds no claim, so its port is
        // reusable rather than reserved forever.
        std::fs::remove_dir_all(&theirs).unwrap();
        assert!(
            !claimed_ports(&state_dir(&mine))
                .iter()
                .any(|(s, _, _)| s.root == theirs)
        );
    }

    #[test]
    fn a_scan_is_only_clear_when_no_neighbour_names_any_of_our_ports() {
        // Clearance is what makes the skip sound, so it must be judged over
        // every claim this project holds, not just the ones starting: a port
        // nobody else names cannot conflict however daemons come and go.
        let tmp = tempfile::tempdir().unwrap();
        let mine = tmp.path().join("mine");
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let state = State {
            root: other.clone(),
            ports: BTreeMap::from([("db".to_string(), PortClaim::fixed(5432))]),
            ..State::default()
        };
        let dir = state_dir(&other);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();

        let claimed: Vec<_> = claimed_ports(&state_dir(&mine))
            .into_iter()
            .filter(|(o, _, _)| o.root != mine)
            .collect();
        let clear = |ports: &BTreeMap<String, PortClaim>| {
            !ports
                .values()
                .any(|claim| claimed.iter().any(|(_, _, port)| *port == claim.port))
        };

        // Ports nobody else names: safe to remember as clear.
        assert!(clear(&BTreeMap::from([(
            "redis".to_string(),
            PortClaim::fixed(6379)
        )])));

        // A neighbour names 5432, so this is not clear even though that daemon
        // may be stopped right now. Liveness decides the verdict, and liveness
        // is not visible here, so the answer must not be reused.
        assert!(!clear(&BTreeMap::from([(
            "pg".to_string(),
            PortClaim::fixed(5432)
        )])));

        // Judged over every claim, so an unrelated overlap still blocks reuse.
        assert!(!clear(&BTreeMap::from([
            ("redis".to_string(), PortClaim::fixed(6379)),
            ("pg".to_string(), PortClaim::fixed(5432)),
        ])));
    }

    #[test]
    fn the_fingerprint_notices_a_neighbour_changing() {
        // The hook skips the scan when this is unchanged, so it has to move for
        // anything that could change the answer.
        let tmp = tempfile::tempdir().unwrap();
        let mine = tmp.path().join("mine");
        let other = tmp.path().join("other");
        let write = |root: &Path, port: u16| {
            let state = State {
                root: root.to_path_buf(),
                ports: BTreeMap::from([("db".to_string(), PortClaim::fixed(port))]),
                ..State::default()
            };
            let dir = state_dir(root);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("state.json"),
                serde_json::to_vec_pretty(&state).unwrap(),
            )
            .unwrap();
        };
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        let before = siblings_fingerprint(&state_dir(&mine));
        assert_eq!(before, siblings_fingerprint(&state_dir(&mine)), "stable");

        // A project appearing must be noticed.
        write(&other, 5432);
        let appeared = siblings_fingerprint(&state_dir(&mine));
        assert_ne!(before, appeared);

        // So must that project rewriting its claims.
        write(&other, 15432);
        assert_ne!(appeared, siblings_fingerprint(&state_dir(&mine)));

        // Our own state file is not a neighbour, so it never moves the value.
        let neighbours = siblings_fingerprint(&state_dir(&mine));
        write(&mine, 6379);
        assert_eq!(neighbours, siblings_fingerprint(&state_dir(&mine)));
    }

    #[test]
    fn a_removed_daemon_keeps_its_claim_while_it_may_still_run() {
        let with_port = |name: &str, port: u16| super::super::Daemon {
            name: name.to_string(),
            source: PathBuf::from("/project/mise.toml"),
            root: PathBuf::from("/project"),
            table: toml::Table::new(),
            preset: None,
            task: None,
            tool: None,
            exports: Default::default(),
            port: Some(PortClaim::fixed(port)),
            imported: false,
            host: None,
        };
        let set = |daemons: Vec<super::super::Daemon>| DaemonSet {
            daemons: daemons.into_iter().map(|d| (d.name.clone(), d)).collect(),
            ..Default::default()
        };
        let previous = BTreeMap::from([
            ("postgres".to_string(), PortClaim::fixed(5432)),
            ("redis".to_string(), PortClaim::fixed(6379)),
        ]);

        // Redis is dropped from the config but keeps its id, so it may still be
        // running. Losing its claim would tell the next project 6379 is free.
        let kept = carry_port_claims(&previous, &set(vec![with_port("postgres", 5432)]));
        assert_eq!(kept["redis"].port, 6379, "a removed daemon keeps its claim");
        assert_eq!(kept["postgres"].port, 5432);

        // A redeclared daemon takes its current port, not the recorded one.
        let moved = carry_port_claims(&previous, &set(vec![with_port("redis", 6400)]));
        assert_eq!(moved["redis"].port, 6400);

        // Still declared but no longer holding a mise-resolved port: nothing
        // manages one for it, so the stale claim goes.
        let mut bare = with_port("redis", 0);
        bare.port = None;
        let dropped = carry_port_claims(&previous, &set(vec![bare]));
        assert!(!dropped.contains_key("redis"));
        assert_eq!(dropped["postgres"].port, 5432);
    }

    #[test]
    fn only_the_daemons_being_started_have_their_ports_checked() {
        // Registration covers a whole project, so an unrelated daemon whose
        // port is taken elsewhere must not block the one being started.
        let ports = BTreeMap::from([
            ("postgres".to_string(), PortClaim::fixed(5432)),
            ("redis".to_string(), PortClaim::fixed(6379)),
        ]);
        let names = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();

        // Starting redis never considers the Postgres claim, so another
        // project serving 5432 cannot block it.
        let scoped = ports_being_started(&ports, &names(&["redis"]));
        assert_eq!(scoped.keys().collect::<Vec<_>>(), ["redis"]);
        assert_eq!(scoped["redis"].port, 6379);

        // A bare start covers everything it will launch.
        assert_eq!(
            ports_being_started(&ports, &names(&["postgres", "redis"])).len(),
            2
        );
        // A name with no claim, and no names at all, leave nothing to check.
        assert!(ports_being_started(&ports, &names(&["web"])).is_empty());
        assert!(ports_being_started(&ports, &[]).is_empty());
    }

    /// `state_dir` canonicalizes before hashing, so both spellings of a
    /// symlinked project land in one state directory and the scan excludes it
    /// by directory. The stored `root` is whichever spelling wrote it last,
    /// which is why the exclusion cannot rest on comparing that.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_does_not_conflict_with_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("project");
        std::fs::create_dir_all(&real).unwrap();
        let link = tmp.path().join("linked");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // Written under one spelling...
        let dir = state_dir(&real);
        std::fs::create_dir_all(&dir).unwrap();
        let state = State {
            root: real.clone(),
            ports: BTreeMap::from([("db".to_string(), PortClaim::fixed(5432))]),
            ..State::default()
        };
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();

        // ...and read under the other, which resolves to the same directory.
        assert_eq!(state_dir(&link), dir, "both spellings share a state dir");
        assert!(
            !claimed_ports(&state_dir(&link))
                .iter()
                .any(|(other, _, _)| other.root == real),
            "a project must not find its own claim through a symlink"
        );
    }

    #[test]
    fn a_stopped_daemon_does_not_reserve_its_port() {
        // The scan deliberately reports a candidate without consulting
        // liveness; `Runtime::check_port_conflicts` probes before failing, so a
        // stopped project cannot hold a default port hostage. Two projects
        // taking turns on 5432 has to keep working.
        let tmp = tempfile::tempdir().unwrap();
        let theirs = tmp.path().join("stopped");
        std::fs::create_dir_all(&theirs).unwrap();
        let state = State {
            root: theirs.clone(),
            ports: BTreeMap::from([("db".to_string(), PortClaim::fixed(5432))]),
            ..State::default()
        };
        let dir = state_dir(&theirs);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        let found = claimed_ports(&state_dir(tmp.path()));
        assert!(
            found.iter().any(|(s, _, p)| s.root == theirs && *p == 5432),
            "a persisted claim is only a candidate, not a verdict"
        );
    }

    #[test]
    fn the_generated_config_carries_hostname_routing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("shop");
        std::fs::create_dir_all(&root).unwrap();
        let daemon = |name: &str, proxy: toml::Value, host: Option<&str>| super::super::Daemon {
            name: name.to_string(),
            source: root.join("mise.toml"),
            root: root.clone(),
            table: toml::Table::from_iter([
                ("run".into(), toml::Value::String(format!("run {name}"))),
                ("proxy".into(), proxy),
                (
                    "proxy_tls".into(),
                    toml::Value::String("passthrough".into()),
                ),
            ]),
            preset: None,
            task: None,
            tool: None,
            exports: Default::default(),
            imported: false,
            port: None,
            host: host.map(str::to_string),
        };
        let set = DaemonSet {
            daemons: indexmap::IndexMap::from_iter([
                (
                    "api".to_string(),
                    daemon(
                        "api",
                        toml::Value::String("front".into()),
                        Some("front.shop.localhost"),
                    ),
                ),
                (
                    "cache".to_string(),
                    daemon("cache", toml::Value::Boolean(false), None),
                ),
            ]),
            ..Default::default()
        };
        let state = State {
            namespace: "shop".into(),
            root: root.clone(),
            ..State::default()
        };
        let rendered = render(&set, &state).unwrap();
        // Both keys reach pitchfork verbatim, so it routes each daemon and
        // handles TLS the way the declaration asked. The hostname itself is
        // pitchfork's to derive; mise only supplies the label.
        let parsed: toml::Table = toml::from_str(&rendered).unwrap();
        assert_eq!(parsed["daemons"]["api"]["proxy"].as_str(), Some("front"));
        assert_eq!(
            parsed["daemons"]["api"]["proxy_tls"].as_str(),
            Some("passthrough")
        );
        assert_eq!(parsed["daemons"]["cache"]["proxy"].as_bool(), Some(false));
        assert!(rendered.contains("needs pitchfork"), "{rendered}");

        // The notice claims hostname routing is in use, so a project where
        // nothing is routed must not carry it. Labels are derived for every
        // root either way, which is what made this easy to get wrong.
        let mut unrouted = set.clone();
        for daemon in unrouted.daemons.values_mut() {
            daemon.host = None;
        }
        unrouted.labels.insert(
            root.clone(),
            super::super::urls::RootLabels {
                project: Some("shop".into()),
                worktree: None,
            },
        );
        let rendered = render(&unrouted, &state).unwrap();
        assert!(!rendered.contains("needs pitchfork"), "{rendered}");
    }

    #[test]
    fn groups_render_with_qualified_daemon_ids() {
        let daemon = |name: &str| super::super::Daemon {
            name: name.to_string(),
            source: PathBuf::from("/project/mise.toml"),
            root: PathBuf::from("/project"),
            table: toml::Table::from_iter([(
                "run".into(),
                toml::Value::String(format!("run {name}")),
            )]),
            preset: None,
            task: None,
            tool: None,
            exports: Default::default(),
            imported: false,
            port: None,
            host: None,
        };
        let set = DaemonSet {
            daemons: ["api", "worker"]
                .into_iter()
                .map(|name| (name.to_string(), daemon(name)))
                .collect(),
            groups: vec![super::super::Group {
                name: "web".into(),
                source: PathBuf::from("/project/mise.toml"),
                root: PathBuf::from("/project"),
                members: vec!["api".into(), "worker".into()],
                daemons: vec!["api".into(), "worker".into()],
            }],
            ..Default::default()
        };
        let state = State {
            namespace: "proj".into(),
            ..State::default()
        };
        let rendered = render(&set, &state).unwrap();
        assert!(rendered.contains("[groups.web]"), "{rendered}");
        assert!(rendered.contains("\"proj/api\""), "{rendered}");
        assert!(rendered.contains("\"proj/worker\""), "{rendered}");

        // A member this project no longer owns is left out rather than rendered as
        // an ID pitchfork cannot resolve.
        let mut overridden = set.clone();
        overridden.groups[0].daemons.push("elsewhere".into());
        let rendered = render(&overridden, &state).unwrap();
        assert!(rendered.contains("[groups.web]"), "{rendered}");
        assert!(!rendered.contains("elsewhere"), "{rendered}");
        // A group left with no members of its own is dropped entirely.
        let mut empty = set.clone();
        empty.groups[0].daemons = vec!["elsewhere".into()];
        assert!(!render(&empty, &state).unwrap().contains("[groups"));
        // Without groups the section is omitted entirely.
        let bare = DaemonSet {
            groups: Vec::new(),
            ..set
        };
        assert!(!render(&bare, &state).unwrap().contains("[groups"));
    }

    #[test]
    fn task_daemons_carry_the_profile_despite_opting_out_of_mise() {
        let daemon = |task: Option<&str>, mise: bool| super::super::Daemon {
            name: "core".into(),
            source: PathBuf::from("/project/mise.toml"),
            root: PathBuf::from("/project"),
            table: toml::Table::from_iter([
                ("run".into(), toml::Value::String("server".into())),
                ("mise".into(), toml::Value::Boolean(mise)),
            ]),
            preset: None,
            task: task.map(str::to_string),
            tool: None,
            exports: Default::default(),
            imported: false,
            port: None,
            host: None,
        };
        let state = State {
            profile: vec!["dev".into()],
            ..State::default()
        };
        let rendered = |d: super::super::Daemon| {
            render(
                &DaemonSet {
                    daemons: indexmap::IndexMap::from_iter([("core".to_string(), d)]),
                    ..Default::default()
                },
                &state,
            )
            .unwrap()
        };
        // A task daemon runs mise itself, so it needs the profile that resolved
        // the task even though pitchfork is told not to wrap it.
        assert!(rendered(daemon(Some("dev"), false)).contains("MISE_ENV = \"dev\""));
        // A daemon that is not mise at all still opts out.
        assert!(!rendered(daemon(None, false)).contains("MISE_ENV"));
        assert!(rendered(daemon(None, true)).contains("MISE_ENV = \"dev\""));
    }

    #[test]
    fn writes_are_atomic_and_unchanged_content_keeps_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        assert!(write_if_changed(&path, b"one").unwrap());
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!write_if_changed(&path, b"one").unwrap());
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        assert!(write_if_changed(&path, b"two").unwrap());
    }
}
