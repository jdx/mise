use super::ports::PortClaim;
use super::{DaemonSet, state_dir};
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct State {
    pub root: PathBuf,
    pub profile: Vec<String>,
    pub namespace: String,
    pub ids: Vec<String>,
    pub bin: PathBuf,
    #[serde(default)]
    pub config_hash: String,
    /// Ports allocated per daemon name. Persisting them keeps an `auto`
    /// allocation stable across a change to the slot derivation, and lets other
    /// project roots on this machine detect a conflict before starting.
    #[serde(default)]
    pub ports: BTreeMap<String, PortClaim>,
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
        let mut command = Command::new(&self.bin);
        command
            .args(args)
            .envs(&self.env)
            .env_remove("PITCHFORK_CONFIG")
            .current_dir(root)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output()).await??;
        if !output.status.success() {
            bail!(
                "pitchfork {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8(output.stdout)?)
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

    async fn active(&self, root: &Path, state: &State) -> Result<bool> {
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
    ) -> Result<()> {
        let ports = &ports_being_started(ports, starting);
        if ports.is_empty() {
            return Ok(());
        }
        for (other, other_name, port) in claimed_ports(&state_dir(root)) {
            if other.root == root {
                continue;
            }
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
        Ok(())
    }

    /// Register this project's daemons with pitchfork.
    ///
    /// `starting` names the daemons the caller is about to launch, and only
    /// their ports are conflict checked. Passing an empty slice checks nothing,
    /// which is correct when a caller launches nothing but silently drops the
    /// diagnostic if a future caller forgets to fill it in.
    pub(crate) async fn prepare(
        &self,
        root: &Path,
        set: &DaemonSet,
        force_registration: bool,
        starting: &[String],
    ) -> Result<(State, fslock::LockFile)> {
        let lock = crate::lock_file::LockFile::at(&state_dir(root).join("project.lock")).lock()?;
        let previous = read_state(root)?;
        let profile = crate::env::MISE_ENV.clone();
        if !previous.namespace.is_empty()
            && previous.profile != profile
            && self.active(root, &previous).await?
        {
            bail!(
                "another daemon profile is active for {}; stop its daemons and leave its shell sessions before switching MISE_ENV",
                root.display()
            );
        }
        let namespace = if !previous.namespace.is_empty() {
            previous.namespace.clone()
        } else {
            namespace(root)?
        };
        let mut state = State {
            root: root.into(),
            profile,
            namespace,
            ids: previous.ids,
            bin: self.bin.clone(),
            config_hash: String::new(),
            ports: set
                .daemons
                .values()
                .filter_map(|d| d.port.map(|claim| (d.name.clone(), claim)))
                .collect(),
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
        // The cost is a directory read and a few small state files per hook for
        // a project that declares ports at all; the liveness probe that follows
        // only runs once a port actually matches.
        self.check_port_conflicts(root, &state.ports, starting)
            .await?;
        if !force_registration
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
        write_if_changed(&file, content.as_bytes())?;
        if set.daemons.is_empty() {
            self.output(
                root,
                &[
                    "config".into(),
                    "remove".into(),
                    file.to_string_lossy().into_owned(),
                ],
            )
            .await?;
        } else {
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
    for daemon in set.daemons.values() {
        header.push_str(&format!(
            "# {} from {}\n",
            daemon.name,
            daemon.source.to_string_lossy().replace(['\r', '\n'], " ")
        ));
        let mut table = daemon.table.clone();
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
        let Some((tool, version)) = &daemon.tool else {
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
            port: None,
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
            port: None,
        };
        let state = State {
            profile: vec!["dev".into()],
            ..State::default()
        };
        let rendered = |d: super::super::Daemon| {
            render(
                &DaemonSet {
                    daemons: indexmap::IndexMap::from_iter([("core".to_string(), d)]),
                    groups: Vec::new(),
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
