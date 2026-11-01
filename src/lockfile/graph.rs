//! Lazy native dependency graphs and per-lockfile sidecar storage.
use super::{AubeLock, UvLock, hash_canonical_toml};
use eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

pub trait NativeGraph: Clone + std::fmt::Debug + Serialize + DeserializeOwned {
    const GRAPH_FILE: &'static str;
    fn graph_text(&self) -> Result<String>;
    fn files(&self) -> Result<Vec<(&'static str, String)>>;
    /// Read the graph in `dir` from `graph_text` and the sidecar's other files.
    /// `base` is the lockfile's directory (see [`read_sidecar_file`]).
    fn read(base: &Path, dir: &Path, graph_text: String) -> Result<Self>;
}

#[derive(Clone, Debug)]
pub enum GraphRef<T> {
    Inline {
        graph: T,
        dir: Option<PathBuf>,
        digest: OnceLock<String>,
    },
    Sidecar {
        dir: PathBuf,
        /// The directory of the lockfile that points at `dir`.
        base: PathBuf,
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

/// Rewrite CRLF line endings to LF.
///
/// Every sidecar file is text that mise itself serializes (`uv.lock`,
/// `pyproject.toml`, `aube-lock.yaml`, `package.json`), so there is no binary
/// content to corrupt here.
pub(crate) fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Digest sidecar text with line endings normalized.
///
/// Git checks out with `core.autocrlf=true` by default on Windows, which
/// rewrites LF to CRLF in the working tree. Hashing the normalized text keeps
/// a committed sidecar verifiable on every platform while still detecting any
/// change to the dependency graph itself.
pub(crate) fn digest_text(text: &str) -> String {
    digest_bytes(normalize_newlines(text).as_bytes())
}

/// Read a file of the sidecar of the lockfile in `base`. A sidecar outside
/// the sidecar root is rewritten below it when the lockfile is saved, so a
/// symlink on the way down from `base` would copy whatever file it leads to
/// into the checkout. This is the read-side counterpart of the check in
/// [`SidecarWrites::publish_files`].
fn read_sidecar_file(base: &Path, path: &Path) -> Result<String> {
    if let Some(link) = symlink_below(base, path) {
        bail!("refusing to read through symlink {}", link.display());
    }
    Ok(std::fs::read_to_string(path)?)
}

impl<T: NativeGraph> GraphRef<T> {
    pub(crate) fn identity(&self) -> String {
        match self {
            Self::Sidecar { digest, .. } => digest.clone(),
            Self::Inline { graph, digest, .. } => digest
                .get_or_init(|| {
                    digest_text(&graph.graph_text().expect("native graph serialization"))
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
            Self::Sidecar {
                dir,
                base,
                digest,
                cell,
            } => cell
                .get_or_init(|| {
                    let raw = read_sidecar_file(base, &dir.join(T::GRAPH_FILE))
                        .map_err(|e| e.to_string())?;
                    let text = normalize_newlines(&raw).into_owned();
                    // Older versions hashed the bytes as they found them, so a
                    // lockfile written from a CRLF checkout records the CRLF
                    // digest. Keep accepting it; the next save records the
                    // normalized one.
                    if digest_bytes(text.as_bytes()) != *digest
                        && digest_bytes(raw.as_bytes()) != *digest
                    {
                        return Err(
                            "digest mismatch; run `mise lock` to accept the edited graph".into(),
                        );
                    }
                    T::read(base, dir, text).map_err(|e| e.to_string())
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
        let Self::Sidecar { dir, base, .. } = self else {
            return Ok(self.clone());
        };
        let text = read_sidecar_file(base, &dir.join(T::GRAPH_FILE))
            .map_err(|e| eyre!("dependency sidecar {}: {e}; run `mise lock`", dir.display()))?;
        let graph = T::read(base, dir, normalize_newlines(&text).into_owned())
            .map_err(|e| eyre!("dependency sidecar {}: {e}; run `mise lock`", dir.display()))?;
        Ok(Self::Inline {
            graph,
            dir: Some(dir.clone()),
            digest: OnceLock::new(),
        })
    }
    pub(crate) fn resolve_path(&mut self, lockfile: &Path) -> Result<()> {
        if let Self::Sidecar { dir, base, .. } = self {
            if dir.is_absolute()
                || dir.components().any(|c| !matches!(c, Component::Normal(_)))
                || dir.to_string_lossy().contains('\\')
            {
                bail!("invalid dependency sidecar path {}", dir.display());
            }
            *base = absolute(lockfile.parent().unwrap_or(Path::new(".")));
            *dir = base.join(&*dir);
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
                // Set with `dir` by `resolve_path`.
                base: PathBuf::new(),
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

/// The first symlink on the way down from `base` (exclusive) to `path`, if
/// any. Everything below a lockfile's directory comes with the working tree,
/// so a link there can lead anywhere on the machine.
pub fn symlink_below<'a>(base: &Path, path: &'a Path) -> Option<&'a Path> {
    path.ancestors()
        .take_while(|p| *p != base && p.starts_with(base))
        .find(|p| p.is_symlink())
}

pub(crate) fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::env::current_dir().unwrap_or_default().join(path)
    }
}

pub fn sidecar_root(lockfile: &Path) -> PathBuf {
    sidecar_root_in(lockfile, &crate::env::MISE_CONFIG_DIR)
}

fn sidecar_root_in(lockfile: &Path, global_config_dir: &Path) -> PathBuf {
    let dir = lockfile.parent().unwrap_or(Path::new("."));
    // Choose the layout from the physical directory, so a symlinked config
    // dir (`~/.config/mise` -> a dotfiles repo) gets the same sidecar location
    // however it is reached. The root itself stays under the given path.
    let physical = dunce::canonicalize(absolute(dir)).ok();
    let layout = physical.as_deref().unwrap_or(dir);
    // The global config dir keeps `locks/` whatever its link target is named.
    let is_global = physical.as_deref().is_some_and(|physical| {
        dunce::canonicalize(absolute(global_config_dir)).is_ok_and(|global| global == physical)
    });
    let mut root = match layout.file_name().and_then(|s| s.to_str()) {
        _ if is_global => dir.join("locks"),
        Some(".mise") => dir.join("locks"),
        Some("mise")
            if layout
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

impl UvLock {
    /// The digest of this graph without the release-age cutoff uv records in
    /// `[options]`, or `None` when the graph carries no cutoff.
    ///
    /// The cutoff is the time `mise lock` ran, so two repos locking the same
    /// tool with the same dependencies record different `exclude-newer` values.
    /// It only constrains resolution, which `uv sync --frozen` does not repeat
    /// on install, so it must not make one installation of the graph differ
    /// from another.
    ///
    /// The cutoff is removed from the text as it is, not from a reserialized
    /// table, so the rest of the graph hashes the bytes it always has: the
    /// result is the [`digest_text`] of the graph uv writes without a cutoff.
    fn identity_without_cutoff(&self) -> Result<Option<String>> {
        Self::identity_of_text(&self.graph_text()?)
    }

    /// [`Self::identity_without_cutoff`] of a graph given as its text.
    fn identity_of_text(text: &str) -> Result<Option<String>> {
        let mut doc = text.parse::<toml_edit::DocumentMut>()?;
        let Some(options) = doc
            .get_mut("options")
            .and_then(toml_edit::Item::as_table_mut)
        else {
            return Ok(None);
        };
        if options.remove("exclude-newer").is_none() {
            return Ok(None);
        }
        if options.is_empty() {
            doc.remove("options");
        } else if options.iter().all(|(_, item)| item.is_table()) {
            // Only sub-tables are left, which uv writes without a `[options]` header.
            options.set_implicit(true);
        }
        Ok(Some(digest_text(&doc.to_string())))
    }
}

impl GraphRef<UvLock> {
    /// What names an installation of this graph: [`Self::identity`], except
    /// that the release-age cutoff in the graph's `[options]` is left out.
    /// A graph without one keeps its [`Self::identity`], so installs made
    /// before this stay where they are.
    pub(crate) fn install_identity(&self) -> String {
        if let Self::Sidecar {
            dir, base, digest, ..
        } = self
            && let Some(identity) = cached_install_identity(base, dir, digest)
        {
            return identity;
        }
        self.load()
            .and_then(UvLock::identity_without_cutoff)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.identity())
    }
}

/// The [`GraphRef::install_identity`] of a sidecar, remembered by the digest of
/// its text. Finding the identity parses the whole graph, which every `hook-env`
/// process would otherwise repeat; the text behind a digest never changes, so
/// the answer is kept in the process and in the cache directory.
///
/// The sidecar is still read and checked against `digest` first, so an edited
/// graph is never answered from the cache. `None` when it cannot be read or does
/// not match, and the caller falls back to parsing it.
fn cached_install_identity(base: &Path, dir: &Path, digest: &str) -> Option<String> {
    static MEMO: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> =
        std::sync::Mutex::new(None);
    let raw = read_sidecar_file(base, &dir.join(UvLock::GRAPH_FILE)).ok()?;
    let text = normalize_newlines(&raw);
    if digest_bytes(text.as_bytes()) != digest && digest_bytes(raw.as_bytes()) != digest {
        return None;
    }
    let key = digest_text(&text);
    let remembered = MEMO
        .lock()
        .ok()
        .and_then(|memo| memo.as_ref()?.get(&key).cloned());
    let cutoff_free = if let Some(identity) = remembered {
        identity
    } else {
        let path = crate::dirs::CACHE
            .join("uv-install-identity")
            // The answer depends on how this mise removes the cutoff, so a
            // new release starts a new set instead of trusting the old one.
            .join(env!("CARGO_PKG_VERSION"))
            .join(key.trim_start_matches("sha256:"));
        let identity = match std::fs::read_to_string(&path) {
            Ok(identity) if identity == NO_CUTOFF || identity.starts_with("sha256:") => identity,
            _ => {
                let identity = UvLock::identity_of_text(&text)
                    .ok()?
                    .unwrap_or_else(|| NO_CUTOFF.to_string());
                if let Err(err) = crate::file::create_dir_all(path.parent().unwrap_or(&path))
                    .and_then(|()| crate::file::write_atomic_unsynced(&path, &identity))
                {
                    debug!("could not cache uv install identity: {err:#}");
                }
                identity
            }
        };
        if let Ok(mut memo) = MEMO.lock() {
            memo.get_or_insert_with(Default::default)
                .insert(key, identity.clone());
        }
        identity
    };
    // A graph without a cutoff keeps its digest, in whichever form its lockfile records it.
    Some(if cutoff_free == NO_CUTOFF {
        digest.to_string()
    } else {
        cutoff_free
    })
}

const NO_CUTOFF: &str = "no-cutoff";

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
    fn read(base: &Path, dir: &Path, graph_text: String) -> Result<Self> {
        Ok(Self {
            project: normalize_newlines(&read_sidecar_file(base, &dir.join("pyproject.toml"))?)
                .parse()?,
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
    fn read(base: &Path, dir: &Path, graph_text: String) -> Result<Self> {
        let mut graph = Self::from_yaml(&graph_text)?;
        let project = read_sidecar_file(base, &dir.join("package.json"))?;
        let project = normalize_newlines(&project).into_owned();
        serde_json::from_str::<serde_json::Value>(&project)?;
        graph.project = Some(project);
        Ok(graph)
    }
}

#[derive(Default)]
pub(super) struct SidecarWrites {
    pub files: Vec<(PathBuf, String)>,
    pub referenced: std::collections::BTreeSet<PathBuf>,
    /// The lockfile's directory. Sidecar writes and removals stay below it.
    pub dir: PathBuf,
    pub root: PathBuf,
    pub remove: Vec<PathBuf>,
}
impl SidecarWrites {
    pub(super) fn new(lockfile: &Path) -> Self {
        Self {
            dir: absolute(lockfile.parent().unwrap_or(Path::new("."))),
            root: absolute(&sidecar_root(lockfile)),
            ..Default::default()
        }
    }
    /// Whether the sidecar root is reached through a symlink, such as a
    /// committed `.mise` or `.mise/locks` link. Such a root is not part of
    /// this project, so nothing below it is collected or removed.
    pub(super) fn root_is_linked(&self) -> bool {
        symlink_below(&self.dir, &self.root).is_some()
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
        if let GraphRef::Sidecar {
            dir: source,
            digest,
            cell,
            ..
        } = graph
            && *source == dir
            && source.is_dir()
        {
            // A legacy CRLF digest is upgraded to the normalized one whenever
            // the graph has already been loaded, so a lockfile written from a
            // Windows checkout heals itself without re-reading the sidecar.
            if let Some(Ok(loaded)) = cell.get() {
                let identity = digest_text(&loaded.graph_text()?);
                if identity != *digest {
                    return Ok(GraphRef::Sidecar {
                        dir,
                        base: self.dir.clone(),
                        digest: identity,
                        cell: cell.clone(),
                    });
                }
            }
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
            let contents = normalize_newlines(&contents).into_owned();
            // Compare normalized text so a CRLF working copy of an unchanged
            // sidecar is left exactly as git checked it out.
            let on_disk = std::fs::read_to_string(&target)
                .ok()
                .map(|text| normalize_newlines(&text).into_owned());
            if on_disk.as_deref() != Some(contents.as_str()) {
                self.files.push((target, contents));
            }
        }
        // Digest what was just published, not what the entry used to claim: a
        // legacy CRLF digest must not survive onto freshly written bytes.
        Ok(GraphRef::Sidecar {
            dir,
            base: self.dir.clone(),
            digest: digest_text(&body.graph_text()?),
            cell: OnceLock::new(),
        })
    }
    pub(super) fn collect_garbage(&mut self) {
        if self.root_is_linked() {
            return;
        }
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
            if let Some(ancestor) = symlink_below(&self.dir, parent) {
                bail!(
                    "refusing to write dependency sidecar through symlink {}",
                    ancestor.display()
                );
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
            base: temp.path().to_path_buf(),
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

    #[cfg(unix)]
    #[test]
    fn global_config_dir_keeps_locks_layout_through_a_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let dotfiles = tmp.path().join("dotfiles/shell");
        let config = tmp.path().join("home/.config-link");
        std::fs::create_dir_all(&dotfiles).unwrap();
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&dotfiles, &config).unwrap();
        for dir in [&config, &dotfiles] {
            assert_eq!(
                sidecar_root_in(&dir.join("mise.lock"), &config),
                dir.join("locks")
            );
        }
        // Not the global dir: the project layout applies.
        let other = tmp.path().join("other");
        assert_eq!(
            sidecar_root_in(&dotfiles.join("mise.lock"), &other),
            dotfiles.join(".mise/locks")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_lockfile_keeps_native_graphs_beside_target() {
        use std::os::unix::fs::symlink;

        for deploy_sidecars in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            // Below the link's directory: a link out of a temp dir, which is
            // neither the global config nor a trusted project, is refused.
            let global = root.join(".config/mise");
            let repo = global.join("repo/mise");
            std::fs::create_dir_all(&repo).unwrap();
            let target = repo.join("mise.lock");
            let link = global.join("mise.lock");
            let mut lock = Lockfile::default();
            let mut py = entry("pypi:fixture");
            py.uv = Some(uv().into());
            let mut npm = entry("npm:fixture");
            npm.aube = Some(
                AubeLock::from_yaml("lockfileVersion: '9.0'\npackages: {}\n")
                    .unwrap()
                    .into(),
            );
            lock.tools.insert("pypi:fixture".into(), vec![py]);
            lock.tools.insert("npm:fixture".into(), vec![npm]);
            lock.save(&target).unwrap();
            symlink("repo/mise/mise.lock", &link).unwrap();
            let files = [
                ".mise/locks/pypi-fixture/1.0.0/uv.lock",
                ".mise/locks/pypi-fixture/1.0.0/pyproject.toml",
                ".mise/locks/npm-fixture/1.0.0/aube-lock.yaml",
                ".mise/locks/npm-fixture/1.0.0/package.json",
            ];
            if deploy_sidecars {
                // symlink-each: all directories are real, only files are links.
                for file in files {
                    let deployed = global.join(file);
                    std::fs::create_dir_all(deployed.parent().unwrap()).unwrap();
                    symlink(repo.join(file), deployed).unwrap();
                }
            }
            let before = std::fs::read_to_string(&target).unwrap();
            let mut loaded = Lockfile::read(&link).unwrap();
            let py = loaded.tools["pypi:fixture"][0].uv.as_ref().unwrap();
            assert_eq!(py.load().unwrap(), &uv());
            let npm = loaded.tools["npm:fixture"][0].aube.as_ref().unwrap();
            assert_eq!(
                npm.load().unwrap().to_yaml().unwrap(),
                "lockfileVersion: '9.0'\npackages: {}\n"
            );
            loaded.save(&link).unwrap();
            assert!(link.is_symlink());
            assert_eq!(std::fs::read_to_string(&target).unwrap(), before);
            assert!(!global.join("locks").exists());

            // Newly generated graphs must also live in the repository, and a
            // subsequent read through the lockfile link must find them without
            // requiring another deployment of the individual sidecar files.
            for versions in loaded.tools.values_mut() {
                let mut next = versions[0].clone();
                next.version = "2.0.0".into();
                if let Some(graph) = &next.uv {
                    next.uv = Some(graph.load().unwrap().clone().into());
                }
                if let Some(graph) = &next.aube {
                    next.aube = Some(graph.load().unwrap().clone().into());
                }
                versions.push(next);
            }
            loaded.save(&link).unwrap();
            assert!(link.is_symlink());
            assert!(!global.join("locks").exists());
            for path in [&link, &target] {
                let loaded = Lockfile::read(path).unwrap();
                let py = loaded.tools["pypi:fixture"][1].uv.as_ref().unwrap();
                let npm = loaded.tools["npm:fixture"][1].aube.as_ref().unwrap();
                assert_eq!(py.load().unwrap(), &uv());
                assert!(npm.load().is_ok());
                assert!(py.dir().unwrap().starts_with(&repo));
                assert!(npm.dir().unwrap().starts_with(&repo));
                assert!(loaded.prepare_write(path).unwrap().is_none());
            }
            if deploy_sidecars {
                for file in files {
                    assert!(global.join(file).is_symlink());
                    assert!(global.join(file).is_file());
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_an_unvouched_directory_is_not_written_through() {
        // A checkout can commit mise.lock as a link to any file of the user.
        // Nothing vouches for a link out of a temp dir: it is neither the
        // global lockfile nor one a trusted config keeps.
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("home/config.toml");
        let link = temp.path().join("checkout/mise.lock");
        for path in [&target, &link] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&target, "[settings]\nparanoid = true\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut lock = Lockfile::read(&link).unwrap();
        let mut tool = entry("pypi:fixture");
        tool.uv = Some(uv().into());
        lock.tools.insert("pypi:fixture".into(), vec![tool]);
        let error = lock.save(&link).unwrap_err();
        assert!(error.to_string().contains("refusing to write lockfile"));
        assert!(link.is_symlink());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "[settings]\nparanoid = true\n"
        );
        assert!(!sidecar_root(&target).exists());
    }

    #[cfg(unix)]
    #[test]
    fn changed_lockfile_symlink_aborts_before_publication() {
        use std::os::unix::fs::symlink;

        for replacement in [Some("b/mise.lock"), Some("missing.lock"), None] {
            for sidecar_only in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let a = temp.path().join("a/mise.lock");
                let b = temp.path().join("b/mise.lock");
                let link = temp.path().join("mise.lock");
                let mut lock = Lockfile::default();
                let mut tool = entry("pypi:fixture");
                tool.uv = Some(uv().into());
                lock.tools.insert("pypi:fixture".into(), vec![tool]);
                for path in [&a, &b] {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    lock.save(path).unwrap();
                }
                symlink("a/mise.lock", &link).unwrap();

                if sidecar_only {
                    // Repairing graph bytes can publish sidecars even when the
                    // serialized lockfile is unchanged (PreparedWrite.tmp=None).
                    std::fs::write(
                        sidecar_root(&a).join("pypi-fixture/1.0.0/uv.lock"),
                        "needs repair\n",
                    )
                    .unwrap();
                } else {
                    lock.tools.get_mut("pypi:fixture").unwrap()[0].version = "2.0.0".into();
                }
                let mut before = Vec::new();
                for path in [&a, &b] {
                    for file in [
                        "mise.lock",
                        ".mise/locks/pypi-fixture/1.0.0/uv.lock",
                        ".mise/locks/pypi-fixture/1.0.0/pyproject.toml",
                    ] {
                        let path = path.parent().unwrap().join(file);
                        before.push((path.clone(), std::fs::read(path).unwrap()));
                    }
                }
                let prepared = lock.prepare_write(&link).unwrap().unwrap();
                assert_eq!(prepared.tmp.is_none(), sidecar_only);
                std::fs::remove_file(&link).unwrap();
                if let Some(replacement) = replacement {
                    symlink(replacement, &link).unwrap();
                }

                let error = prepared.publish().unwrap_err();
                assert!(error.to_string().contains("changed during preparation"));
                for (path, bytes) in before {
                    assert_eq!(std::fs::read(path).unwrap(), bytes);
                }
                for path in [&a, &b] {
                    assert!(!sidecar_root(path).join("pypi-fixture/2.0.0").exists());
                }
                assert_eq!(
                    std::fs::read_link(&link).ok(),
                    replacement.map(PathBuf::from)
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn writes_still_reject_symlinked_sidecar_directories() {
        for ancestor in [
            ".mise",
            ".mise/locks",
            ".mise/locks/pypi-fixture",
            ".mise/locks/pypi-fixture/1.0.0",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("mise.lock");
            let outside = temp.path().join("outside");
            std::fs::create_dir(&outside).unwrap();
            let ancestor = temp.path().join(ancestor);
            std::fs::create_dir_all(ancestor.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&outside, ancestor).unwrap();
            let mut lock = Lockfile::default();
            let mut tool = entry("pypi:fixture");
            tool.uv = Some(uv().into());
            lock.tools.insert("pypi:fixture".into(), vec![tool]);
            let error = lock.save(&path).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("refusing to write dependency sidecar through symlink")
            );
            assert!(!path.exists());
            assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_follows_symlinked_sidecar_roots() {
        // A checkout can commit `.mise` or `.mise/locks` as a link to a
        // directory of other projects, each holding a `uv.lock` two levels
        // below it. Neither saving nor migrating a lockfile may remove them.
        for (name, link, unrelated) in [
            ("mise.lock", ".mise", "locks/acme/payments"),
            ("mise.lock", ".mise/locks", "acme/payments"),
            ("mise.local.lock", ".mise/locks", "mise.local/acme/payments"),
            ("mise.local.lock", ".mise/locks/mise.local", "acme/payments"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("apps/a").join(name);
            let target = temp.path().join(name);
            let outside = temp.path().join("outside");
            let unrelated = outside.join(unrelated).join("uv.lock");
            std::fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
            std::fs::write(&unrelated, "version = 1\n").unwrap();
            let link = path.parent().unwrap().join(link);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let lock = Lockfile::default();
            lock.save(&path).unwrap();
            assert!(unrelated.is_file(), "{name} {link:?}");
            lock.save(&target).unwrap();
            crate::lockfile::remove_migrated_sidecars(&path, &target).unwrap();
            assert!(unrelated.is_file(), "{name} {link:?}");
            assert!(link.is_symlink());
        }
    }

    #[cfg(unix)]
    #[test]
    fn reads_reject_symlinked_sidecars() {
        // A sidecar outside the sidecar root is rewritten below it on save, so
        // a linked sidecar file or directory would copy a file of the user's
        // into the checkout.
        let files = [
            ("graphs/pypi/uv.lock", "version = 1\n"),
            ("graphs/pypi/pyproject.toml", "token = \"secret\"\n"),
            ("graphs/npm/aube-lock.yaml", "lockfileVersion: '9.0'\n"),
            ("graphs/npm/package.json", "{\"token\":\"secret\"}\n"),
        ];
        for link in ["graphs", "graphs/pypi", "graphs/npm"]
            .into_iter()
            .chain(files.iter().map(|(file, _)| *file))
        {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("checkout/mise.lock");
            let checkout = path.parent().unwrap();
            for (file, text) in files {
                // Only what `link` leads to lives outside the checkout.
                let dir = if file.starts_with(link) {
                    "home"
                } else {
                    "checkout"
                };
                let file = temp.path().join(dir).join(file);
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, text).unwrap();
            }
            std::fs::create_dir_all(checkout.join(link).parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(temp.path().join("home").join(link), checkout.join(link))
                .unwrap();
            std::fs::write(
                &path,
                format!(
                    r#"lockfile_version = 2
[[tools."pypi:fixture"]]
version = "1.0.0"
backend = "pypi:fixture"
uv = {{ path = "graphs/pypi", digest = "{}" }}
[[tools."npm:fixture"]]
version = "1.0.0"
backend = "npm:fixture"
aube = {{ path = "graphs/npm", digest = "{}" }}
"#,
                    digest_text(files[0].1),
                    digest_text(files[2].1)
                ),
            )
            .unwrap();
            let lock = Lockfile::read(&path).unwrap();
            let py = lock.tools["pypi:fixture"][0].uv.as_ref().unwrap();
            let npm = lock.tools["npm:fixture"][0].aube.as_ref().unwrap();
            let errors = [
                ("pypi", [py.load().err(), py.refresh().err()]),
                ("npm", [npm.load().err(), npm.refresh().err()]),
            ];
            lock.save(&path).unwrap();
            let saved = std::fs::read_to_string(&path).unwrap();
            for (kind, errors) in errors {
                let dir = format!("graphs/{kind}");
                let linked = dir.starts_with(link) || link.starts_with(&dir);
                for error in errors {
                    assert_eq!(error.is_some(), linked, "{link}");
                    if let Some(error) = error {
                        assert!(
                            error
                                .to_string()
                                .contains("refusing to read through symlink")
                        );
                    }
                }
                // A graph that cannot be read stays where it is, and nothing
                // it leads to is copied into the checkout.
                assert_eq!(saved.contains(&format!("path = \"{dir}\"")), linked);
                let moved = sidecar_root(&path).join(format!("{kind}-fixture"));
                assert_eq!(moved.exists(), !linked, "{link}");
            }
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
    fn uv_with_cutoff(cutoff: Option<&str>) -> UvLock {
        let options = cutoff
            .map(|cutoff| format!("\n[options]\nexclude-newer = \"{cutoff}\"\n"))
            .unwrap_or_default();
        let graph_text = format!("version = 1\nrevision = 3\n{options}");
        UvLock {
            project: toml::toml! { [project] name = "fixture" },
            graph: graph_text.parse().unwrap(),
            graph_text,
        }
    }
    #[test]
    fn uv_install_identity_ignores_the_release_age_cutoff() {
        let monday = GraphRef::from(uv_with_cutoff(Some("2026-10-26T09:00:00Z")));
        let tuesday = GraphRef::from(uv_with_cutoff(Some("2026-10-27T17:30:00Z")));
        // The lockfile still records each graph as it was written ...
        assert_ne!(monday.identity(), tuesday.identity());
        // ... but the same dependencies are one installation.
        assert_eq!(monday.install_identity(), tuesday.install_identity());
    }
    #[test]
    fn uv_install_identity_keeps_graphs_without_a_cutoff_where_they_were() {
        let graph = GraphRef::from(uv());
        assert_eq!(graph.install_identity(), graph.identity());
        // Another resolver option is not the cutoff.
        let mut other = uv();
        other.graph.insert(
            "options".into(),
            toml::toml! { prerelease-mode = "allow" }.into(),
        );
        other.graph_text.clear();
        let other = GraphRef::from(other);
        assert_eq!(other.install_identity(), other.identity());
    }
    #[test]
    fn uv_install_identity_still_tells_different_graphs_apart() {
        let cutoff = "2026-10-26T09:00:00Z";
        let before = GraphRef::from(uv_with_cutoff(Some(cutoff)));
        let mut changed = uv_with_cutoff(Some(cutoff));
        changed
            .graph
            .insert("requires-python".into(), ">=3.10".into());
        changed.graph_text.clear();
        assert_ne!(
            before.install_identity(),
            GraphRef::from(changed).install_identity()
        );
        // An option that stays in the graph still counts.
        let mut pre = uv_with_cutoff(Some(cutoff));
        pre.graph["options"]
            .as_table_mut()
            .unwrap()
            .insert("prerelease-mode".into(), "allow".into());
        pre.graph_text.clear();
        assert_ne!(
            before.install_identity(),
            GraphRef::from(pre).install_identity()
        );
    }
    #[test]
    fn uv_install_identity_is_the_identity_of_the_graph_uv_writes_without_a_cutoff() {
        // `uv lock` output, with and without `--exclude-newer` (and an exempt package).
        let package = r#"[[package]]
name = "six"
version = "1.17.0"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/six-1.17.0.tar.gz", hash = "sha256:ff70", size = 34031, upload-time = "2024-12-04T17:35:28.174Z" }
wheels = [
    { url = "https://files.pythonhosted.org/six-1.17.0-py2.py3-none-any.whl", hash = "sha256:4721", size = 11050, upload-time = "2024-12-04T17:35:26.475Z" },
]
"#;
        let header = "version = 1\nrevision = 5\nrequires-python = \">=3.9\"\n";
        let graph = |options: &str| {
            let text = format!("{header}{options}\n{package}");
            GraphRef::from(UvLock {
                project: toml::Table::new(),
                graph: text.parse().unwrap(),
                graph_text: text,
            })
        };
        let plain = graph("");
        let cutoff = graph("\n[options]\nexclude-newer = \"2026-10-27T17:30:00Z\"\n");
        assert_eq!(cutoff.install_identity(), plain.identity());
        assert_eq!(plain.install_identity(), plain.identity());
        let exempt = "\n[options.exclude-newer-package]\nsix = false\n";
        assert_eq!(
            graph(&format!(
                "\n[options]\nexclude-newer = \"2026-10-27T17:30:00Z\"\n{exempt}"
            ))
            .install_identity(),
            graph(exempt).identity()
        );
    }
    #[test]
    fn uv_install_identity_of_a_sidecar_is_saved_to_the_cache() {
        let temp = tempfile::tempdir().unwrap();
        // A graph no other test or run has seen, so only this lookup can write its entry.
        let nonce = temp.path().display();
        let text = format!(
            "version = 1\n# {nonce}\n[options]\nexclude-newer = \"2026-10-26T09:00:00Z\"\n"
        );
        let dir = temp.path().join("locks/pypi-fixture");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("uv.lock"), &text).unwrap();
        let sidecar: GraphRef<UvLock> = GraphRef::Sidecar {
            dir,
            base: temp.path().to_path_buf(),
            digest: digest_text(&text),
            cell: OnceLock::new(),
        };
        let identity = sidecar.install_identity();
        assert_ne!(identity, sidecar.identity());
        let saved = crate::dirs::CACHE
            .join("uv-install-identity")
            .join(env!("CARGO_PKG_VERSION"))
            .join(digest_text(&text).trim_start_matches("sha256:"));
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), identity);
        std::fs::remove_file(saved).unwrap();
    }
    #[test]
    fn uv_install_identity_is_the_same_for_a_sidecar_graph() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let identities = ["2026-10-26T09:00:00Z", "2026-10-27T17:30:00Z"].map(|cutoff| {
            let dir =
                sidecar_root(&path).join(format!("pypi-fixture/{}", cutoff.replace(':', "-")));
            let graph = uv_with_cutoff(Some(cutoff));
            for (name, text) in graph.files().unwrap() {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(name), text).unwrap();
            }
            let sidecar: GraphRef<UvLock> = GraphRef::Sidecar {
                dir,
                base: temp.path().to_path_buf(),
                digest: digest_text(&graph.graph_text),
                cell: OnceLock::new(),
            };
            (sidecar.identity(), sidecar.install_identity())
        });
        assert_ne!(identities[0].0, identities[1].0);
        assert_eq!(identities[0].1, identities[1].1);
        // The answer is remembered by the digest, so a second look agrees.
        let again = sidecar_root(&path).join("pypi-fixture/2026-10-26T09-00-00Z");
        let graph = uv_with_cutoff(Some("2026-10-26T09:00:00Z"));
        let sidecar: GraphRef<UvLock> = GraphRef::Sidecar {
            dir: again.clone(),
            base: temp.path().to_path_buf(),
            digest: digest_text(&graph.graph_text),
            cell: OnceLock::new(),
        };
        assert_eq!(sidecar.install_identity(), identities[0].1);
        // An edited sidecar is not answered from what was remembered.
        std::fs::write(again.join("uv.lock"), "version = 1\n").unwrap();
        assert_eq!(sidecar.install_identity(), sidecar.identity());
        assert_eq!(
            identities[0].1,
            GraphRef::from(uv_with_cutoff(Some("2030-01-01T00:00:00Z"))).install_identity()
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
    fn crlf_checkout_of_sidecars_still_verifies() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let mut lock = Lockfile::default();
        let mut py = entry("pypi:fixture");
        py.uv = Some(uv().into());
        let mut npm = entry("npm:fixture");
        let aube = AubeLock::from_yaml("lockfileVersion: '9.0'\npackages: {}\n").unwrap();
        npm.aube = Some(aube.clone().into());
        lock.tools.insert("pypi:fixture".into(), vec![py]);
        lock.tools.insert("npm:fixture".into(), vec![npm]);
        lock.save(&path).unwrap();
        let recorded = std::fs::read(&path).unwrap();

        // Simulate git's `core.autocrlf=true` checkout on Windows.
        let mut crlf = BTreeMap::new();
        for dir in ["pypi-fixture/1.0.0", "npm-fixture/1.0.0"] {
            for file in std::fs::read_dir(sidecar_root(&path).join(dir)).unwrap() {
                let file = file.unwrap().path();
                let text = std::fs::read_to_string(&file).unwrap();
                assert!(!text.contains("\r\n"), "mise writes LF: {}", file.display());
                let converted = text.replace('\n', "\r\n");
                std::fs::write(&file, &converted).unwrap();
                crlf.insert(file, converted);
            }
        }

        let loaded = Lockfile::read(&path).unwrap();
        let py = loaded.tools["pypi:fixture"][0].uv.as_ref().unwrap();
        assert_eq!(py.load().unwrap(), &uv());
        let npm = loaded.tools["npm:fixture"][0].aube.as_ref().unwrap();
        assert_eq!(
            npm.load().unwrap().to_yaml().unwrap(),
            "lockfileVersion: '9.0'\npackages: {}\n"
        );
        // Line endings do not change the recorded identity.
        assert_eq!(npm.identity(), GraphRef::from(aube).identity());

        // Re-locking keeps the lockfile stable and leaves the CRLF bytes alone.
        let mut regenerated = loaded.clone();
        loaded.save(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), recorded);

        // Re-resolving the same graphs must not rewrite the working copy either.
        for versions in regenerated.tools.values_mut() {
            if let Some(graph) = &versions[0].uv {
                versions[0].uv = Some(graph.load().unwrap().clone().into());
            }
            if let Some(graph) = &versions[0].aube {
                versions[0].aube = Some(graph.load().unwrap().clone().into());
            }
        }
        regenerated.save(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), recorded);
        for (file, contents) in crlf {
            assert_eq!(std::fs::read_to_string(&file).unwrap(), contents);
        }
    }

    #[test]
    fn legacy_crlf_digests_keep_verifying_and_heal_on_save() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.lock");
        let mut lock = Lockfile::default();
        let mut tool = entry("pypi:fixture");
        tool.uv = Some(uv().into());
        lock.tools.insert("pypi:fixture".into(), vec![tool]);
        lock.save(&path).unwrap();

        // An older mise on a Windows checkout accepted the CRLF working copy as
        // an edit and recorded the digest of those raw bytes.
        let graph_file = sidecar_root(&path).join("pypi-fixture/1.0.0/uv.lock");
        let crlf = std::fs::read_to_string(&graph_file)
            .unwrap()
            .replace('\n', "\r\n");
        std::fs::write(&graph_file, &crlf).unwrap();
        let normalized = digest_text(&crlf);
        let legacy = digest_bytes(crlf.as_bytes());
        assert_ne!(legacy, normalized);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace(&normalized, &legacy)).unwrap();

        // Upgrading to a mise that hashes normalized text must not invalidate it.
        let lock = Lockfile::read(&path).unwrap();
        let graph = lock.tools["pypi:fixture"][0].uv.as_ref().unwrap();
        assert_eq!(graph.identity(), legacy);
        assert_eq!(graph.load().unwrap(), &uv());

        // Saving after the graph was loaded records the normalized digest, and
        // the sidecar bytes stay as git checked them out.
        lock.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains(&normalized), "{saved}");
        assert!(!saved.contains(&legacy), "{saved}");
        assert_eq!(std::fs::read_to_string(&graph_file).unwrap(), crlf);
        let lock = Lockfile::read(&path).unwrap();
        assert_eq!(
            lock.tools["pypi:fixture"][0]
                .uv
                .as_ref()
                .unwrap()
                .load()
                .unwrap(),
            &uv()
        );

        // A graph that was never opened keeps its recorded digest untouched.
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace(&normalized, &legacy)).unwrap();
        Lockfile::read(&path).unwrap().save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains(&legacy));

        // Republishing the sidecar elsewhere writes normalized bytes, so the
        // new entry must pin those and not the digest it came in with.
        let moved = temp.path().join("moved/mise.lock");
        std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
        Lockfile::read(&path).unwrap().save(&moved).unwrap();
        let saved = std::fs::read_to_string(&moved).unwrap();
        assert!(saved.contains(&normalized), "{saved}");
        assert!(!saved.contains(&legacy), "{saved}");
        let moved_graph = sidecar_root(&moved).join("pypi-fixture/1.0.0/uv.lock");
        assert!(!std::fs::read_to_string(moved_graph).unwrap().contains('\r'));
        assert_eq!(
            Lockfile::read(&moved).unwrap().tools["pypi:fixture"][0]
                .uv
                .as_ref()
                .unwrap()
                .load()
                .unwrap(),
            &uv()
        );
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
