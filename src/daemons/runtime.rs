use super::{DaemonSet, DaemonSettings, state_dir};
use crate::cli::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::env_diff::EnvMap;
use crate::toolset::{ToolRequest, ToolSource, Toolset, ToolsetBuilder};
use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
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

pub(crate) async fn toolset(config: &Arc<Config>, install: bool) -> Result<(Arc<Config>, Toolset)> {
    let mut config = config.clone();
    let pitchfork: ToolArg = "pitchfork".parse()?;
    let args = if install
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
    let mut ts = ToolsetBuilder::new()
        .with_args(&args)
        .with_default_to_latest(true)
        .build(&config)
        .await?;
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

    /// Register a root's daemons.
    ///
    /// `owns_profile` is false when another project is preparing this root
    /// because it imported a daemon from it. The environment profile belongs to
    /// the project that owns the root, so the importer neither imposes its own
    /// nor is told that the two conflict.
    pub(crate) async fn prepare(
        &self,
        root: &Path,
        set: &DaemonSet,
        force_registration: bool,
        owns_profile: bool,
    ) -> Result<(State, fslock::LockFile)> {
        let lock = crate::lock_file::LockFile::at(&state_dir(root).join("project.lock")).lock()?;
        let previous = read_state(root)?;
        let profile = if owns_profile {
            crate::env::MISE_ENV.clone()
        } else {
            previous.profile.clone()
        };
        if owns_profile
            && !previous.namespace.is_empty()
            && previous.profile != profile
            && self.active(root, &previous).await?
        {
            bail!(
                "another daemon profile is active for {}; stop its daemons and leave its shell sessions before switching MISE_ENV",
                root.display()
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
        let mut state = State {
            root: root.into(),
            profile,
            namespace: desired,
            // IDs from the previous namespace name daemons that are no longer
            // reachable; nothing is running under them, so drop them here rather
            // than forwarding unresolvable IDs to pitchfork.
            ids: if changed { Vec::new() } else { previous.ids },
            bin: self.bin.clone(),
            config_hash: String::new(),
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
    crate::daemons::validate_id("namespace", explicit)?;
    if settings.is_none_or(|s| s.namespace_per_worktree()) && is_linked_worktree(root) {
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

/// Whether `root` sits in a linked git worktree rather than the main checkout.
///
/// A linked worktree's `.git` is a file pointing into the main repository's
/// `worktrees/` directory, which is what `git rev-parse --git-common-dir`
/// reports as a path outside the worktree. Reading the file directly keeps this
/// out of subprocess territory: namespaces are resolved on every config load,
/// including the activation hook. A submodule also uses a `.git` file, but it
/// points at `modules/`, so only `worktrees/` counts here.
fn is_linked_worktree(root: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    for dir in root.ancestors() {
        let git = dir.join(".git");
        if git.is_dir() {
            return false;
        }
        if git.is_file() {
            return std::fs::read_to_string(&git).is_ok_and(|s| {
                s.trim().starts_with("gitdir:")
                    && (s.contains("/worktrees/") || s.contains("\\worktrees\\"))
            });
        }
    }
    false
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
        // Ensure mise x sees the profile that generated this definition, even at boot.
        if !state.profile.is_empty()
            && table.get("mise").and_then(toml::Value::as_bool) != Some(false)
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
        std::fs::create_dir_all(main.join(".git").join("worktrees").join("feature")).unwrap();
        let worktree = tmp.path().join("feature");
        std::fs::create_dir(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!(
                "gitdir: {}\n",
                main.join(".git/worktrees/feature").display()
            ),
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
