//! `[history]`: the operational history configuration read from the system
//! and global configuration layers only. Project configuration found from
//! the current directory never contributes, so no project can change what
//! personal history captures.

use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::Settings;
use crate::config::config_file::mise_toml::MiseToml;
use crate::file::display_path;

/// `[history]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct HistoryTomlConfig {
    /// Globs (with `~`) never captured; `!glob` re-includes.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Commands to run after a rollback touches a matching path.
    #[serde(default)]
    pub reload: IndexMap<String, String>,
    /// A task to run once a setup repository has been adopted.
    #[serde(default)]
    pub post_adopt: Option<String>,
    /// The setup repository this machine publishes to and fetches from.
    #[serde(default)]
    pub origin: Option<OriginTomlConfig>,
    #[serde(default)]
    pub encryption: Option<FileEncryptionConfig>,
}

/// Public recipients shared by every encrypted dotfile.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileEncryptionConfig {
    #[serde(default)]
    pub recipients: Vec<String>,
}

pub(crate) fn file_recipients() -> Result<Vec<String>> {
    let mut recipients = Vec::new();
    for (path, layer) in layers()? {
        if let Some(encryption) = layer.encryption {
            if !crate::config::config_file::is_trusted(&path) {
                eyre::bail!(
                    "trust the configuration before using its encryption recipients: {}",
                    display_path(&path)
                );
            }
            recipients = encryption.recipients;
            if recipients.is_empty() {
                eyre::bail!(
                    "[history.encryption].recipients must not be empty; configure recipients before capturing encrypted files"
                );
            }
        }
    }
    Ok(recipients)
}

/// `[history.origin]`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginTomlConfig {
    pub url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
}

impl OriginTomlConfig {
    /// A plaintext connection, as recorded before any declaration.
    pub(crate) fn plain(url: String, branch: String) -> Self {
        Self { url, branch }
    }
}

fn default_branch() -> String {
    "main".to_string()
}

/// The effective `settings.history.describe_command`: the environment wins,
/// otherwise the last trusted system/global layer that declares it wins.
/// Project settings are deliberately never considered: this command receives
/// personal dotfile contents and must not be controlled by a repository.
pub(crate) fn describe_command() -> Result<Option<String>> {
    if let Some(command) = std::env::var_os("MISE_HISTORY_DESCRIBE_COMMAND") {
        let command = command
            .into_string()
            .map_err(|_| eyre::eyre!("MISE_HISTORY_DESCRIBE_COMMAND contains invalid Unicode"))?;
        return Ok(nonempty_command(command));
    }

    let mut found = None;
    for path in config_files() {
        let settings = match Settings::parse_settings_file(&path) {
            Ok(settings) => settings,
            Err(err) => {
                warn!(
                    "history: cannot read description command from {}: {err}",
                    display_path(&path)
                );
                continue;
            }
        };
        if let Some(command) = settings.history.describe_command {
            if !crate::config::config_file::is_trusted(&path) {
                warn!(
                    "history: ignoring settings.history.describe_command in untrusted {}",
                    display_path(&path)
                );
                continue;
            }
            found = Some(command);
        }
    }
    Ok(found.and_then(nonempty_command))
}

fn nonempty_command(command: String) -> Option<String> {
    let command = command.trim().to_string();
    (!command.is_empty()).then_some(command)
}

/// The effective `[history.origin]`: the last layer that declares one.
pub(crate) fn origin() -> Result<Option<(PathBuf, OriginTomlConfig)>> {
    let mut found = None;
    for (path, layer) in layers()? {
        if let Some(origin) = layer.origin {
            super::sync::network::validate_url(&origin.url)?;
            found = Some((path, origin));
        }
    }
    Ok(found)
}

/// The `[history]` tables of the system and global layers, in discovery
/// order, each with the file that declared it.
pub(crate) fn layers() -> Result<Vec<(PathBuf, HistoryTomlConfig)>> {
    let mut layers = vec![];
    for path in config_files() {
        if let Some(history) = read_layer(&path)? {
            layers.push((path, history));
        }
    }
    Ok(layers)
}

fn config_files() -> impl Iterator<Item = PathBuf> {
    crate::config::system_config_files()
        .into_iter()
        .chain(crate::config::global_config_files())
        .filter(|path| path.is_file())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
}

fn read_layer(path: &Path) -> Result<Option<HistoryTomlConfig>> {
    // Ignoring a malformed later layer could restore obsolete recipients or
    // remove exclusions. Fail closed instead of falling back to another policy.
    let toml = MiseToml::from_file(path)
        .wrap_err_with(|| format!("cannot read history configuration: {}", display_path(path)))?;
    Ok(toml.history_config())
}

/// The task to run after a setup repository is adopted, from the last
/// trusted layer that names one.
///
/// **A setup is not finished when its files land.** Changing the login
/// shell, rebuilding a font cache, importing a keyring — the things a new
/// machine needs once the configuration arrives — have nowhere to go
/// today. A `bootstrap` task runs on every bootstrap, which is the wrong
/// place for work that should happen once.
///
/// It is read under the same trust rule as `reload`: a layer the user has
/// not trusted does not get to name a command. The global configuration
/// an adoption writes is trusted by definition, which is not a gap —
/// adopting a repository already installs its packages and services and
/// runs its `bootstrap` task, so the same decision covers this.
pub(crate) fn post_adopt_task() -> Result<Option<String>> {
    // The configuration this reads was installed moments ago, by this same
    // process: the cached global discovery predates it and would find
    // nothing, so the global layers are rediscovered from the directory.
    let global = crate::config::config_files_with_incoming(
        &super::tracked::global_config_dir(),
        &Default::default(),
    );
    let paths = crate::config::system_config_files()
        .into_iter()
        .chain(global);
    last_post_adopt(paths)
}

/// The last layer that names a post-adopt task, in discovery order.
fn last_post_adopt(paths: impl IntoIterator<Item = PathBuf>) -> Result<Option<String>> {
    let mut found = None;
    for path in paths {
        if !path.is_file() || path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let Some(task) = read_layer(&path)?.and_then(|layer| layer.post_adopt) else {
            continue;
        };
        if !crate::config::config_file::is_trusted(&path) {
            warn!(
                "history: ignoring [history] post_adopt in untrusted {}",
                display_path(&path)
            );
            continue;
        }
        found = nonempty_command(task);
    }
    Ok(found)
}

/// The post-adopt tasks this machine has already finished, one per line.
///
/// **Once means once.** A `mise bootstrap` on a machine that is already
/// set up must not change the login shell again or re-import a keyring,
/// so what ran is remembered rather than inferred from whether an
/// adoption happened to run just now. Only a task that succeeded is
/// recorded, so a failure the user fixes still runs on the next attempt.
fn post_adopt_record() -> PathBuf {
    super::store::store_dir_in(&super::store::state_dir()).join("post-adopt")
}

/// What "this task, from this setup" is recorded as.
///
/// **A task name alone is not an identity.** `setup` is what half of
/// these tasks will be called, and a machine that later adopts a
/// different repository must still get that repository's task run. The
/// setup this machine is connected to is part of the key, so changing
/// setups changes the answer; a machine with no recorded origin keys on
/// the name alone, because there is nothing else to tell two apart.
pub(crate) fn post_adopt_key(task: &str) -> Result<String> {
    let setup = origin()?
        .map(|(_, origin)| format!("{}#{}", origin.url, origin.branch))
        .unwrap_or_default();
    Ok(format!("{setup}\t{task}"))
}

/// Whether this machine has already finished the task `key` names.
pub(crate) fn post_adopt_already_ran(key: &str) -> bool {
    std::fs::read_to_string(post_adopt_record())
        .map(|ran| ran.lines().any(|line| line == key))
        .unwrap_or(false)
}

/// Records the task `key` names as finished on this machine.
pub(crate) fn record_post_adopt(key: &str) -> Result<()> {
    let path = post_adopt_record();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut ran = std::fs::read_to_string(&path).unwrap_or_default();
    if !ran.is_empty() && !ran.ends_with('\n') {
        ran.push('\n');
    }
    ran.push_str(key);
    ran.push('\n');
    std::fs::write(&path, ran)
        .wrap_err_with(|| format!("recording the post-adopt task in {}", display_path(&path)))?;
    Ok(())
}

/// The effective reload map: glob -> command, a later layer overriding an
/// earlier one for the same glob. Read from the trusted layers only, and
/// resolved before an operation begins so nothing it writes can change it.
pub(crate) fn reload_commands() -> Result<IndexMap<String, String>> {
    let mut commands = IndexMap::new();
    for (path, layer) in layers()? {
        if !crate::config::config_file::is_trusted(&path) {
            if !layer.reload.is_empty() {
                warn!(
                    "history: ignoring [history.reload] in untrusted {}",
                    display_path(&path)
                );
            }
            continue;
        }
        for (glob, command) in &layer.reload {
            commands.insert(glob.clone(), command.clone());
        }
    }
    Ok(commands)
}

/// The effective `exclude` list: every layer's globs in order, later
/// layers after earlier ones, repeats included. Patterns apply in order
/// and the last match wins, so `!glob` re-includes what an earlier glob
/// excluded and a repeated glob excludes again what a `!glob` in between
/// re-included.
pub(crate) fn exclude_globs() -> Result<Vec<String>> {
    let mut globs: Vec<String> = vec![];
    for (_, layer) in layers()? {
        globs.extend(layer.exclude.iter().cloned());
    }
    Ok(globs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_encryption_is_rejected_and_layer_errors_propagate() {
        let error = toml::from_str::<HistoryTomlConfig>("[encryption]\nrecipents = ['typo']\n")
            .unwrap_err();
        assert!(error.to_string().contains("recipents"));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[history.encryption]\nrecipents = ['typo']\n").unwrap();
        let error = read_layer(&path).unwrap_err();
        assert!(format!("{error:#}").contains("config.toml"));
    }

    /// The layers are read in mise's own precedence order, so what is
    /// reported is what the bootstrap runs: the last layer that names a
    /// task, and nothing when none does or the name is blank.
    #[test]
    fn the_last_layer_that_names_a_post_adopt_task_wins() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            last_post_adopt(crate::config::config_files_with_incoming(
                temp.path(),
                &Default::default()
            ))
            .unwrap(),
            None
        );

        std::fs::write(
            temp.path().join("config.toml"),
            "[history]
post_adopt = 'setup'
",
        )
        .unwrap();
        assert_eq!(
            last_post_adopt(crate::config::config_files_with_incoming(
                temp.path(),
                &Default::default()
            ))
            .unwrap()
            .as_deref(),
            Some("setup")
        );

        // a later layer overrides an earlier one, exactly as the
        // installed layers do
        std::fs::write(
            temp.path().join("config.local.toml"),
            "[history]
post_adopt = 'machine-setup'
",
        )
        .unwrap();
        assert_eq!(
            last_post_adopt(crate::config::config_files_with_incoming(
                temp.path(),
                &Default::default()
            ))
            .unwrap()
            .as_deref(),
            Some("machine-setup")
        );

        // and an empty name turns it off rather than running `mise run`
        // with no task
        std::fs::write(
            temp.path().join("config.local.toml"),
            "[history]
post_adopt = '  '
",
        )
        .unwrap();
        assert_eq!(
            last_post_adopt(crate::config::config_files_with_incoming(
                temp.path(),
                &Default::default()
            ))
            .unwrap(),
            None
        );

        // a directory that is not there at all is not an error: a setup
        // without configuration simply brings no task
        assert_eq!(
            last_post_adopt(crate::config::config_files_with_incoming(
                &temp.path().join("missing"),
                &Default::default(),
            ))
            .unwrap(),
            None
        );
    }
}
