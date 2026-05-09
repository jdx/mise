use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::config::{Alias, Config, Settings};
use crate::file::make_symlink_or_file;
use crate::plugins::VERSION_REGEX;
use crate::semver::split_version_prefix;
use crate::{backend, env, file};
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

pub async fn rebuild(config: &Config) -> Result<()> {
    for backend in backend::list() {
        for installs_dir in install_dirs_for(&backend) {
            rebuild_symlinks_in_dir(config, &backend, &installs_dir)?;
        }
    }
    Ok(())
}

pub async fn migrate_real_dirs(config: &Config) -> Result<()> {
    for backend in backend::list() {
        for installs_dir in install_dirs_for(&backend) {
            migrate_real_dirs_in_dir(config, &backend, &installs_dir)?;
        }
    }
    Ok(())
}

/// All install directories to consider for a backend: the backend's primary
/// installs_path plus any shared/system dirs that contain the tool. Per-dir
/// rebuilds are no-ops when desired state already matches actual state, so
/// dirs we have no write access to (read-only system installs) only error
/// out when we actually need to change something there.
fn install_dirs_for(backend: &Arc<dyn Backend>) -> Vec<PathBuf> {
    let ba = backend.ba();
    let mut dirs = vec![ba.installs_path.clone()];
    let tool_dir_name = ba.tool_dir_name();
    for shared_dir in env::shared_install_dirs() {
        let dir = shared_dir.join(&tool_dir_name);
        if dir.is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

fn rebuild_symlinks_in_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    let concrete_installs = installed_versions_in_dir(installs_dir)
        .into_iter()
        .filter(|v| is_concrete_install(v))
        .collect::<std::collections::HashSet<_>>();
    let symlinks = list_symlinks_for_dir(config, backend, installs_dir);
    for (from, to) in symlinks {
        let from_name = from.clone();
        let from = installs_dir.join(from);
        if from.exists() {
            if is_runtime_symlink(&from) {
                // Existing runtime symlink: only rewrite if the target changed.
                if file::resolve_symlink(&from)?.unwrap_or_default() == to {
                    continue;
                }
                trace!("Removing existing symlink: {}", from.display());
                file::remove_file(&from)?;
            } else if from
                .file_name()
                .zip(to.file_name())
                .is_some_and(|(f, t)| f != t)
                && !concrete_installs.contains(&from_name)
            {
                // Real (non-symlink) directory at a runtime-symlink slot —
                // legacy stale state from the 2026.4 regression. Replace it.
                trace!("Replacing stale runtime dir: {}", from.display());
                file::remove_all(&from)?;
            } else {
                continue;
            }
        }
        make_symlink_or_file(&to, &from)?;
    }
    remove_missing_symlinks_in_dir(installs_dir)?;
    Ok(())
}

fn migrate_real_dirs_in_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    let concrete_installs = installed_versions_in_dir(installs_dir)
        .into_iter()
        .filter(|v| is_concrete_install(v))
        .collect::<std::collections::HashSet<_>>();
    let symlinks = list_symlinks_for_dir(config, backend, installs_dir);
    for (from, to) in symlinks {
        let from_name = from.clone();
        let from = installs_dir.join(from);
        if !from.exists() || is_runtime_symlink(&from) || concrete_installs.contains(&from_name) {
            continue;
        }
        trace!("Replacing stale runtime dir: {}", from.display());
        file::remove_all(&from)?;
        make_symlink_or_file(&to, &from)?;
    }
    Ok(())
}

/// If the install directory name ends with a known arch suffix (e.g. `-x64`),
/// returns the base name and the suffix string. Otherwise returns the name
/// unchanged and an empty suffix.
fn split_arch_suffix(v: &str) -> (&str, &str) {
    if let Some(pos) = v.rfind('-') {
        let candidate = &v[pos + 1..];
        if Settings::normalize_arch(candidate).is_some() {
            return (&v[..pos], &v[pos..]);
        }
    }
    (v, "")
}

/// Build symlinks for versions found in a specific install directory.
fn list_symlinks_for_dir(
    config: &Config,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> IndexMap<String, PathBuf> {
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    for v in installed_versions_in_dir(installs_dir) {
        if is_temporary_runtime_label(&v) {
            continue;
        }
        // Strip the arch suffix (e.g. "-x64") before version parsing so that
        // Versioning does not treat it as a pre-release tag and generate
        // wrong partial-version symlink names. The suffix is then appended to
        // every generated symlink name so arch variants stay separate.
        let (base_v, arch_suffix) = split_arch_suffix(&v);
        let (prefix, version) = split_version_prefix(base_v);
        let Some(versions) = Versioning::new(version) else {
            continue;
        };
        let mut partial = vec![];
        while versions.nth(partial.len()).is_some() && versions.nth(partial.len() + 1).is_some() {
            let version = versions.nth(partial.len()).unwrap();
            partial.push(version.to_string());
            let from = format!("{}{}{}", prefix, partial.join("."), arch_suffix);
            symlinks.insert(from, rel_path(&v));
        }
        symlinks.insert(format!("{prefix}latest{arch_suffix}"), rel_path(&v));
        for (from, to) in &config
            .all_aliases
            .get(&backend.ba().short)
            .unwrap_or(&Alias::default())
            .versions
        {
            if from.contains('/') {
                continue;
            }
            if !base_v.starts_with(to) {
                continue;
            }
            symlinks.insert(format!("{prefix}{from}{arch_suffix}"), rel_path(&v));
        }
    }
    symlinks = symlinks
        .into_iter()
        .sorted_by_cached_key(|(k, _)| (Versioning::new(k), k.to_string()))
        .collect();
    symlinks
}

/// List real (non-symlink) installed versions in a specific directory.
fn installed_versions_in_dir(installs_dir: &Path) -> Vec<String> {
    if !installs_dir.is_dir() {
        return vec![];
    }
    file::dir_subdirs(installs_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|v| !v.starts_with('.'))
        .filter(|v| !is_runtime_symlink(&installs_dir.join(v)))
        .filter(|v| !installs_dir.join(v).join("incomplete").exists())
        .filter(|v| !VERSION_REGEX.is_match(v))
        .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
        .collect()
}

fn is_concrete_install(v: &str) -> bool {
    let (_, version) = split_version_prefix(v);
    version.chars().any(|c| c.is_ascii_digit()) && Versioning::new(version).is_some()
}

fn is_temporary_runtime_label(v: &str) -> bool {
    debug_assert!(
        {
            let remove_version = Versioning::new("2026.10.0").unwrap();
            *crate::cli::version::V < remove_version
        },
        "Temporary runtime symlink migration guard should be removed in version 2026.10.0."
    );
    // The 2026.4 runtime symlink regression created real "latest" dirs. Treat
    // only that literal label as generated state: numeric prefixes like "25"
    // may be concrete installs requested by users and must not be migrated.
    v == "latest"
}

pub fn remove_missing_symlinks(backend: Arc<dyn Backend>) -> Result<()> {
    remove_missing_symlinks_in_dir(&backend.ba().installs_path)
}

fn remove_missing_symlinks_in_dir(installs_dir: &Path) -> Result<()> {
    if !installs_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(installs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if is_runtime_symlink(&path) && !path.exists() {
            trace!("Removing missing symlink: {}", path.display());
            file::remove_file(path)?;
        }
    }
    // remove install dir if empty (ignore metadata)
    file::remove_dir_ignore(installs_dir, vec![".mise.backend.json", ".mise.backend"])?;
    Ok(())
}

pub fn is_runtime_symlink(path: &Path) -> bool {
    if let Ok(Some(link)) = file::resolve_symlink(path) {
        return link.starts_with("./");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_arch_suffix_known_arches() {
        // Known arch suffixes are split off
        assert_eq!(split_arch_suffix("corretto-8.462.08.1-x64"), ("corretto-8.462.08.1", "-x64"));
        assert_eq!(split_arch_suffix("corretto-8.462.08.1-arm64"), ("corretto-8.462.08.1", "-arm64"));
        assert_eq!(split_arch_suffix("tool-1.0.0-x86"), ("tool-1.0.0", "-x86"));
        assert_eq!(split_arch_suffix("tool-1.0.0-ppc64le"), ("tool-1.0.0", "-ppc64le"));
        assert_eq!(split_arch_suffix("tool-1.0.0-s390x"), ("tool-1.0.0", "-s390x"));
    }

    #[test]
    fn test_split_arch_suffix_non_arch() {
        // Non-arch suffixes are not split
        assert_eq!(split_arch_suffix("temurin-21.0.11+10.0.LTS"), ("temurin-21.0.11+10.0.LTS", ""));
        assert_eq!(split_arch_suffix("corretto-8.462.08.1"), ("corretto-8.462.08.1", ""));
        assert_eq!(split_arch_suffix("tool-1.0.0-beta"), ("tool-1.0.0-beta", ""));
        assert_eq!(split_arch_suffix("no-hyphen"), ("no-hyphen", ""));
        assert_eq!(split_arch_suffix("1.0.0"), ("1.0.0", ""));
    }

    #[test]
    fn test_symlink_names_include_arch_suffix() {
        // Regression test for the bug where corretto-8.462.08.1-x64 generated
        // symlinks named corretto-8, corretto-latest (no suffix), colliding with
        // native-arch installs. The fix: strip the arch suffix before version
        // parsing, then re-append it to every generated symlink name.
        let (base, suffix) = split_arch_suffix("corretto-8.462.08.1-x64");
        assert_eq!(base, "corretto-8.462.08.1");
        assert_eq!(suffix, "-x64");

        let (prefix, _version) = crate::semver::split_version_prefix(base);
        assert_eq!(prefix, "corretto-");

        // Partial version symlinks include the arch suffix
        assert_eq!(format!("{prefix}8{suffix}"), "corretto-8-x64");
        assert_eq!(format!("{prefix}8.462{suffix}"), "corretto-8.462-x64");
        assert_eq!(format!("{prefix}latest{suffix}"), "corretto-latest-x64");

        // Without our fix, Versioning would parse "8.462.08.1-x64" treating
        // -x64 as a pre-release tag, producing corretto-8 (no suffix) which
        // would collide with the native arm64 corretto-8 symlink.
        let (prefix_native, _) = crate::semver::split_version_prefix("corretto-8.462.08.1");
        assert_eq!(format!("{prefix_native}latest"), "corretto-latest");
        // The two "latest" symlinks are distinct — no collision
        assert_ne!(
            format!("{prefix}latest{suffix}"),
            format!("{prefix_native}latest")
        );
    }
}
