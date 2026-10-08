use super::ports::PortClaim;
use super::{DaemonSet, DaemonSettings, state_dir};
use crate::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::env_diff::EnvMap;
use crate::toolset::{ToolRequest, ToolSource, Toolset, ToolsetBuilder};
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

/// How to give one checkout a port of its own. A `[daemons.<name>]` declared in a
/// higher-precedence file replaces the whole daemon, so the other keys are
/// repeated there.
const PIN_A_PORT: &str = "declare the daemon again in a gitignored mise.local.toml with a fixed `port = <n>`, or with `port = { auto = true, base = <n> }` to move its range. That declaration replaces the whole daemon, so repeat its other keys.";

/// Names a nested mise withholds from pitchfork: the secrets it inherited and
/// the marker that names them.
fn inherited_secret_env_names() -> Vec<&'static str> {
    if mise_util::env::INHERITED_SECRET_KEYS.is_empty() {
        return vec![];
    }
    mise_util::env::INHERITED_SECRET_KEYS
        .iter()
        .map(String::as_str)
        .chain([mise_util::env::SECRET_KEYS_MARKER])
        .collect()
}

/// Keep a nested mise from starting a pitchfork supervisor, and so every later
/// daemon and probe, with the parent's secrets.
fn strip_inherited_secrets(command: &mut Command, keys: &BTreeSet<String>) {
    if keys.is_empty() {
        return;
    }
    for k in keys
        .iter()
        .map(String::as_str)
        .chain([mise_util::env::SECRET_KEYS_MARKER])
    {
        command.env_remove(k);
    }
}

/// Whether something already listens on this loopback port, on either family.
/// Binding is what a daemon would do next, so a failure to bind is the same
/// answer it would get. A daemon may bind `::1` instead of `127.0.0.1`, and an
/// IPv6-only listener does not conflict on the other family, so both are tried.
/// A host without IPv6 loopback cannot have anything listening there.
fn port_is_taken(port: u16) -> bool {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err()
        || std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port))
            .is_err_and(|err| err.kind() == std::io::ErrorKind::AddrInUse)
}

/// Where an automatic port came from, for a message about it being in use.
fn port_origin(claim: &PortClaim) -> &'static str {
    if claim.port == claim.base {
        // A primary checkout and a project outside Git get no path-based offset.
        "the configured base"
    } else {
        // Only a linked worktree is offset by a slot derived from its path.
        "the base offset by this worktree's path"
    }
}

/// What a pitchfork's external configuration registration can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExternalConfig {
    /// `pitchfork config add --label` exists, so a registration can say what
    /// hostname label the project is served under.
    pub label: bool,
}

/// Whether the `config add` command in pitchfork's `usage` output declares a
/// `--label` flag. Detected rather than versioned so a build that gains the flag
/// is used as soon as it is installed.
fn config_add_takes_label(usage: &str) -> bool {
    fn indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }
    fn command_name(line: &str) -> Option<&str> {
        let rest = line.trim_start().strip_prefix("cmd ")?;
        Some(rest.split_whitespace().next()?.trim_matches('"'))
    }
    // `long_help #"""` text is prose, often unindented; it says nothing about
    // structure and would end an indentation scan early.
    let mut in_prose = false;
    let structure: Vec<&str> = usage
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if in_prose {
                in_prose = trimmed != "\"\"\"#";
                return false;
            }
            in_prose = trimmed.ends_with("#\"\"\"");
            true
        })
        .collect();
    // The top-level `config` command, then its nested `add`. A `--label` on
    // another command, or on another `add` such as `pitchfork daemons add`,
    // does not count.
    let subtree = |start: usize| {
        let base = indent(structure[start]);
        structure[start + 1..]
            .iter()
            .copied()
            .take_while(move |line| indent(line) > base)
    };
    let Some(config) = structure
        .iter()
        .position(|l| indent(l) == 0 && command_name(l) == Some("config"))
    else {
        return false;
    };
    let Some(add) = subtree(config)
        .position(|l| command_name(l) == Some("add"))
        .map(|offset| config + 1 + offset)
    else {
        return false;
    };
    subtree(add)
        .filter_map(|l| l.trim_start().strip_prefix("flag "))
        .any(|flag| {
            // `flag --label {` or `flag "-l --label" help=...`
            let spec = match flag.strip_prefix('"') {
                Some(quoted) => quoted.split('"').next().unwrap_or_default(),
                None => flag.split_whitespace().next().unwrap_or_default(),
            };
            spec.split_whitespace().any(|name| name == "--label")
        })
}

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
pub struct State {
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
    /// The project hostname label this registration wants pitchfork to serve.
    /// Empty when mise could not name the project.
    #[serde(default)]
    pub label: String,
    /// Whether that label was actually passed to `pitchfork config add`. Only a
    /// pitchfork that understands `--label` takes it; without it the registry
    /// keeps naming the project after the namespace, as it always did.
    #[serde(default)]
    pub label_registered: bool,
    /// Identity of the pitchfork executable that could not take the wanted
    /// label, so the registration is retried when that executable changes, even
    /// in place at the same path. Empty when the label was registered, was not
    /// wanted, or the executable could not be examined.
    #[serde(default)]
    pub label_unsupported_by: String,
    /// The registry still holds a label mise no longer wants, and removing it
    /// was postponed because the project's daemons are running. `config remove`
    /// would orphan them, so it waits for a later registration that finds them
    /// stopped; until then the unchanged fast path must not be taken.
    #[serde(default)]
    pub label_detach_pending: bool,
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

pub struct Runtime {
    pub bin: PathBuf,
    pub env: EnvMap,
}

pub fn read_state(root: &Path) -> Result<State> {
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

pub async fn config_for_root(config: &Arc<Config>, root: &Path) -> Result<Arc<Config>> {
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

/// The tools of `ts` that installing for `set` needs: pitchfork, each daemon's
/// tool, and everything those tools depend on to install.
///
/// Dependencies come from the same declarations the installer resolves, so a
/// runtime configured under its full backend id or named in a tool's `depends`
/// option is found the way the installer finds it.
fn install_scope(ts: &Toolset, set: &DaemonSet) -> Vec<Arc<crate::args::BackendArg>> {
    let mut roots = vec![crate::args::BackendArg::from("pitchfork")];
    for daemon in set.daemons.values().filter(|d| !d.imported) {
        if let Some((tool, _)) = &daemon.tool {
            roots.push(crate::args::BackendArg::from(tool.as_str()));
        }
    }
    let mut scope: Vec<Arc<crate::args::BackendArg>> = ts
        .versions
        .keys()
        .filter(|ba| {
            roots
                .iter()
                .any(|root| crate::install_context::backend_args_match(root, ba))
        })
        .cloned()
        .collect();
    let mut next = 0;
    while let Some(ba) = scope.get(next).cloned() {
        next += 1;
        let Some(list) = ts.versions.get(&ba) else {
            continue;
        };
        for request in &list.requests {
            let declarations = crate::install_context::install_dependency_declarations(request);
            for candidate in ts.versions.keys() {
                if declarations.matches(candidate)
                    && !scope.iter().any(|s| Arc::ptr_eq(s, candidate))
                {
                    scope.push(candidate.clone());
                }
            }
        }
    }
    scope
}

/// Build the daemon toolset, installing what `install` needs when it is given.
///
/// `install` is the set of daemons about to be validated and started. Only
/// pitchfork and the tools those daemons declare are installed: a preset's tool
/// comes from its daemon declaration, not the command line, so the default
/// install would leave it out and validation would report a version mismatch.
/// A tool that belongs to another daemon, or to no daemon, is left alone, so
/// one that cannot be installed does not keep an unrelated daemon from starting.
pub async fn toolset(
    config: &Arc<Config>,
    install: Option<&DaemonSet>,
) -> Result<(Arc<Config>, Toolset)> {
    let mut config = config.clone();
    let mut ts = toolset_resolved(&config, install.is_some()).await?;
    if let Some(set) = install {
        let wanted = install_scope(&ts, set);
        let mut skip = crate::config::Settings::get()
            .auto_install_disable_tools
            .clone()
            .unwrap_or_default();
        skip.extend(
            ts.versions
                .keys()
                .filter(|ba| !wanted.iter().any(|w| Arc::ptr_eq(w, ba)))
                .map(|ba| ba.short.clone()),
        );
        let (_, missing) = ts
            .install_missing_versions(
                &mut config,
                &crate::toolset::InstallOptions {
                    missing_args_only: false,
                    auto_install_disable_tools: Some(skip),
                    ..Default::default()
                },
            )
            .await?;
        ts.notify_missing_versions(missing);
    }
    Ok((config, ts))
}

impl Runtime {
    pub async fn from_toolset(
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
        strip_inherited_secrets(&mut command, &mise_util::env::INHERITED_SECRET_KEYS);
        Ok(tokio::time::timeout(Duration::from_secs(15), command.output()).await??)
    }

    /// Whether this pitchfork registers external configuration, and if so
    /// whether `config add` takes `--label`.
    pub(crate) async fn supports_external_config(&self, root: &Path) -> Result<ExternalConfig> {
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
        Ok(ExternalConfig {
            label: config_add_takes_label(&usage),
        })
    }

    /// Task daemons are started with an argv `run`, which older pitchfork rejects.
    /// Asked of the config schema, a read-only command every pitchfork with
    /// external configuration has, rather than of a version number.
    pub(crate) async fn supports_argv_run(&self, root: &Path, daemon: &str) -> Result<()> {
        let schema = self.output(root, &["schema".into()]).await?;
        if !serde_json::from_str(&schema).is_ok_and(|schema| schema_accepts_argv_run(&schema)) {
            bail!(
                "daemon {daemon} runs a task, which needs pitchfork 2.28.0 or later; upgrade pitchfork, for example with `mise use pitchfork@latest`"
            );
        }
        Ok(())
    }

    pub async fn status(&self, root: &Path, id: &str) -> Result<serde_json::Value> {
        let out = self
            .output(root, &["status".into(), id.into(), "--json".into()])
            .await?;
        Ok(serde_json::from_str(&out)?)
    }

    pub async fn supervisor_up(&self, root: &Path) -> Result<bool> {
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

    pub async fn active(&self, root: &Path, state: &State) -> Result<bool> {
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
                 Stop it, or {PIN_A_PORT}",
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
    pub async fn prepare(
        &self,
        root: &Path,
        set: &DaemonSet,
        force_registration: bool,
        owns_profile: bool,
        starting: &[String],
    ) -> Result<(State, super::ProjectLock)> {
        let providers = if starting.is_empty() {
            set.clone()
        } else {
            set.with_dependencies(starting)
        };
        super::providers::prepare_set(self, &providers, force_registration).await?;
        let lock = super::ProjectLock::acquire(root)?;
        let previous = read_state(root)?;
        // This root's configuration was read under the current profile, whoever
        // asked for it, so that is what the rendered definitions reflect and
        // what has to be recorded. Claiming the profile the root last used would
        // leave the generated file and the state describing different things.
        let profile = if root.starts_with(crate::dirs::STATE.join("daemon-providers")) {
            vec![]
        } else {
            crate::env::MISE_ENV.clone()
        };
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
            ids: if changed {
                Vec::new()
            } else {
                previous.ids.clone()
            },
            bin: self.bin.clone(),
            config_hash: String::new(),
            ports,
            ports_scan: String::new(),
            // Nothing is registered for a project without daemons, so there is
            // no label for the registry to hold.
            label: if set.daemons.is_empty() {
                String::new()
            } else {
                project_label(set, root)
            },
            label_registered: false,
            label_unsupported_by: String::new(),
            label_detach_pending: false,
        };
        for daemon in set.daemons.values() {
            let id = format!("{}/{}", state.namespace, daemon.name);
            if !state.ids.contains(&id) {
                state.ids.push(id);
            }
        }
        let content = render(set, &state)?;
        let file = state_dir(root).join("pitchfork.toml");
        // The pitchfork is part of it: a different or replaced one, such as one a
        // project was downgraded to, may not read this configuration, so it has to
        // pass the capability checks below again.
        state.config_hash = crate::hash::hash_to_str(&(&content, pitchfork_identity(&self.bin)));
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
        // The label lives in pitchfork's registry rather than the rendered file,
        // so the file comparing equal says nothing about it.
        let bin_stamp = executable_stamp(&self.bin);
        let label_current = label_is_current(&previous, &state, &bin_stamp);
        if !force_registration
            && !changed
            && label_current
            && state.config_hash == previous.config_hash
            && std::fs::read(&file).ok().as_deref() == Some(content.as_bytes())
        {
            // Profile and executable ownership may change without affecting the
            // rendered daemon configuration (for example with mise = false).
            state.label_registered = previous.label_registered;
            state.label_unsupported_by = previous.label_unsupported_by.clone();
            write_if_changed(
                &state_dir(root).join("state.json"),
                &serde_json::to_vec_pretty(&state)?,
            )?;
            return Ok((state, lock));
        }
        let external = self.supports_external_config(root).await?;
        let label = (external.label && !state.label.is_empty()).then(|| state.label.clone());
        if let Some(daemon) = set
            .daemons
            .values()
            .find(|d| d.table.get("run").is_some_and(toml::Value::is_array))
        {
            self.supports_argv_run(root, &daemon.name).await?;
        }
        // Pitchfork binds a registered file to its namespace. Detach the old
        // mapping before registering the same file under a different name.
        // The active check above ensures this cannot orphan running daemons.
        //
        // A new label needs no detaching: `config add --label` replaces the one
        // on record. Only losing the label does, because adding without one
        // leaves the old one in place. That is done only when nothing runs,
        // like a namespace change; a running project keeps the stale label
        // until it is stopped, which only affects how its hostname routes.
        let removing = changed || set.daemons.is_empty();
        let stale = label_is_stale(&previous, label.is_some());
        let running = stale && !removing && self.active(root, &previous).await?;
        let plan = plan_label(stale, removing, running);
        state.label_registered = label.is_some() || plan.pending;
        state.label_detach_pending = plan.pending;
        if !state.label.is_empty() && !external.label {
            state.label_unsupported_by = bin_stamp;
        }
        if removing || plan.detach {
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
            let mut args = vec![
                "config".into(),
                "add".into(),
                file.to_string_lossy().into_owned(),
                "--dir".into(),
                root.to_string_lossy().into_owned(),
                "--namespace".into(),
                state.namespace.clone(),
            ];
            if let Some(label) = label {
                args.push("--label".into());
                args.push(label);
            }
            self.output(root, &args).await?;
        }
        write_if_changed(
            &state_dir(root).join("state.json"),
            &serde_json::to_vec_pretty(&state)?,
        )?;
        Ok((state, lock))
    }

    /// Run pitchfork with the terminal, so its own output reaches the user.
    ///
    /// A failing command has therefore already said why, in its own words. mise
    /// keeps its status and adds nothing: the error it would otherwise raise
    /// reads `pitchfork exited with non-zero status`, then the version and a
    /// pointer to `--verbose`, none of which the user can act on.
    pub async fn exec(&self, root: &Path, args: Vec<String>) -> Result<()> {
        let mut runner = CmdLineRunner::new(&self.bin)
            .args(args)
            .envs(&self.env)
            .env_remove("PITCHFORK_CONFIG")
            .current_dir(root)
            .raw(true);
        for k in inherited_secret_env_names() {
            runner = runner.env_remove(k);
        }
        runner.with_pass_signals();
        match runner.execute_async().await {
            Err(err) => match crate::errors::ProcessError::get_exit_status(&err) {
                Some(code) => Err(crate::request_exit(code)),
                None => Err(err),
            },
            ok => ok,
        }
    }

    /// Start daemons, and if that fails, say how to get out of a port that is
    /// taken.
    ///
    /// `ids` are the daemons to check when the start fails: the ones `args` names
    /// and the dependencies pitchfork starts with them. `ports` are the claims this
    /// project recorded. Pitchfork reports a busy port in its own words, which
    /// cannot know that mise chose the number and that it can be pinned
    /// somewhere else, so mise adds that once pitchfork has said its part.
    pub async fn start(
        &self,
        root: &Path,
        args: Vec<String>,
        ids: &[String],
        ports: &BTreeMap<String, PortClaim>,
    ) -> Result<()> {
        let result = self.exec(root, args).await;
        if result.is_err() {
            for (name, claim) in self.taken_auto_ports(root, ids, ports).await {
                warn!(
                    "[daemons] {name} did not start and its automatically allocated port {} ({}) is in use by another process. To move it, {PIN_A_PORT}",
                    claim.port,
                    port_origin(&claim)
                );
            }
        }
        result
    }

    /// The daemons in `ids` whose automatically allocated port is held by
    /// something other than the daemon itself.
    async fn taken_auto_ports(
        &self,
        root: &Path,
        ids: &[String],
        ports: &BTreeMap<String, PortClaim>,
    ) -> Vec<(String, PortClaim)> {
        let mut taken = Vec::new();
        for id in ids {
            let name = id.rsplit('/').next().unwrap_or(id);
            let Some(claim) = ports.get(name).filter(|claim| claim.is_auto()) else {
                continue;
            };
            if !port_is_taken(claim.port) {
                continue;
            }
            // A daemon that is up holds its own port, and a sibling's failure
            // must not make that look like a conflict.
            let running = self.status(root, id).await.ok().is_some_and(|value| {
                matches!(
                    value["status"].as_str(),
                    Some("running" | "waiting" | "stopping")
                )
            });
            if !running {
                taken.push((name.to_string(), *claim));
            }
        }
        taken
    }
}

/// Whether the registry already holds the label this state wants.
///
/// A label that was never registered because pitchfork could not take one is
/// retried once the executable changes, the only way it can have learned the
/// flag. Comparing the executable's identity, not probing it, keeps this off the
/// shell-hook path; the identity moves when the file is replaced in place, as a
/// package manager upgrade does, not only when its path changes.
fn label_is_current(previous: &State, wanted: &State, bin_stamp: &str) -> bool {
    wanted.label == previous.label
        && !previous.label_detach_pending
        && (previous.label_registered
            || wanted.label.is_empty()
            || (!bin_stamp.is_empty() && previous.label_unsupported_by == bin_stamp))
}

/// Path, size and modification time of the executable, following symlinks.
/// Empty when it cannot be examined, which never matches a recorded identity.
fn executable_stamp(bin: &Path) -> String {
    let Ok(meta) = std::fs::metadata(bin) else {
        return String::new();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("{}:{}:{modified}", bin.display(), meta.len())
}

/// The registry holds a label that this registration no longer asks for.
fn label_is_stale(previous: &State, wants_label: bool) -> bool {
    previous.label_registered && !wants_label
}

/// What to do about the registry's label once it is known to be unwanted.
#[derive(Debug, PartialEq, Eq)]
struct LabelPlan {
    /// Run `config remove` beyond the removal the registration already needs.
    detach: bool,
    /// The removal was postponed and the old label is still on record.
    pending: bool,
}

/// `removing` means the registration detaches the file anyway; `running` means
/// the project has daemons up, which forbids detaching it now.
fn plan_label(stale: bool, removing: bool, running: bool) -> LabelPlan {
    LabelPlan {
        detach: stale && !removing && !running,
        pending: stale && !removing && running,
    }
}

/// The hostname label mise advertises for a project, empty when it has none.
///
/// Pitchfork treats a registered namespace as the project's hostname label, and
/// the default namespace is a hash that keeps unrelated checkouts apart. Left
/// to itself it would serve `web.shop-528f92b13a6784f0.localhost` while mise
/// prints and exports `web.shop.localhost`, so the registration says what the
/// label is.
fn project_label(set: &DaemonSet, root: &Path) -> String {
    set.labels
        .get(root)
        .or_else(|| root.canonicalize().ok().and_then(|r| set.labels.get(&r)))
        .and_then(|labels| labels.project.clone())
        .unwrap_or_default()
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

pub fn namespace(root: &Path) -> Result<String> {
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

pub(crate) fn render(set: &DaemonSet, state: &State) -> Result<String> {
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
        if let Some(binding) = &daemon.provider {
            table.insert(
                "depends".into(),
                toml::Value::Array(vec![binding.provider.id().into()]),
            );
        }
        // Pitchfork wraps the main process in mise, but runs readiness probes
        // directly. Resolve custom probes in this checkout too: the supervisor
        // may have inherited another worktree's tools and endpoint variables.
        // Preset and task probes already carry their own environment wrapper.
        if daemon.preset.is_none()
            && daemon.task.is_none()
            && table.get("mise").and_then(toml::Value::as_bool) != Some(false)
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

pub async fn validate_tools(set: &DaemonSet, config: &Arc<Config>, ts: &Toolset) -> Result<()> {
    for daemon in set.daemons.values() {
        // Imported daemons resolve their tool against the project that declares
        // them, which happens when that root is prepared.
        let Some((tool, version)) = daemon.tool.as_ref().filter(|_| !daemon.imported) else {
            continue;
        };
        let ba: crate::args::BackendArg = tool.as_str().into();
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

/// Which pitchfork executable this is, from its path, size and modification time,
/// so that replacing it in place, as a package manager does, is noticed too.
fn pitchfork_identity(bin: &Path) -> String {
    let meta = std::fs::metadata(bin).ok();
    let stamp = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let len = meta.map(|m| m.len()).unwrap_or_default();
    format!("{}:{len}:{stamp}", bin.display())
}

/// Whether a `pitchfork schema` lets a daemon's `run` be an array. pitchfork
/// 2.28.0 made it a reference to a string-or-array type; before, it was a string.
fn schema_accepts_argv_run(schema: &serde_json::Value) -> bool {
    let defs = &schema["$defs"];
    let mut run = &defs["PitchforkTomlDaemon"]["properties"]["run"];
    if let Some(name) = run["$ref"]
        .as_str()
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        run = &defs[name];
    }
    let is_array = |variant: &serde_json::Value| variant["type"] == "array";
    is_array(run)
        || ["anyOf", "oneOf"]
            .iter()
            .any(|key| run[key].as_array().is_some_and(|v| v.iter().any(is_array)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_loopback_port_is_taken() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_is_taken(port));
        drop(listener);
        assert!(!port_is_taken(port));
    }

    #[test]
    fn a_port_held_only_on_ipv6_loopback_is_taken() {
        // A host without IPv6 loopback has nothing to prove here.
        let Ok(listener) = std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, 0)) else {
            return;
        };
        let port = listener.local_addr().unwrap().port();
        assert!(port_is_taken(port));
        drop(listener);
        assert!(!port_is_taken(port));
    }

    #[test]
    fn a_port_is_described_by_where_it_came_from() {
        let claim = |port| PortClaim {
            port,
            base: 3000,
            stride: 1,
        };
        assert_eq!(port_origin(&claim(3000)), "the configured base");
        assert_eq!(
            port_origin(&claim(3007)),
            "the base offset by this worktree's path"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn inherited_secrets_are_removed_from_the_pitchfork_command() {
        let mut command = Command::new("/usr/bin/env");
        command.env("FOO", "secret").env("KEEP", "x");
        strip_inherited_secrets(&mut command, &BTreeSet::from(["FOO".to_string()]));
        let out = command.output().await.unwrap();
        let out = String::from_utf8_lossy(&out.stdout);
        assert!(!out.lines().any(|l| l.starts_with("FOO=")));
        assert!(out.lines().any(|l| l == "KEEP=x"));
    }

    /// A failing pitchfork has already explained itself on the terminal, so the
    /// error mise raises is only the status it exits with. A signal has no
    /// status to hand on and stays an ordinary error.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_pitchfork_exits_quietly_with_its_status() {
        let runtime = Runtime {
            bin: PathBuf::from("/bin/sh"),
            env: EnvMap::default(),
        };
        let root = std::env::temp_dir();
        let exec = |script: &str| runtime.exec(&root, vec!["-c".into(), script.into()]);
        exec("exit 0").await.unwrap();
        let err = exec("exit 3").await.unwrap_err();
        assert_eq!(crate::exit::requested_exit_code(&err), Some(3));
        let err = exec("kill -9 $$").await.unwrap_err();
        assert_eq!(crate::exit::requested_exit_code(&err), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn only_a_taken_auto_port_of_a_daemon_that_is_not_running_is_reported() {
        // `sh status ...` fails, which is how an unknown daemon looks.
        let runtime = Runtime {
            bin: PathBuf::from("/bin/sh"),
            env: EnvMap::default(),
        };
        let root = std::env::temp_dir();
        let held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let taken = held.local_addr().unwrap().port();
        let free = {
            let probe = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            probe.local_addr().unwrap().port()
        };
        let claim = |port| PortClaim {
            port,
            base: port,
            stride: 1,
        };
        let ports = BTreeMap::from([
            ("api".to_string(), claim(taken)),
            ("web".to_string(), claim(free)),
            ("db".to_string(), PortClaim::fixed(taken)),
        ]);
        let ids: Vec<String> = ["ns/api", "ns/web", "ns/db"].map(String::from).into();
        assert_eq!(
            runtime.taken_auto_ports(&root, &ids, &ports).await,
            vec![("api".to_string(), ports["api"])],
            "a free port and a port the user fixed are not this hint's business"
        );
    }

    const USAGE_WITHOUT_LABEL: &str = r##"cmd daemons help="List configured daemons." {
    cmd add help="Add a new daemon" effect=write {
        long_help #"""
Add a new daemon to pitchfork.toml

Examples:

    pitchfork daemons add api --label nope
"""#
        flag --label {
            arg <LABEL>
        }
    }
}
cmd config help="Attach externally generated configuration to a project." {
    cmd add help="Register a configuration file." effect=write {
        flag --dir {
            arg <DIR>
        }
        flag --namespace {
            arg <NAMESPACE>
        }
        arg <FILE>
    }
    cmd remove help="Detach a configuration file." {
        flag --label
        arg <FILE>
    }
}
"##;

    #[test]
    fn config_add_label_is_detected_from_usage() {
        assert!(!config_add_takes_label(""));
        // `--label` on `daemons add` and on `config remove` is not the flag.
        assert!(!config_add_takes_label(USAGE_WITHOUT_LABEL));
        let with = USAGE_WITHOUT_LABEL.replace(
            "        flag --namespace {\n            arg <NAMESPACE>\n        }\n",
            "        flag --namespace {\n            arg <NAMESPACE>\n        }\n        flag --label {\n            arg <LABEL>\n        }\n",
        );
        assert_ne!(with, USAGE_WITHOUT_LABEL);
        assert!(config_add_takes_label(&with));
        // A short-and-long spelling counts too.
        let quoted = with.replace(
            "flag --label {\n            arg <LABEL>\n        }\n        arg <FILE>",
            "flag \"-l --label\" {\n            arg <LABEL>\n        }\n        arg <FILE>",
        );
        assert!(config_add_takes_label(&quoted));
    }

    fn label_state(label: &str, registered: bool, unsupported_by: &str) -> State {
        State {
            label: label.into(),
            label_registered: registered,
            label_unsupported_by: unsupported_by.into(),
            ..State::default()
        }
    }

    #[test]
    fn a_changed_or_unregistered_label_is_registered_again() {
        let current = label_state("shop", true, "");
        assert!(label_is_current(
            &current,
            &label_state("shop", true, ""),
            ""
        ));
        // A renamed project or directory changes only the label.
        assert!(!label_is_current(
            &current,
            &label_state("store", true, ""),
            ""
        ));
        assert!(!label_is_current(&current, &label_state("", true, ""), ""));
        // State written before labels existed has none recorded.
        assert!(!label_is_current(&State::default(), &current, ""));
        // A pitchfork that took no label: nothing to retry while it is the same
        // executable, but a replaced one may understand the flag.
        let unregistered = label_state("shop", false, "/pf:100:1");
        let wanted = label_state("shop", false, "");
        assert!(label_is_current(&unregistered, &wanted, "/pf:100:1"));
        assert!(!label_is_current(&unregistered, &wanted, "/pf:100:2"));
        assert!(!label_is_current(&unregistered, &wanted, "/pf:200:1"));
        assert!(!label_is_current(&unregistered, &wanted, "/other:100:1"));
        // An executable that cannot be examined is never taken to be unchanged.
        assert!(!label_is_current(
            &label_state("shop", false, ""),
            &wanted,
            ""
        ));
        // A project mise cannot name never has a label to retry.
        let unnamed = label_state("", false, "");
        assert!(label_is_current(
            &unnamed,
            &label_state("", false, ""),
            "/pf:1:1"
        ));
    }

    #[test]
    fn an_upgrade_in_place_changes_the_executable_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("pitchfork");
        assert_eq!(executable_stamp(&bin), "");
        std::fs::write(&bin, b"2.25").unwrap();
        let before = executable_stamp(&bin);
        assert_eq!(before, executable_stamp(&bin));
        std::fs::write(&bin, b"2.28 with --label").unwrap();
        let after = executable_stamp(&bin);
        assert_ne!(before, after);
        let unregistered = label_state("shop", false, &before);
        let wanted = label_state("shop", false, "");
        assert!(label_is_current(&unregistered, &wanted, &before));
        assert!(!label_is_current(&unregistered, &wanted, &after));
    }

    #[test]
    fn a_deferred_detach_is_finished_once_the_project_stops() {
        // Registered under a label the configuration then stops asking for.
        let registered = label_state("shop", true, "");
        let dropped = label_state("", false, "");
        assert!(!label_is_current(&registered, &dropped, ""));
        let stale = label_is_stale(&registered, false);
        assert!(stale);

        // While its daemons run, `config remove` would orphan them: postponed,
        // with the old label still on record and the fact remembered.
        let plan = plan_label(stale, false, true);
        assert_eq!(
            plan,
            LabelPlan {
                detach: false,
                pending: true
            }
        );
        let deferred = State {
            label_registered: plan.pending,
            label_detach_pending: plan.pending,
            ..dropped.clone()
        };
        assert!(deferred.label_registered);
        // The unchanged fast path cannot be taken with a removal owed, however
        // much else is unchanged.
        assert!(!label_is_current(&deferred, &dropped, ""));

        // Still running on the next registration: still postponed.
        let stale = label_is_stale(&deferred, false);
        assert!(stale);
        assert!(plan_label(stale, false, true).pending);

        // Once stopped it is removed, and the marker clears with it.
        let plan = plan_label(stale, false, false);
        assert_eq!(
            plan,
            LabelPlan {
                detach: true,
                pending: false
            }
        );
        let finished = State {
            label_registered: false,
            label_detach_pending: plan.pending,
            ..dropped.clone()
        };
        assert!(label_is_current(&finished, &dropped, ""));
        assert!(!label_is_stale(&finished, false));

        // A removal the registration performs anyway settles it too.
        let plan = plan_label(stale, true, true);
        assert_eq!(
            plan,
            LabelPlan {
                detach: false,
                pending: false
            }
        );
        // And a label that is still wanted is never stale.
        assert!(!label_is_stale(&registered, true));
    }

    #[test]
    fn project_label_reads_the_roots_own_labels() {
        let root = PathBuf::from("/project");
        let mut set = DaemonSet::default();
        assert_eq!(project_label(&set, &root), "");
        set.labels.insert(
            root.clone(),
            super::super::urls::RootLabels {
                project: Some("shop".into()),
                worktree: Some("feature".into()),
            },
        );
        // The worktree component never becomes part of the registered label.
        assert_eq!(project_label(&set, &root), "shop");
        assert_eq!(project_label(&set, Path::new("/other")), "");
    }

    /// The shape of `run` in the schemas pitchfork 2.27.0 and 2.28.0 print.
    #[test]
    fn argv_run_support_is_read_from_the_schema() {
        let before = serde_json::json!({
            "$defs": { "PitchforkTomlDaemon": { "properties": {
                "run": { "type": "string", "examples": ["exec node server.js"] }
            } } }
        });
        let after = serde_json::json!({
            "$defs": {
                "PitchforkTomlDaemon": { "properties": { "run": { "$ref": "#/$defs/RunCommand" } } },
                "RunCommand": { "anyOf": [
                    { "type": "string" },
                    { "type": "array", "items": { "type": "string" }, "minItems": 1 }
                ] }
            }
        });
        assert!(!schema_accepts_argv_run(&before));
        assert!(schema_accepts_argv_run(&after));
        assert!(!schema_accepts_argv_run(&serde_json::json!({})));
    }

    #[test]
    fn replacing_pitchfork_in_place_changes_its_identity() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("pitchfork");
        std::fs::write(&bin, "2.28.0").unwrap();
        let before = pitchfork_identity(&bin);
        assert_eq!(pitchfork_identity(&bin), before);
        std::fs::write(&bin, "2.27").unwrap();
        assert_ne!(pitchfork_identity(&bin), before);
    }

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
            data_dir: None,
            task: None,
            tool: None,
            provider: None,
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
            data_dir: None,
            task: None,
            tool: None,
            provider: None,
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
            data_dir: None,
            task: None,
            tool: None,
            provider: None,
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
            data_dir: None,
            task: task.map(str::to_string),
            tool: None,
            provider: None,
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
