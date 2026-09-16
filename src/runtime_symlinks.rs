use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::config::{Alias, Config};
use crate::file::make_symlink_or_file;
use crate::plugins::VERSION_REGEX;
use crate::semver::split_version_prefix;
use crate::toolset::{ToolRequest, Toolset};
use crate::{backend, env, file};
use eyre::{Result, WrapErr};
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

pub(crate) async fn rebuild_for_toolset(config: &Config, ts: &Toolset) -> Result<()> {
    rebuild_for_backends(config, ts, ts.list_cached_and_current_backends()).await
}

pub(crate) async fn rebuild_for_backends(
    config: &Config,
    ts: &Toolset,
    backends: impl IntoIterator<Item = Arc<dyn Backend>>,
) -> Result<()> {
    let rebuilds = backends.into_iter().flat_map(|backend| {
        install_dirs_for(&backend)
            .into_iter()
            .map(move |installs_dir| (backend.clone(), installs_dir))
    });
    run_all_rebuilds(rebuilds, |(backend, installs_dir)| {
        rebuild_symlinks_in_dir(config, ts, &backend, &installs_dir).wrap_err_with(|| {
            format!(
                "failed to rebuild runtime symlinks for {} in {}",
                backend.ba().short,
                installs_dir.display()
            )
        })
    })
}

fn run_all_rebuilds<T>(
    rebuilds: impl IntoIterator<Item = T>,
    mut rebuild: impl FnMut(T) -> Result<()>,
) -> Result<()> {
    let errors = rebuilds
        .into_iter()
        .filter_map(|item| rebuild(item).err())
        .collect_vec();
    if errors.is_empty() {
        return Ok(());
    }
    Err(eyre::eyre!(
        "{} runtime symlink repair(s) failed:\n{}",
        errors.len(),
        errors.iter().map(|err| format!("{err:#}")).join("\n")
    ))
}

pub(crate) async fn migrate_real_dirs(config: &Config) -> Result<()> {
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
    ts: &Toolset,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    let concrete_installs = installed_versions_in_dir(installs_dir)
        .into_iter()
        .filter(|v| is_concrete_install(v))
        .collect::<HashSet<_>>();
    let symlinks = list_symlinks_for_dir(config, Some(ts), backend, installs_dir);
    for (from, to) in &symlinks {
        let from_name = from.clone();
        let from = installs_dir.join(from);
        if from.exists() {
            if is_runtime_symlink(&from) {
                // Existing runtime symlink: only rewrite if the target changed.
                if file::resolve_symlink(&from)?.unwrap_or_default() == *to {
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
        make_symlink_or_file(to, &from)?;
    }
    let default_alias = Alias::default();
    let aliases = &config
        .all_aliases
        .get(&backend.ba().short)
        .unwrap_or(&default_alias)
        .versions;
    prune_stale_generated_symlinks(
        installs_dir,
        &symlinks,
        &configured_alias_names(aliases, installs_dir),
    )?;
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
        .collect::<HashSet<_>>();
    let symlinks = list_symlinks_for_dir(config, None, backend, installs_dir);
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

/// Build symlinks for versions found in a specific install directory.
fn list_symlinks_for_dir(
    config: &Config,
    ts: Option<&Toolset>,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> IndexMap<String, PathBuf> {
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    for v in installed_versions_in_dir(installs_dir) {
        if is_temporary_runtime_label(&v) {
            continue;
        }
        let (prefix, _) = split_version_prefix(&v);
        for from in generated_names_for(&v) {
            symlinks.insert(from, rel_path(&v));
        }
        for (from, to) in &config
            .all_aliases
            .get(&backend.ba().short)
            .unwrap_or(&Alias::default())
            .versions
        {
            if from.contains('/') {
                continue;
            }
            if !v.starts_with(to) {
                continue;
            }
            symlinks.insert(format!("{prefix}{from}"), rel_path(&v));
        }
    }
    if let Some(ts) = ts {
        for (b, tv) in ts.list_current_versions() {
            if b.ba() != backend.ba() {
                continue;
            }
            if !matches!(tv.request, ToolRequest::Sub { .. }) {
                continue;
            }
            let Some(from) = tv.runtime_pathname() else {
                continue;
            };
            let install_path = tv.install_path();
            if install_path.parent() != Some(installs_dir) || !install_path.exists() {
                continue;
            }
            if let Some(to) = install_path
                .file_name()
                .map(|to| PathBuf::from(".").join(to))
            {
                symlinks.insert(from, to);
            }
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
    real_installs_in_dir(installs_dir)
        .into_iter()
        .filter(|v| !installs_dir.join(v).join("incomplete").exists())
        .filter(|v| !VERSION_REGEX.is_match(v))
        .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
        .collect()
}

/// Every real (non-symlink) install directory, including the ones no longer
/// eligible for a runtime symlink. [`installed_versions_in_dir`] is the
/// eligible subset.
fn real_installs_in_dir(installs_dir: &Path) -> Vec<String> {
    if !installs_dir.is_dir() {
        return vec![];
    }
    file::dir_subdirs(installs_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|v| !v.starts_with('.'))
        .filter(|v| !is_runtime_symlink(&installs_dir.join(v)))
        .collect()
}

/// The link names [`list_symlinks_for_dir`] derives from one install: the
/// dotted version prefixes (`1`, `1.3`) and `{prefix}latest`.
fn generated_names_for(v: &str) -> Vec<String> {
    let (prefix, version) = split_version_prefix(v);
    let Some(versions) = Versioning::new(&version) else {
        return vec![];
    };
    let mut names = vec![];
    let mut partial: Vec<String> = vec![];
    while versions.nth(partial.len()).is_some() && versions.nth(partial.len() + 1).is_some() {
        partial.push(versions.nth(partial.len()).unwrap().to_string());
        names.push(format!("{}{}", prefix, partial.join(".")));
    }
    names.push(format!("{prefix}latest"));
    names
}

/// Every name mise generates for the real installs in this directory.
///
/// Derived from all real installs, not just the eligible ones: a link is only
/// stale *because* its version stopped being eligible, so the version that
/// explains the name is by definition missing from the eligible set.
fn generated_symlink_namespace(installs_dir: &Path) -> HashSet<String> {
    real_installs_in_dir(installs_dir)
        .into_iter()
        .filter(|v| !is_temporary_runtime_label(v))
        .flat_map(|v| generated_names_for(&v))
        .collect()
}

/// Link names configured aliases occupy, with and without each install's
/// version prefix (`lts` and `temurin-lts`). An alias may be named like a
/// version prefix, so these are excluded from pruning by name.
fn configured_alias_names(
    aliases: &IndexMap<String, String>,
    installs_dir: &Path,
) -> HashSet<String> {
    let prefixes = real_installs_in_dir(installs_dir)
        .into_iter()
        .map(|v| split_version_prefix(&v).0)
        .collect::<HashSet<_>>();
    aliases
        .keys()
        .flat_map(|from| {
            prefixes
                .iter()
                .map(move |prefix| format!("{prefix}{from}"))
                .chain(std::iter::once(from.clone()))
        })
        .collect()
}

/// Remove runtime symlinks mise generated that the current install state no
/// longer supports.
///
/// The rebuild loop is additive — it writes the links it wants, and
/// [`remove_missing_symlinks_in_dir`] only clears pointers whose target is
/// gone. A generated name whose target still exists on disk therefore survives
/// after it stops being eligible: interrupt an install and the `incomplete`
/// marker drops that version from the symlink set while `latest` and `1` keep
/// pointing into the half-installed directory.
///
/// A link is removed only when all of these hold:
/// - its name is one mise generates for a real install here, so configured
///   aliases and hand-picked names are left alone,
/// - this rebuild did not ask for that name, and
/// - its target is not an install that is currently eligible for links.
///
/// Only relative `./`-style links count as mise's own, so an absolute symlink a
/// user dropped in here is never a candidate. A `Sub`/`Prefix` request pins a
/// link for a version that is otherwise ineligible; rebuilding from a directory
/// that does not request it drops the link, and the requesting directory
/// recreates it on its next rebuild.
fn prune_stale_generated_symlinks(
    installs_dir: &Path,
    desired: &IndexMap<String, PathBuf>,
    alias_names: &HashSet<String>,
) -> Result<()> {
    let namespace = generated_symlink_namespace(installs_dir);
    if namespace.is_empty() {
        return Ok(());
    }
    let eligible = installed_versions_in_dir(installs_dir)
        .into_iter()
        .collect::<HashSet<_>>();
    for path in file::ls(installs_dir)? {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if desired.contains_key(&name) || alias_names.contains(&name) || !namespace.contains(&name)
        {
            continue;
        }
        if let Some(target) = runtime_symlink_target(&path)
            && let Some(target) = target.file_name().map(|t| t.to_string_lossy().to_string())
            && !eligible.contains(&target)
        {
            trace!("Removing stale runtime symlink: {}", path.display());
            file::remove_file(&path)?;
        }
    }
    Ok(())
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

pub(crate) fn remove_missing_symlinks(backend: Arc<dyn Backend>) -> Result<()> {
    remove_missing_symlinks_in_dir(&backend.ba().installs_path)
}

pub(crate) fn remove_missing_symlinks_in_dir(installs_dir: &Path) -> Result<()> {
    if !installs_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(installs_dir)? {
        let entry = entry?;
        let path = entry.path();
        // On Windows runtime symlinks are regular files containing the relative
        // target, so `path.exists()` cannot detect a dangling pointer — resolve
        // the stored target and check that instead. On unix this is equivalent
        // to following the symlink. (#5260)
        if let Some(target) = runtime_symlink_target(&path)
            && !installs_dir.join(target).exists()
        {
            trace!("Removing missing symlink: {}", path.display());
            file::remove_file(path)?;
        }
    }
    // remove install dir if empty (ignore metadata)
    file::remove_dir_ignore(installs_dir, vec![".mise.backend.json", ".mise.backend"])?;
    Ok(())
}

pub(crate) fn is_runtime_symlink(path: &Path) -> bool {
    runtime_symlink_target(path).is_some()
}

/// Returns the (relative) target a runtime symlink points to, or None if
/// `path` is not a runtime symlink.
fn runtime_symlink_target(path: &Path) -> Option<PathBuf> {
    if let Ok(Some(link)) = file::resolve_symlink(path)
        && link.starts_with("./")
    {
        return Some(link);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn run_all_rebuilds_attempts_every_item() {
        let mut attempted = vec![];
        let err = run_all_rebuilds([1, 2, 3], |item| {
            attempted.push(item);
            if item == 1 || item == 3 {
                eyre::bail!("repair {item} failed");
            }
            Ok(())
        })
        .unwrap_err();

        assert_eq!(attempted, [1, 2, 3]);
        let message = format!("{err:#}");
        assert!(message.contains("repair 1 failed"));
        assert!(message.contains("repair 3 failed"));
    }

    // https://github.com/jdx/mise/discussions/5260 — on Windows runtime
    // symlinks are regular files containing the target, so dangling pointers
    // were never detected as missing.
    #[test]
    fn remove_missing_symlinks_in_dir_removes_dangling_pointers() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("1.0.0"))?;
        // valid pointer -> concrete install
        make_symlink_or_file(Path::new("./1.0.0"), &installs_dir.join("1"))?;
        // dangling pointer -> version that no longer exists
        make_symlink_or_file(Path::new("./2.0.0"), &installs_dir.join("2"))?;

        remove_missing_symlinks_in_dir(&installs_dir)?;

        // a dangling unix symlink still has metadata even though `exists()` is
        // false, so this asserts actual removal on both platforms
        assert!(fs::symlink_metadata(installs_dir.join("2")).is_err());
        // concrete install and valid pointer retained
        assert!(installs_dir.join("1.0.0").is_dir());
        assert!(is_runtime_symlink(&installs_dir.join("1")));
        Ok(())
    }

    #[test]
    fn remove_missing_symlinks_in_dir_removes_dir_when_only_dangling_pointers_remain() -> Result<()>
    {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(&installs_dir)?;
        fs::write(installs_dir.join(".mise.backend.json"), "{}")?;
        make_symlink_or_file(Path::new("./2.0.0"), &installs_dir.join("2"))?;
        make_symlink_or_file(Path::new("./2.0.0"), &installs_dir.join("latest"))?;

        remove_missing_symlinks_in_dir(&installs_dir)?;

        // all pointers were dangling -> whole dir removed (metadata ignored)
        assert!(!installs_dir.exists());
        Ok(())
    }

    /// `latest`/`2` keep pointing into a half-installed directory: the
    /// `incomplete` marker drops 2.1.0 from the symlink set, but the directory
    /// is still there so the links are not dangling.
    #[test]
    fn prune_stale_generated_symlinks_removes_links_into_ineligible_installs() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        fs::write(installs_dir.join("2.1.0").join("incomplete"), "")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2.1"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(&installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(fs::symlink_metadata(installs_dir.join("2")).is_err());
        assert!(fs::symlink_metadata(installs_dir.join("2.1")).is_err());
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        // the install itself is never touched
        assert!(installs_dir.join("2.1.0").is_dir());
        Ok(())
    }

    #[test]
    fn prune_stale_generated_symlinks_keeps_links_to_eligible_installs() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(&installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(is_runtime_symlink(&installs_dir.join("2")));
        assert!(is_runtime_symlink(&installs_dir.join("latest")));
        Ok(())
    }

    /// Only names mise derives from an install are pruned; `next` is someone
    /// else's, even though it points at the same ineligible version.
    #[test]
    fn prune_stale_generated_symlinks_keeps_names_it_does_not_generate() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        fs::write(installs_dir.join("2.1.0").join("incomplete"), "")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("next"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(&installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(is_runtime_symlink(&installs_dir.join("next")));
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        Ok(())
    }

    #[test]
    fn prune_stale_generated_symlinks_keeps_names_this_rebuild_asked_for() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        fs::write(installs_dir.join("2.1.0").join("incomplete"), "")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;
        let desired = IndexMap::from([("latest".to_string(), PathBuf::from("./2.1.0"))]);

        prune_stale_generated_symlinks(&installs_dir, &desired, &HashSet::new())?;

        assert!(is_runtime_symlink(&installs_dir.join("latest")));
        Ok(())
    }

    /// An alias may be named like a version prefix mise also generates.
    #[test]
    fn prune_stale_generated_symlinks_keeps_configured_aliases() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        fs::write(installs_dir.join("2.1.0").join("incomplete"), "")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2"))?;
        let aliases = IndexMap::from([("2".to_string(), "2.1.0".to_string())]);

        prune_stale_generated_symlinks(
            &installs_dir,
            &IndexMap::new(),
            &configured_alias_names(&aliases, &installs_dir),
        )?;

        assert!(is_runtime_symlink(&installs_dir.join("2")));
        Ok(())
    }

    #[test]
    fn configured_alias_names_covers_prefixed_installs() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("java");
        fs::create_dir_all(installs_dir.join("temurin-21.0.1"))?;
        let aliases = IndexMap::from([("lts".to_string(), "temurin-21".to_string())]);

        let names = configured_alias_names(&aliases, &installs_dir);

        assert!(names.contains("temurin-lts"));
        assert!(names.contains("lts"));
        Ok(())
    }

    /// The namespace has to come from every real install, including the ones
    /// that are no longer eligible — that is the only thing tying a stale name
    /// back to mise.
    #[test]
    fn generated_symlink_namespace_covers_ineligible_installs() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        fs::write(installs_dir.join("2.1.0").join("incomplete"), "")?;

        let namespace = generated_symlink_namespace(&installs_dir);

        assert!(installed_versions_in_dir(&installs_dir).is_empty());
        assert!(namespace.contains("2"));
        assert!(namespace.contains("2.1"));
        assert!(namespace.contains("latest"));
        Ok(())
    }

    #[test]
    fn generated_names_for_matches_the_names_the_rebuild_writes() {
        assert_eq!(generated_names_for("1.3.1"), ["1", "1.3", "latest"]);
        assert_eq!(
            generated_names_for("temurin-21.0.1"),
            ["temurin-21", "temurin-21.0", "temurin-latest"]
        );
    }
}
