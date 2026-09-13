//! Lazy native dependency graphs and per-lockfile sidecar storage.
use super::{AubeLock, UvLock, hash_canonical_toml};
use eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

pub(crate) trait NativeGraph:
    Clone + std::fmt::Debug + Serialize + DeserializeOwned
{
    const GRAPH_FILE: &'static str;
    fn graph_text(&self) -> Result<String>;
    fn files(&self) -> Result<Vec<(&'static str, String)>>;
    fn read(dir: &Path, graph_text: String) -> Result<Self>;
}

#[derive(Clone, Debug)]
pub(crate) enum GraphRef<T> {
    Inline {
        graph: T,
        dir: Option<PathBuf>,
        digest: OnceLock<String>,
    },
    Sidecar {
        dir: PathBuf,
        digest: String,
        cell: OnceLock<Result<T, String>>,
    },
}

impl<T: NativeGraph> From<T> for GraphRef<T> {
    fn from(graph: T) -> Self {
        Self::Inline {
            graph,
            dir: None,
            digest: OnceLock::new(),
        }
    }
}
impl<T: NativeGraph> PartialEq for GraphRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}
impl<T: NativeGraph> Eq for GraphRef<T> {}

pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

impl<T: NativeGraph> GraphRef<T> {
    pub(crate) fn identity(&self) -> String {
        match self {
            Self::Sidecar { digest, .. } => digest.clone(),
            Self::Inline { graph, digest, .. } => digest
                .get_or_init(|| {
                    digest_bytes(
                        graph
                            .graph_text()
                            .expect("native graph serialization")
                            .as_bytes(),
                    )
                })
                .clone(),
        }
    }
    /// Check availability without opening or parsing the native graph.
    pub(crate) fn warn_if_missing(&self) {
        if let Some(dir) = self.dir() {
            let path = dir.join(T::GRAPH_FILE);
            if let Err(error) = std::fs::metadata(&path)
                && error.kind() == std::io::ErrorKind::NotFound
            {
                warn!(
                    "dependency sidecar {} is missing; commit sidecars alongside mise.lock",
                    path.display()
                );
            }
        }
    }

    pub(crate) fn dir(&self) -> Option<&Path> {
        match self {
            Self::Inline { dir, .. } => dir.as_deref(),
            Self::Sidecar { dir, .. } => Some(dir),
        }
    }
    pub(crate) fn keep_path_from(&mut self, old: &Self) {
        if let Self::Inline { dir, .. } = self {
            *dir = old.dir().map(Path::to_path_buf);
        }
    }
    pub(crate) fn load(&self) -> Result<&T> {
        match self {
            Self::Inline { graph, .. } => Ok(graph),
            Self::Sidecar { dir, digest, cell } => cell
                .get_or_init(|| {
                    let text = std::fs::read_to_string(dir.join(T::GRAPH_FILE))
                        .map_err(|e| e.to_string())?;
                    if digest_bytes(text.as_bytes()) != *digest {
                        return Err(
                            "digest mismatch; run `mise lock` to accept the edited graph".into(),
                        );
                    }
                    T::read(dir, text).map_err(|e| e.to_string())
                })
                .as_ref()
                .map_err(|e| {
                    eyre!(
                        "dependency sidecar {}: {e}; run `mise lock` to repair it",
                        dir.display()
                    )
                }),
        }
    }
    /// Read actual bytes when accepting external edits. Keep the recorded directory.
    pub(crate) fn refresh(&self) -> Result<Self> {
        let Self::Sidecar { dir, .. } = self else {
            return Ok(self.clone());
        };
        let text = std::fs::read_to_string(dir.join(T::GRAPH_FILE))
            .map_err(|e| eyre!("dependency sidecar {}: {e}; run `mise lock`", dir.display()))?;
        let graph = T::read(dir, text)
            .map_err(|e| eyre!("dependency sidecar {}: {e}; run `mise lock`", dir.display()))?;
        Ok(Self::Inline {
            graph,
            dir: Some(dir.clone()),
            digest: OnceLock::new(),
        })
    }
    pub(crate) fn resolve_path(&mut self, lockfile: &Path) -> Result<()> {
        if let Self::Sidecar { dir, .. } = self {
            if dir.is_absolute()
                || dir.components().any(|c| !matches!(c, Component::Normal(_)))
                || dir.to_string_lossy().contains('\\')
            {
                bail!("invalid dependency sidecar path {}", dir.display());
            }
            *dir = absolute(lockfile.parent().unwrap_or(Path::new("."))).join(&*dir);
        }
        Ok(())
    }
    pub(crate) fn parse(value: toml::Value) -> Result<Self> {
        if value.get("path").is_some() {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Pointer {
                path: PathBuf,
                digest: String,
            }
            let p: Pointer = value.try_into()?;
            if !p
                .digest
                .strip_prefix("sha256:")
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                bail!("invalid dependency graph digest");
            }
            Ok(Self::Sidecar {
                dir: p.path,
                digest: p.digest,
                cell: OnceLock::new(),
            })
        } else {
            debug!(
                "migrating inline dependency graph to a native sidecar on the next lockfile save"
            );
            Ok(Self::from(value.try_into::<T>()?))
        }
    }
    pub(crate) fn pointer(&self, base: &Path) -> Result<toml::Value> {
        let dir = self
            .dir()
            .ok_or_else(|| eyre!("dependency sidecar has not been prepared"))?;
        let relative = dir.strip_prefix(absolute(base))?;
        let path = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let digest = self.identity();
        Ok(toml::toml! { path = path digest = digest }.into())
    }
}

// Serde is used for legacy inline parsing and internal snapshots. Disk writes
// replace graph fields with prepared pointer values before serializing.
impl<T: NativeGraph> Serialize for GraphRef<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Inline { graph, .. } => graph.serialize(serializer),
            Self::Sidecar { dir, digest, .. } => {
                use serde::ser::SerializeStruct;
                let mut s = serializer.serialize_struct("GraphRef", 2)?;
                s.serialize_field("path", dir)?;
                s.serialize_field("digest", digest)?;
                s.end()
            }
        }
    }
}
impl<'de, T: NativeGraph> Deserialize<'de> for GraphRef<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(toml::Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

pub(crate) fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::env::current_dir().unwrap_or_default().join(path)
    }
}

pub(crate) fn sidecar_root(lockfile: &Path) -> PathBuf {
    let dir = lockfile.parent().unwrap_or(Path::new("."));
    let mut root = match dir.file_name().and_then(|s| s.to_str()) {
        Some(".mise") => dir.join("locks"),
        Some("mise")
            if dir
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|s| s == ".config") =>
        {
            dir.join("locks")
        }
        Some(".config") => dir.join("mise/locks"),
        _ => dir.join(".mise/locks"),
    };
    if lockfile.file_name().is_some_and(|s| s != "mise.lock") {
        root.push(lockfile.file_stem().unwrap_or_default());
    }
    root
}

pub(crate) fn variant_suffix(
    backend: Option<&str>,
    options: &std::collections::BTreeMap<String, String>,
) -> String {
    let backend = backend.unwrap_or("");
    let options = toml::Value::try_from(options).expect("string options");
    let value = toml::toml! { backend = backend options = options };
    let mut hash = Sha256::new();
    hash_canonical_toml(&mut hash, &value.into());
    hex::encode(hash.finalize())[..8].to_owned()
}

impl NativeGraph for UvLock {
    const GRAPH_FILE: &'static str = "uv.lock";
    fn graph_text(&self) -> Result<String> {
        if self.graph_text.is_empty() {
            Ok(toml::to_string(&self.graph)?)
        } else {
            Ok(self.graph_text.clone())
        }
    }
    fn files(&self) -> Result<Vec<(&'static str, String)>> {
        Ok(vec![
            ("uv.lock", self.graph_text()?),
            ("pyproject.toml", toml::to_string(&self.project)?),
        ])
    }
    fn read(dir: &Path, graph_text: String) -> Result<Self> {
        Ok(Self {
            project: std::fs::read_to_string(dir.join("pyproject.toml"))?.parse()?,
            graph: graph_text.parse()?,
            graph_text,
        })
    }
}
impl NativeGraph for AubeLock {
    const GRAPH_FILE: &'static str = "aube-lock.yaml";
    fn graph_text(&self) -> Result<String> {
        self.to_yaml()
    }
    fn files(&self) -> Result<Vec<(&'static str, String)>> {
        let project = if let Some(project) = &self.project {
            project.clone()
        } else {
            let dependencies = self
                .graph
                .get("importers")
                .and_then(|v| v.get("."))
                .and_then(|v| v.get("dependencies"))
                .and_then(toml::Value::as_table)
                .map(|deps| {
                    deps.iter()
                        .filter_map(|(k, v)| {
                            v.get("specifier")
                                .and_then(toml::Value::as_str)
                                .map(|v| (k.clone(), v.to_owned()))
                        })
                        .collect::<std::collections::BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            serde_json::to_string_pretty(
                &serde_json::json!({"name":"mise-npm-install","private":true,"dependencies":dependencies}),
            )?
        };
        Ok(vec![
            ("aube-lock.yaml", self.to_yaml()?),
            ("package.json", project),
        ])
    }
    fn read(dir: &Path, graph_text: String) -> Result<Self> {
        let mut graph = Self::from_yaml(&graph_text)?;
        let project = std::fs::read_to_string(dir.join("package.json"))?;
        serde_json::from_str::<serde_json::Value>(&project)?;
        graph.project = Some(project);
        Ok(graph)
    }
}

#[derive(Default)]
pub(super) struct SidecarWrites {
    pub files: Vec<(PathBuf, String)>,
    pub referenced: std::collections::BTreeSet<PathBuf>,
    pub root: PathBuf,
    pub remove: Vec<PathBuf>,
}
impl SidecarWrites {
    pub(super) fn new(lockfile: &Path) -> Self {
        Self {
            root: absolute(&sidecar_root(lockfile)),
            ..Default::default()
        }
    }
    pub(super) fn reserve<T: NativeGraph>(&mut self, graph: &GraphRef<T>) {
        if let Some(dir) = graph.dir().filter(|dir| dir.starts_with(&self.root)) {
            self.referenced.insert(dir.to_path_buf());
        }
    }
    pub(super) fn prepare<T: NativeGraph>(
        &mut self,
        graph: &GraphRef<T>,
        short: &str,
        version: &str,
        backend: Option<&str>,
        options: &std::collections::BTreeMap<String, String>,
    ) -> Result<GraphRef<T>> {
        if !crate::file::is_plain_file_name(version) {
            bail!("cannot store dependency sidecar for invalid version {version}");
        }
        let dir = if let Some(dir) = graph.dir().filter(|dir| dir.starts_with(&self.root)) {
            dir.to_path_buf()
        } else {
            let parent = self.root.join(crate::backend::tool_directory_name(short));
            let plain = parent.join(version);
            if !options.is_empty() || self.referenced.contains(&plain) {
                parent.join(format!("{version}~{}", variant_suffix(backend, options)))
            } else {
                plain
            }
        };
        self.referenced.insert(dir.clone());
        if matches!(graph, GraphRef::Sidecar { dir: source, .. } if *source == dir && source.is_dir())
        {
            return Ok(graph.clone());
        }
        let body = match graph.load() {
            Ok(body) => body,
            Err(error) if matches!(graph, GraphRef::Sidecar { .. }) => {
                warn!("preserving unavailable dependency sidecar: {error:#}");
                return Ok(graph.clone());
            }
            Err(error) => return Err(error),
        };
        for (name, contents) in body.files()? {
            let target = dir.join(name);
            if std::fs::read(&target).ok().as_deref() != Some(contents.as_bytes()) {
                self.files.push((target, contents));
            }
        }
        Ok(GraphRef::Sidecar {
            dir,
            digest: graph.identity(),
            cell: OnceLock::new(),
        })
    }
    pub(super) fn collect_garbage(&mut self) {
        let Ok(tools) = std::fs::read_dir(&self.root) else {
            return;
        };
        for tool in tools.flatten() {
            if tool.path().is_symlink() {
                continue;
            }
            let Ok(versions) = std::fs::read_dir(tool.path()) else {
                continue;
            };
            for version in versions.flatten() {
                let dir = version.path();
                if dir.is_symlink() || self.referenced.contains(&dir) {
                    continue;
                }
                if dir.join("uv.lock").is_file() || dir.join("aube-lock.yaml").is_file() {
                    self.remove.push(dir);
                }
            }
        }
    }
    pub(super) fn has_changes(&self) -> bool {
        !self.files.is_empty() || !self.remove.is_empty()
    }
    pub(super) fn publish_files(&self) -> Result<()> {
        use std::io::Write;
        for (target, text) in &self.files {
            let parent = target
                .parent()
                .ok_or_else(|| eyre!("invalid sidecar file path"))?;
            for ancestor in parent.ancestors().take_while(|p| p.starts_with(&self.root)) {
                if ancestor.is_symlink() {
                    bail!(
                        "refusing to write dependency sidecar through symlink {}",
                        ancestor.display()
                    );
                }
            }
            std::fs::create_dir_all(parent)?;
            let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
            tmp.write_all(text.as_bytes())?;
            tmp.as_file().sync_all()?;
            tmp.persist(target)?;
        }
        Ok(())
    }
    pub(super) fn prune(&self) -> Result<()> {
        for dir in &self.remove {
            if dir.exists() {
                std::fs::remove_dir_all(dir)?;
            }
            if let Some(parent) = dir.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::{Lockfile, LockfileTool};
    use std::collections::{BTreeMap, BTreeSet};

    fn uv() -> UvLock {
        let graph_text = "version = 1\nrevision = 3\n".to_owned();
        UvLock {
            project: toml::toml! { [project] name = "fixture" },
            graph: graph_text.parse().unwrap(),
            graph_text,
        }
    }
    fn entry(backend: &str) -> LockfileTool {
        LockfileTool {
            version: "1.0.0".into(),
            backend: Some(backend.into()),
            specifiers: BTreeSet::new(),
            options: BTreeMap::new(),
            platforms: BTreeMap::new(),
            uv: None,
            aube: None,
        }
    }
    #[test]
    fn missing_sidecar_is_preserved_without_writes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let graph: GraphRef<UvLock> = GraphRef::Sidecar {
            dir: sidecar_root(&path).join("pypi-fixture/1.0.0"),
            digest: digest_bytes(b"missing"),
            cell: OnceLock::new(),
        };
        let mut writes = SidecarWrites::new(&path);
        let retained = writes
            .prepare(
                &graph,
                "pypi:fixture",
                "1.0.0",
                Some("pypi:fixture"),
                &BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(retained.dir(), graph.dir());
        assert_eq!(retained.identity(), graph.identity());
        assert!(writes.files.is_empty());
        let mut lock = Lockfile::default();
        let mut tool = entry("pypi:fixture");
        tool.uv = Some(graph);
        lock.tools.insert("pypi:fixture".into(), vec![tool]);
        lock.save(&path).unwrap();
        let saved = std::fs::read(&path).unwrap();
        Lockfile::read(&path).unwrap().save(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), saved);
    }

    #[test]
    fn migration_cleanup_preserves_sibling_lockfile_graphs() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("apps/a/mise.lock");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        let target = temp.path().join("mise.lock");
        let mut lock = Lockfile::default();
        let mut tool = entry("pypi:fixture");
        tool.uv = Some(uv().into());
        lock.tools.insert("pypi:fixture".into(), vec![tool]);
        lock.save(&source).unwrap();
        let sibling = source.with_file_name("mise.local.lock");
        lock.save(&sibling).unwrap();
        let sibling_graph = sidecar_root(&sibling).join("pypi-fixture/1.0.0/uv.lock");
        let bytes = std::fs::read(&sibling_graph).unwrap();
        Lockfile::read(&source).unwrap().save(&target).unwrap();
        crate::lockfile::remove_migrated_sidecars(&source, &target).unwrap();
        assert!(!sidecar_root(&source).join("pypi-fixture/1.0.0").exists());
        assert_eq!(std::fs::read(&sibling_graph).unwrap(), bytes);
        let sibling_target = target.with_file_name("mise.local.lock");
        Lockfile::read(&sibling)
            .unwrap()
            .save(&sibling_target)
            .unwrap();
        crate::lockfile::remove_migrated_sidecars(&sibling, &sibling_target).unwrap();
        assert_eq!(
            std::fs::read(sidecar_root(&sibling_target).join("pypi-fixture/1.0.0/uv.lock"))
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn sidecar_layout_follows_config_and_lockfile_name() {
        for (path, expected) in [
            ("project/mise.lock", "project/.mise/locks"),
            ("project/.mise/mise.lock", "project/.mise/locks"),
            (
                "project/.config/mise/mise.lock",
                "project/.config/mise/locks",
            ),
            ("project/.config/mise.lock", "project/.config/mise/locks"),
            (
                "project/.config/mise/mise.local.lock",
                "project/.config/mise/locks/mise.local",
            ),
            (
                "project/mise.test.local.lock",
                "project/.mise/locks/mise.test.local",
            ),
        ] {
            assert_eq!(sidecar_root(Path::new(path)), PathBuf::from(expected));
        }
    }
    #[test]
    fn native_round_trip_is_lazy_and_byte_stable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let mut lock = Lockfile::default();
        let mut py = entry("pypi:fixture");
        py.uv = Some(uv().into());
        let mut npm = entry("npm:fixture");
        npm.aube = Some(
            AubeLock::from_yaml("lockfileVersion: '9.0'\npackages: {}\n")
                .unwrap()
                .into(),
        );
        lock.tools.insert("pipx:fixture".into(), vec![py]);
        lock.tools.insert("npm:fixture".into(), vec![npm]);
        let prepared = lock.prepare_write(&path).unwrap().unwrap();
        assert!(
            !sidecar_root(&path).exists(),
            "preparing a dry run must not write sidecars"
        );
        prepared.publish().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("uv.graph"));
        assert!(!text.contains("aube.graph"));
        assert!(text.contains(".mise/locks/pipx-fixture/1.0.0"));
        assert!(!text.contains('\\'));
        let loaded = Lockfile::read(&path).unwrap();
        let py = loaded.tools["pipx:fixture"][0].uv.as_ref().unwrap();
        let npm = loaded.tools["npm:fixture"][0].aube.as_ref().unwrap();
        assert_eq!(py.load().unwrap(), &uv());
        assert_eq!(
            std::fs::read_to_string(py.dir().unwrap().join("uv.lock")).unwrap(),
            uv().graph_text
        );
        assert!(npm.dir().unwrap().join("package.json").is_file());
        assert_eq!(
            npm.load().unwrap().to_yaml().unwrap(),
            "lockfileVersion: '9.0'\npackages: {}\n"
        );
        assert!(loaded.prepare_write(&path).unwrap().is_none());
        std::fs::remove_dir_all(sidecar_root(&path)).unwrap();
        let missing = Lockfile::read(&path).unwrap();
        let missing = missing.tools["pipx:fixture"][0].uv.as_ref().unwrap();
        assert_eq!(missing.identity(), py.identity());
        assert!(
            missing
                .load()
                .unwrap_err()
                .to_string()
                .contains("mise lock")
        );
    }
    #[test]
    fn variants_are_sticky_and_gc_is_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let local = temp.path().join("mise.local.lock");
        let mut lock = Lockfile::default();
        let mut a = entry("pypi:fixture");
        a.uv = Some(uv().into());
        lock.tools.insert("pypi:fixture".into(), vec![a]);
        lock.save(&path).unwrap();
        lock.save(&local).unwrap();
        let mut lock = Lockfile::read(&path).unwrap();
        let a_dir = lock.tools["pypi:fixture"][0]
            .uv
            .as_ref()
            .unwrap()
            .dir()
            .unwrap()
            .to_owned();
        let before = std::fs::metadata(a_dir.join("uv.lock"))
            .unwrap()
            .modified()
            .unwrap();
        let mut b = entry("pypi:fixture");
        b.options.insert("extras".into(), "feature".into());
        b.uv = Some(uv().into());
        lock.tools.get_mut("pypi:fixture").unwrap().push(b);
        lock.save(&path).unwrap();
        let mut lock = Lockfile::read(&path).unwrap();
        let b_dir = lock.tools["pypi:fixture"][1]
            .uv
            .as_ref()
            .unwrap()
            .dir()
            .unwrap()
            .to_owned();
        assert_ne!(a_dir, b_dir);
        assert!(
            b_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("1.0.0~")
        );
        assert_eq!(
            lock.tools["pypi:fixture"][0].uv.as_ref().unwrap().dir(),
            Some(a_dir.as_path())
        );
        lock.tools.get_mut("pypi:fixture").unwrap().truncate(1);
        lock.save(&path).unwrap();
        assert!(!b_dir.exists());
        assert_eq!(
            std::fs::metadata(a_dir.join("uv.lock"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
        assert!(
            sidecar_root(&local)
                .join("pypi-fixture/1.0.0/uv.lock")
                .exists()
        );
    }
    #[test]
    fn backend_variants_and_inline_migration_get_distinct_paths() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        std::fs::write(
            &path,
            r#"lockfile_version = 2
[[tools.fixture]]
version = "1.0.0"
backend = "pypi:first"
uv = { project = {}, graph = { version = 1 } }
[[tools.fixture]]
version = "1.0.0"
backend = "pypi:second"
uv = { project = {}, graph = { version = 1 } }
"#,
        )
        .unwrap();
        let lock = Lockfile::read(&path).unwrap();
        lock.save(&path).unwrap();
        let loaded = Lockfile::read(&path).unwrap();
        let entries = &loaded.tools["fixture"];
        let a = entries[0].uv.as_ref().unwrap().dir().unwrap();
        let b = entries[1].uv.as_ref().unwrap().dir().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.file_name().unwrap(), "1.0.0");
        assert!(
            b.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("1.0.0~")
        );
        assert!(!std::fs::read_to_string(path).unwrap().contains("graph ="));
    }

    #[test]
    fn changed_digest_can_be_explicitly_refreshed_without_changing_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let mut lock = Lockfile::default();
        let mut tool = entry("pypi:fixture");
        tool.uv = Some(uv().into());
        lock.tools.insert("pypi:fixture".into(), vec![tool]);
        lock.save(&path).unwrap();
        let mut lock = Lockfile::read(&path).unwrap();
        let original = lock.tools["pypi:fixture"][0].uv.as_ref().unwrap();
        let dir = original.dir().unwrap().to_owned();
        std::fs::write(dir.join("uv.lock"), "# edited\nversion = 1\nrevision = 3\n").unwrap();
        assert!(
            original
                .load()
                .unwrap_err()
                .to_string()
                .contains("accept the edited graph")
        );
        let edited = original.refresh().unwrap();
        assert_ne!(edited.identity(), original.identity());
        assert_eq!(edited.load().unwrap(), &uv());
        lock.tools.get_mut("pypi:fixture").unwrap()[0].uv = Some(edited);
        lock.save(&path).unwrap();
        let lock = Lockfile::read(&path).unwrap();
        assert_eq!(
            lock.tools["pypi:fixture"][0].uv.as_ref().unwrap().dir(),
            Some(dir.as_path())
        );
        assert!(
            lock.tools["pypi:fixture"][0]
                .uv
                .as_ref()
                .unwrap()
                .load()
                .is_ok()
        );
    }
}
