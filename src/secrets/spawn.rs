//! What one child process gets: values for its env map, files for `as_file` keys.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};

use super::{SecretName, SecretValue};
use crate::env_diff::EnvMap;
use mise_util::env::env_key_eq;

/// Files that hold `as_file` secrets for one spawn, in a private directory of their own with
/// an unpredictable name. Dropping deletes them, so they live exactly as long as the task
/// that was granted them.
#[derive(Default)]
pub(crate) struct TempSecretFiles {
    paths: Vec<PathBuf>,
    dir: Option<tempfile::TempDir>,
}

impl TempSecretFiles {
    /// Writes each value to `<dir>/<random>/<KEY>` (0600, never overwriting) and returns the
    /// `KEY=<path>` pairs. `dir` is created with 0700 if missing, and the per-spawn
    /// directory inside it is 0700 and named unpredictably.
    pub(crate) fn create(
        dir: &Path,
        entries: &BTreeMap<SecretName, SecretValue>,
    ) -> Result<(Self, BTreeMap<String, String>)> {
        let mut files = Self::default();
        let mut env = BTreeMap::new();
        if entries.is_empty() {
            return Ok((files, env));
        }
        create_private_dir(dir)?;
        let spawn_dir = tempfile::Builder::new()
            .prefix("s-")
            .tempdir_in(dir)
            .wrap_err("mise secrets: cannot create a directory for secret files")?;
        let base = spawn_dir.path().to_path_buf();
        restrict_dir(&base)?;
        files.dir = Some(spawn_dir);
        for (key, value) in entries {
            let path = base.join(key.as_str());
            let mut file = open_private(&path)
                .wrap_err_with(|| format!("mise secrets: cannot write a file for {key}"))?;
            files.paths.push(path.clone());
            file.write_all(value.expose().as_bytes())
                .wrap_err_with(|| format!("mise secrets: cannot write a file for {key}"))?;
            env.insert(key.to_string(), path.to_string_lossy().to_string());
        }
        Ok((files, env))
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &Path> {
        self.paths.iter().map(PathBuf::as_path)
    }
}

impl Drop for TempSecretFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .wrap_err("mise secrets: cannot restrict the secret files directory")
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .wrap_err("mise secrets: cannot create the secret files directory")?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).wrap_err("mise secrets: cannot create the secret files directory")
}

#[cfg(unix)]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Debug lists key names only.
pub(crate) struct SpawnSecrets {
    pub(super) env: BTreeMap<String, SecretValue>,
    /// path-valued entries for `as_file` keys
    pub(super) file_env: BTreeMap<String, String>,
    pub(super) remove: BTreeSet<String>,
    pub(super) files: TempSecretFiles,
    pub(super) names: BTreeSet<String>,
}

impl std::fmt::Debug for SpawnSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnSecrets")
            .field("names", &self.names)
            .field("remove", &self.remove)
            .finish()
    }
}

impl SpawnSecrets {
    /// remove, then set; every set key is also deleted from `env_remove` (M2 put inherited
    /// keys there).
    pub(crate) fn apply(&self, env: &mut EnvMap, env_remove: &mut BTreeSet<String>) {
        for key in &self.remove {
            env.retain(|k, _| !env_key_eq(k, key));
            env_remove.insert(key.clone());
        }
        let set = self
            .env
            .iter()
            .map(|(k, v)| (k, v.expose()))
            .chain(self.file_env.iter().map(|(k, v)| (k, v.as_str())));
        for (key, value) in set {
            env.retain(|k, _| !env_key_eq(k, key));
            env_remove.retain(|k| !env_key_eq(k, key));
            env.insert(key.clone(), value.to_string());
        }
    }

    /// Sorted names this spawn sets, comma-joined.
    pub(crate) fn marker_value(&self) -> String {
        self.names.iter().cloned().collect::<Vec<_>>().join(",")
    }

    pub(crate) fn file_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.paths()
    }

    pub(crate) fn has_values(&self) -> bool {
        !self.names.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn for_test(values: &[(&str, &str)]) -> Self {
        Self::new(
            values
                .iter()
                .map(|(k, v)| (k.to_string(), SecretValue::new(*v)))
                .collect(),
            TempSecretFiles::default(),
            BTreeMap::new(),
            BTreeSet::new(),
        )
    }

    pub(super) fn new(
        env: BTreeMap<String, SecretValue>,
        files: TempSecretFiles,
        file_env: BTreeMap<String, String>,
        remove: BTreeSet<String>,
    ) -> Self {
        let names = env.keys().chain(file_env.keys()).cloned().collect();
        Self {
            env,
            file_env,
            remove,
            files,
            names,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(remove: &[&str]) -> SpawnSecrets {
        SpawnSecrets::new(
            BTreeMap::from([
                ("B".to_string(), SecretValue::new("bv-s3cr3t")),
                ("A".to_string(), SecretValue::new("av-s3cr3t")),
            ]),
            TempSecretFiles::default(),
            BTreeMap::new(),
            remove.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn apply_removes_then_sets_and_clears_env_remove() {
        let s = spawn(&["GONE"]);
        let mut env = EnvMap::from([("GONE".into(), "1".into()), ("A".into(), "old".into())]);
        let mut rm = BTreeSet::from(["A".to_string()]);
        s.apply(&mut env, &mut rm);
        assert_eq!(env.get("A").map(String::as_str), Some("av-s3cr3t"));
        assert!(!env.contains_key("GONE"));
        assert!(rm.contains("GONE") && !rm.contains("A"));
        assert_eq!(s.marker_value(), "A,B");
        assert!(s.has_values());
    }

    #[test]
    fn debug_never_shows_values() {
        let s = spawn(&[]);
        let text = format!("{s:?} {:?}", SecretValue::new("s3cr3t"));
        assert!(!text.contains("s3cr3t"), "{text}");
        assert!(text.contains("A") && text.contains("[redacted]"));
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("secrets");
        let entries = BTreeMap::from([(
            SecretName::new("GCP_SA_JSON").unwrap(),
            SecretValue::new("{\"k\":1}"),
        )]);
        let (files, env) = TempSecretFiles::create(&dir, &entries).unwrap();
        let path = PathBuf::from(&env["GCP_SA_JSON"]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"k\":1}");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        let spawn_dir = path.parent().unwrap().to_path_buf();
        assert_eq!(mode(&spawn_dir), 0o700);
        assert_eq!(mode(&path), 0o600);
        // a second spawn gets a different, unpredictable directory
        let (second, env2) = TempSecretFiles::create(&dir, &entries).unwrap();
        assert_ne!(PathBuf::from(&env2["GCP_SA_JSON"]).parent(), path.parent());
        drop(files);
        assert!(!path.exists());
        assert!(!spawn_dir.exists());
        assert!(PathBuf::from(&env2["GCP_SA_JSON"]).exists());
        drop(second);
        assert!(dir.exists());
    }
}
