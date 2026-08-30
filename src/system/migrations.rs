//! Ordered, once-per-machine bootstrap migrations from `mise-migrations/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use eyre::{Result, bail, eyre};
use serde::Serialize;

use crate::config::Config;
use crate::path::PathExt;
use crate::{dirs, file, hash};

const MIGRATIONS_DIR: &str = "mise-migrations";

#[derive(Clone, Debug)]
pub(crate) struct Migration {
    pub id: String,
    pub path: PathBuf,
    pub root: PathBuf,
    digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MigrationState {
    Pending,
    Applied,
    Modified,
}

impl std::fmt::Display for MigrationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Applied => write!(f, "applied"),
            Self::Modified => write!(f, "modified after apply"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationStatus {
    pub id: String,
    pub path: PathBuf,
    pub state: MigrationState,
}

impl MigrationStatus {
    pub(crate) fn missing(&self) -> bool {
        self.state != MigrationState::Applied
    }
}

fn migrations_root(config: &Config) -> PathBuf {
    config
        .project_root
        .clone()
        .unwrap_or_else(|| dirs::CONFIG.to_path_buf())
}

fn state_dir() -> PathBuf {
    dirs::STATE.join("bootstrap").join("migrations")
}

fn state_path(id: &str) -> PathBuf {
    state_dir().join(id)
}

fn migration_state(migration: &Migration) -> Result<MigrationState> {
    let path = state_path(&migration.id);
    let applied_digest = match fs::read_to_string(&path) {
        Ok(value) => value.trim().to_string(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MigrationState::Pending);
        }
        Err(err) => return Err(err.into()),
    };
    Ok(if applied_digest == migration.digest {
        MigrationState::Applied
    } else {
        MigrationState::Modified
    })
}

pub(crate) fn discover(config: &Config) -> Result<Vec<Migration>> {
    let root = migrations_root(config);
    let directory = root.join(MIGRATIONS_DIR);
    if !directory.exists() {
        return Ok(vec![]);
    }
    if !directory.is_dir() {
        bail!("{} must be a directory", directory.display_user());
    }

    let mut migrations = BTreeMap::new();
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let id = entry.file_name().into_string().map_err(|_| {
            eyre!(
                "migration names must be valid UTF-8: {}",
                path.display_user()
            )
        })?;
        if id.starts_with('.') {
            continue;
        }
        if !file::is_executable(&path) {
            bail!(
                "bootstrap migration {} is not executable. {}",
                path.display_user(),
                file::make_executable_hint(&path)
            );
        }
        let digest = hash::file_hash_blake3(&path, None)?;
        migrations.insert(
            id.clone(),
            Migration {
                id,
                path,
                root: root.clone(),
                digest,
            },
        );
    }
    Ok(migrations.into_values().collect())
}

pub(crate) fn statuses(config: &Config) -> Result<Vec<MigrationStatus>> {
    discover(config)?
        .into_iter()
        .map(|migration| {
            Ok(MigrationStatus {
                id: migration.id.clone(),
                path: migration.path.clone(),
                state: migration_state(&migration)?,
            })
        })
        .collect()
}

pub(crate) fn apply(config: &Config, dry_run: bool) -> Result<()> {
    let migrations = discover(config)?;
    if migrations.is_empty() {
        debug!("bootstrap: no {MIGRATIONS_DIR} directory or migrations configured");
        return Ok(());
    }
    let _lock = if dry_run {
        None
    } else {
        file::create_dir_all(state_dir())?;
        Some(crate::lock_file::LockFile::new(&state_dir()).lock()?)
    };

    for migration in migrations {
        match migration_state(&migration)? {
            MigrationState::Applied => continue,
            MigrationState::Modified => bail!(
                "bootstrap migration '{}' changed after it was applied; restore the original file and add a new migration",
                migration.id
            ),
            MigrationState::Pending => {}
        }

        if dry_run {
            miseprintln!(
                "Would run bootstrap migration {}",
                migration.path.display_user()
            );
            continue;
        }

        info!("bootstrap: migration {}", migration.id);
        let status = Command::new(std::env::current_exe()?)
            .arg("--cd")
            .arg(&migration.root)
            .args(["exec", "--"])
            .arg(&migration.path)
            .env("MISE_BOOTSTRAP_MIGRATION", &migration.id)
            .status()?;
        if !status.success() {
            bail!(
                "bootstrap migration '{}' failed with {status}",
                migration.id
            );
        }

        let digest_after = hash::file_hash_blake3(&migration.path, None)?;
        if digest_after != migration.digest {
            bail!(
                "bootstrap migration '{}' changed while it was running and was not recorded as applied",
                migration.id
            );
        }
        file::write_atomic(state_path(&migration.id), format!("{}\n", migration.digest))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn migration_state_display_is_human_readable() {
        assert_eq!(MigrationState::Pending.to_string(), "pending");
        assert_eq!(MigrationState::Applied.to_string(), "applied");
        assert_eq!(MigrationState::Modified.to_string(), "modified after apply");
    }

    #[test]
    fn state_path_is_scoped_to_bootstrap_migrations() {
        assert!(
            state_path("20260830-example")
                .ends_with(Path::new("bootstrap/migrations/20260830-example"))
        );
    }
}
