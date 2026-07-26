use crate::config::{Config, Settings};
use crate::dirs;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::hash;
use crate::task::task_source_checker::{task_cache_inputs, task_cwd};
use crate::task::{RunEntry, Task};
use crate::toolset::Toolset;
use eyre::{Context, Result, bail};
use glob::glob;
use jdx_tar::{Builder, EntryType, Header};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const CACHE_FORMAT_VERSION: u8 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskCacheConfig {
    pub enabled: bool,
    /// Ambient environment variables whose resolved values affect the cache key.
    pub env: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CacheKeyMaterial<'a> {
    format: u8,
    task: &'a str,
    run: &'a [RunEntry],
    args: &'a [String],
    shell: &'a Option<String>,
    outputs: Vec<String>,
    root: PathBuf,
    source_hash: String,
    environment: BTreeMap<String, Option<String>>,
    vars: BTreeMap<String, String>,
    tools: Vec<String>,
    os: &'static str,
    arch: &'static str,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheManifest {
    format: u8,
    key: String,
    roots: Vec<PathBuf>,
}

pub struct TaskArtifactCache {
    root: PathBuf,
    key: String,
    state_path: PathBuf,
}

impl TaskArtifactCache {
    pub async fn new(
        task: &Task,
        config: &Arc<Config>,
        toolset: &Toolset,
        resolved_env: &BTreeMap<String, String>,
        declared_env: &[(String, String)],
    ) -> Result<Option<Self>> {
        Settings::get().ensure_experimental("task artifact caching")?;
        let root = task_cwd(task, config).await?;
        validate_config(task, &root)?;
        let output_roots = resolve_output_roots(task, &root, false)?;
        for output in &output_roots {
            ensure_no_symlink_ancestors(&root, output)?;
        }
        let Some(inputs) = task_cache_inputs(task, config).await? else {
            warn!(
                "task {} has sources defined but no matching files found; artifact caching disabled",
                task.name
            );
            return Ok(None);
        };
        for source in &inputs.source_paths {
            if output_roots
                .iter()
                .any(|output| source == output || source.starts_with(output))
            {
                bail!(
                    "task {} cache outputs must not contain source {}",
                    task.name,
                    source.display()
                );
            }
        }

        let mut environment = declared_env
            .iter()
            .map(|(key, value)| (key.clone(), Some(value.clone())))
            .collect::<BTreeMap<_, _>>();
        let cache_config = task.cache.as_ref().expect("cache must be configured");
        for key in &cache_config.env {
            environment.insert(key.clone(), resolved_env.get(key).cloned());
        }
        let vars = task
            .tera_ctx(config)
            .await?
            .get("vars")
            .map(|value| serde::Deserialize::deserialize(value.clone()))
            .transpose()?
            .unwrap_or_default();
        let mut tools = toolset
            .list_current_versions()
            .into_iter()
            .map(|(_, tv)| tv.to_string())
            .collect::<Vec<_>>();
        tools.sort();

        let material = CacheKeyMaterial {
            format: CACHE_FORMAT_VERSION,
            task: &task.name,
            run: task.run(),
            args: &task.args,
            shell: &task.shell,
            outputs: task.outputs.patterns(),
            root: inputs.root_identity,
            source_hash: inputs.source_hash,
            environment,
            vars,
            tools,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        };
        let encoded = serde_json::to_vec(&material)?;
        let key = hash::hash_blake3_to_str(std::str::from_utf8(&encoded)?);
        let state_identity = hash::hash_blake3_to_str(&format!(
            "{}\0{}\0{}",
            root.display(),
            task.name,
            task.config_source.display()
        ));
        let state_path = dirs::STATE
            .join("task-artifacts")
            .join(format!("{state_identity}.key"));
        Ok(Some(Self {
            root,
            key,
            state_path,
        }))
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn is_current(&self) -> bool {
        file::read_to_string(&self.state_path).is_ok_and(|key| key.trim() == self.key)
    }

    pub fn mark_current(&self) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write(&self.state_path, &self.key)
    }

    pub fn restore(&self, task: &Task) -> Result<bool> {
        let (archive_path, manifest_path) = self.paths();
        if !archive_path.is_file() || !manifest_path.is_file() {
            return Ok(false);
        }
        // Serialize restores targeting the same working directory across mise
        // processes so ancestor validation and the following renames are one
        // cooperative critical section.
        let _output_lock =
            crate::lock_file::LockFile::new(&self.root.join(".mise-task-artifact-cache-output"))
                .lock()?;

        let restore = || -> Result<()> {
            let manifest: CacheManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
            if manifest.format != CACHE_FORMAT_VERSION || manifest.key != self.key {
                bail!("task cache manifest does not match cache key");
            }
            for root in &manifest.roots {
                ensure_safe_relative(root)?;
            }
            if remove_nested_roots(manifest.roots.clone()) != manifest.roots {
                bail!("task cache manifest contains duplicate or nested roots");
            }

            let staging = tempfile::Builder::new()
                .prefix(".mise-task-cache-")
                .tempdir_in(&self.root)?;
            file::untar(
                &archive_path,
                staging.path(),
                ExtractionFormat::TarZst,
                &ExtractOptions {
                    preserve_mtime: false,
                    ..Default::default()
                },
            )?;
            for rel in &manifest.roots {
                let restored = staging.path().join(rel);
                if !restored.exists() && fs::symlink_metadata(&restored).is_err() {
                    bail!("task cache archive is missing {}", rel.display());
                }
                ensure_no_symlink_ancestors(staging.path(), rel)?;
                ensure_no_symlink_ancestors(&self.root, rel)?;
            }

            let mut remove = resolve_output_roots(task, &self.root, false)?;
            remove.extend(manifest.roots.iter().cloned());
            let remove = remove_nested_roots(remove);
            for rel in &remove {
                ensure_no_symlink_ancestors(&self.root, rel)?;
            }
            install_transactionally(&self.root, staging.path(), &manifest.roots, &remove)?;
            if let Err(err) = file::touch_file(&archive_path) {
                warn!("failed to update task cache archive access time: {err}");
            }
            if let Err(err) = file::touch_file(&manifest_path) {
                warn!("failed to update task cache manifest access time: {err}");
            }
            Ok(())
        };

        match restore() {
            Ok(()) => Ok(true),
            Err(err) => {
                warn!("ignoring corrupt task cache entry {}: {err}", self.key);
                let _ = file::remove_file(&archive_path);
                let _ = file::remove_file(&manifest_path);
                Ok(false)
            }
        }
    }

    pub fn store(&self, task: &Task) -> Result<()> {
        let roots = resolve_output_roots(task, &self.root, true)?;
        let roots = remove_nested_roots(roots);
        if roots.is_empty() {
            bail!("task {} produced no cacheable outputs", task.name);
        }
        for root in &roots {
            ensure_no_symlink_ancestors(&self.root, root)?;
        }

        let cache_dir = dirs::CACHE.join("task-artifacts").join("v1");
        file::create_dir_all(&cache_dir)?;
        let (archive_path, manifest_path) = self.paths();
        let nonce = crate::rand::random_string(8);
        let archive_partial = cache_dir.join(format!("{}.part-{nonce}.tar.zst", self.key));
        let manifest_partial = cache_dir.join(format!("{}.part-{nonce}.json", self.key));

        write_archive(&archive_partial, &self.root, &roots)?;
        let manifest = CacheManifest {
            format: CACHE_FORMAT_VERSION,
            key: self.key.clone(),
            roots,
        };
        fs::write(&manifest_partial, serde_json::to_vec(&manifest)?)?;
        file::rename(&archive_partial, &archive_path)?;
        file::rename(&manifest_partial, &manifest_path)?;
        Ok(())
    }

    fn paths(&self) -> (PathBuf, PathBuf) {
        let base = dirs::CACHE.join("task-artifacts").join("v1");
        (
            base.join(format!("{}.tar.zst", self.key)),
            base.join(format!("{}.json", self.key)),
        )
    }
}

fn validate_config(task: &Task, root: &Path) -> Result<()> {
    if task.sources.is_empty() {
        bail!("task {} cache requires at least one source", task.name);
    }
    if task.outputs.is_auto() || task.outputs.patterns().is_empty() {
        bail!(
            "task {} cache requires explicit output files or directories",
            task.name
        );
    }
    for output in task.outputs.patterns() {
        let path = Path::new(&output);
        ensure_safe_relative(path).wrap_err_with(|| {
            format!(
                "task {} cache output must stay within {}: {output}",
                task.name,
                root.display()
            )
        })?;
    }
    Ok(())
}

fn ensure_safe_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("path must be a non-empty relative path");
    }
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("path must not escape the task directory");
    }
    if !path.components().any(|c| matches!(c, Component::Normal(_))) {
        bail!("path must identify an output beneath the task directory");
    }
    Ok(())
}

fn resolve_output_roots(task: &Task, root: &Path, require_matches: bool) -> Result<Vec<PathBuf>> {
    let mut resolved = BTreeSet::new();
    for output in task.outputs.patterns() {
        ensure_safe_relative(Path::new(&output))?;
        if crate::task::task_source_checker::is_glob_pattern(&output) {
            let mut matched = false;
            for entry in glob(root.join(&output).to_str().unwrap_or_default())? {
                let path = entry?;
                let rel = path.strip_prefix(root)?.to_path_buf();
                ensure_safe_relative(&rel)?;
                resolved.insert(rel);
                matched = true;
            }
            if require_matches && !matched {
                bail!("output pattern {output:?} matched no files");
            }
        } else {
            let rel = PathBuf::from(&output);
            if require_matches
                && !root.join(&rel).exists()
                && fs::symlink_metadata(root.join(&rel)).is_err()
            {
                bail!("output {} does not exist", rel.display());
            }
            resolved.insert(rel);
        }
    }
    Ok(resolved.into_iter().collect())
}

fn remove_nested_roots(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort_by_key(|path| path.components().count());
    let mut result = Vec::<PathBuf>::new();
    for root in roots {
        if !result.iter().any(|parent| root.starts_with(parent)) {
            result.push(root);
        }
    }
    result.sort();
    result
}

fn ensure_no_symlink_ancestors(root: &Path, rel: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    let component_count = rel.components().count();
    for component in rel.components().take(component_count.saturating_sub(1)) {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "output path {} traverses symlink ancestor {}",
                    rel.display(),
                    current.display()
                );
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn install_transactionally(
    root: &Path,
    staging: &Path,
    install_roots: &[PathBuf],
    remove_roots: &[PathBuf],
) -> Result<()> {
    let backup = tempfile::Builder::new()
        .prefix(".mise-task-cache-backup-")
        .tempdir_in(root)?;
    let mut backed_up = Vec::new();
    for rel in remove_roots {
        let from = root.join(rel);
        match fs::symlink_metadata(&from) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                rollback_install(root, backup.path(), &[], &backed_up);
                return Err(err).wrap_err_with(|| {
                    format!("failed to inspect existing output {}", from.display())
                });
            }
        }
        let to = backup.path().join(rel);
        let backup_output = (|| -> Result<()> {
            if let Some(parent) = to.parent() {
                file::create_dir_all(parent)?;
            }
            fs::rename(&from, &to).wrap_err_with(|| format!("failed to back up {}", rel.display()))
        })();
        if let Err(err) = backup_output {
            rollback_install(root, backup.path(), &[], &backed_up);
            return Err(err);
        }
        backed_up.push(rel.clone());
    }

    let mut installed = Vec::new();
    for rel in install_roots {
        let from = staging.join(rel);
        let to = root.join(rel);
        let install = (|| -> Result<()> {
            ensure_no_symlink_ancestors(root, rel)?;
            if let Some(parent) = to.parent() {
                file::create_dir_all(parent)?;
            }
            fs::rename(&from, &to)
                .wrap_err_with(|| format!("failed to install cached output {}", rel.display()))
        })();
        if let Err(err) = install {
            rollback_install(root, backup.path(), &installed, &backed_up);
            return Err(err);
        }
        installed.push(rel.clone());
    }
    Ok(())
}

fn rollback_install(root: &Path, backup: &Path, installed: &[PathBuf], backed_up: &[PathBuf]) {
    for rel in installed.iter().rev() {
        if let Err(err) = file::remove_all(root.join(rel)) {
            warn!(
                "failed to remove partial cache restore {}: {err}",
                rel.display()
            );
        }
    }
    for rel in backed_up.iter().rev() {
        let from = backup.join(rel);
        let to = root.join(rel);
        if let Some(parent) = to.parent()
            && let Err(err) = file::create_dir_all(parent)
        {
            warn!(
                "failed to prepare cache restore rollback {}: {err}",
                rel.display()
            );
            continue;
        }
        if let Err(err) = fs::rename(&from, &to) {
            warn!("failed to roll back cached output {}: {err}", rel.display());
        }
    }
}

fn write_archive(path: &Path, root: &Path, roots: &[PathBuf]) -> Result<()> {
    let file = File::create(path)?;
    let encoder = zstd::Encoder::new(file, 0)?;
    let mut archive = Builder::new(encoder);
    let mut entries = BTreeMap::<PathBuf, PathBuf>::new();
    for rel_root in roots {
        let abs_root = root.join(rel_root);
        for entry in WalkDir::new(&abs_root).follow_links(false) {
            let entry = entry?;
            let abs = entry.path().to_path_buf();
            let rel = abs.strip_prefix(root)?.to_path_buf();
            ensure_safe_relative(&rel)?;
            entries.insert(rel, abs);
        }
    }

    for (rel, abs) in entries {
        let metadata = fs::symlink_metadata(&abs)?;
        let mut header = Header::new_gnu(if metadata.file_type().is_symlink() {
            EntryType::Symlink
        } else if metadata.is_dir() {
            EntryType::Directory
        } else {
            EntryType::File
        });
        header.set_mode(metadata_mode(&metadata));
        header.set_mtime(
            metadata
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
        if metadata.file_type().is_symlink() {
            header.set_size(0);
            archive.append_link(&mut header, &rel, fs::read_link(&abs)?)?;
        } else if metadata.is_dir() {
            header.set_size(0);
            archive.append_data(&mut header, &rel, std::io::empty())?;
        } else if metadata.is_file() {
            header.set_size(metadata.len());
            archive.append_data(&mut header, &rel, File::open(&abs)?)?;
        } else {
            bail!("unsupported output file type: {}", rel.display());
        }
    }
    let encoder = archive.into_inner()?;
    encoder.finish()?;
    Ok(())
}

#[cfg(unix)]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    if metadata.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_deserializes_and_rejects_unknown_fields() {
        let config: TaskCacheConfig = toml::from_str("enabled = true\nenv = ['PROFILE']").unwrap();
        assert!(config.enabled);
        assert_eq!(config.env, ["PROFILE"]);
        assert!(toml::from_str::<TaskCacheConfig>("remote = true").is_err());
    }

    #[test]
    fn safe_relative_paths_reject_escapes() {
        assert!(ensure_safe_relative(Path::new("dist/app")).is_ok());
        assert!(ensure_safe_relative(Path::new("../dist")).is_err());
        assert!(ensure_safe_relative(Path::new("/tmp/dist")).is_err());
        assert!(ensure_safe_relative(Path::new("")).is_err());
        assert!(ensure_safe_relative(Path::new(".")).is_err());
    }

    #[test]
    fn nested_output_roots_are_collapsed() {
        assert_eq!(
            remove_nested_roots(vec![
                PathBuf::from("dist/a.js"),
                PathBuf::from("dist"),
                PathBuf::from("coverage"),
            ]),
            vec![PathBuf::from("coverage"), PathBuf::from("dist")]
        );
    }

    #[test]
    fn failed_install_restores_previous_outputs() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir_in(root.path()).unwrap();
        fs::create_dir(root.path().join("dist")).unwrap();
        fs::write(root.path().join("dist/result.txt"), "old").unwrap();
        fs::create_dir(staging.path().join("dist")).unwrap();
        fs::write(staging.path().join("dist/result.txt"), "new").unwrap();

        assert!(
            install_transactionally(
                root.path(),
                staging.path(),
                &[PathBuf::from("dist"), PathBuf::from("missing")],
                &[PathBuf::from("dist")],
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("dist/result.txt")).unwrap(),
            "old"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("build")).unwrap();

        assert!(ensure_no_symlink_ancestors(root.path(), Path::new("build/dist")).is_err());
        assert!(ensure_no_symlink_ancestors(root.path(), Path::new("build")).is_ok());
    }
}
