use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
#[cfg(unix)]
use crate::config::SettingsExt;
use crate::config::{Alias, Config};
use crate::plugins::VERSION_REGEX;
use crate::semver::split_version_prefix;
use crate::toolset::{
    ConfigScope, ResolveOptions, ToolRequest, Toolset, ToolsetBuilder, install_state,
};
use crate::{env, file};
use eyre::{Result, WrapErr};
use indexmap::IndexMap;
use itertools::Itertools;
use versions::Versioning;

pub async fn rebuild_for_toolset(config: &Arc<Config>, ts: &Toolset) -> Result<()> {
    rebuild_for_backends(config, ts, ts.list_cached_and_current_backends()).await
}

pub(crate) async fn rebuild_for_backends(
    config: &Arc<Config>,
    ts: &Toolset,
    backends: impl IntoIterator<Item = Arc<dyn Backend>>,
) -> Result<()> {
    let global = global_toolset(config).await;
    let rebuilds = backends.into_iter().flat_map(|backend| {
        install_dirs_for(&backend)
            .into_iter()
            .map(move |installs_dir| (backend.clone(), installs_dir))
    });
    run_all_rebuilds(rebuilds, |(backend, installs_dir)| {
        rebuild_symlinks_in_dir(config, ts, global.as_ref(), &backend, &installs_dir).wrap_err_with(
            || {
                format!(
                    "failed to rebuild runtime symlinks for {} in {}",
                    backend.ba().short,
                    installs_dir.display()
                )
            },
        )
    })
}

/// The tools global and system config select, resolved against what is
/// installed. It backs the `global` link, which names the version in use
/// outside any project, so project config and `MISE_*_VERSION` stay out of it.
/// `None` when it could not be built, so the links already there are kept.
async fn global_toolset(config: &Arc<Config>) -> Option<Toolset> {
    let built = ToolsetBuilder::new()
        .with_scope(ConfigScope::GlobalOnly)
        .without_runtime_env()
        .build_unresolved(config);
    let mut global = match built {
        Ok(global) => global,
        Err(err) => {
            debug!("skipping global runtime symlink, global config did not load: {err:#}");
            return None;
        }
    };
    let opts = ResolveOptions {
        offline: true,
        ..ResolveOptions::without_lockfile_warnings()
    };
    if let Err(err) = global.resolve_with_opts(config, &opts).await {
        debug!("skipping global runtime symlink, global tools did not resolve: {err:#}");
        return None;
    }
    Some(global)
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

/// All install directories to consider for a backend: the backend's primary
/// installs_path plus any shared/system dirs that contain the tool. Per-dir
/// rebuilds are no-ops when desired state already matches actual state, so
/// dirs we have no write access to (read-only system installs) only error
/// out when we actually need to change something there.
fn install_dirs_for(backend: &Arc<dyn Backend>) -> Vec<PathBuf> {
    let ba = backend.ba();
    let mut dirs = vec![ba.installs_path().to_path_buf()];
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
    global: Option<&Toolset>,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> Result<()> {
    if installs_dir == backend.ba().installs_path() {
        crate::install_layout::resolver::heal_links(backend.ba());
    }
    let concrete_installs = concrete_installs_in_dir(backend, installs_dir);
    let symlinks = list_symlinks_for_dir(config, Some(ts), global, backend, installs_dir);
    let default_alias = Alias::default();
    let aliases = &config
        .all_aliases
        .get(&backend.ba().short)
        .unwrap_or(&default_alias)
        .versions;
    let alias_names = configured_alias_names(aliases, installs_dir);
    #[cfg(unix)]
    if installs_dir.parent() == Some(crate::config::Settings::get().system_installs_dir())
        && crate::system_install::needs_elevation(installs_dir)
    {
        // The root helper only creates, retargets and removes symlinks, so the
        // pruning below is computed here and sent along with the desired links.
        let remove = stale_generated_symlinks(backend, installs_dir, &symlinks, &alias_names)?
            .into_iter()
            .chain(missing_symlinks_in_dir(installs_dir)?)
            .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        // A real directory in a generated selector slot (`latest`, a version
        // prefix) that is not a concrete install is legacy stale state. Alias
        // names are excluded: an alias may point at a real directory.
        let namespace = generated_symlink_namespace(installs_dir);
        let replace = symlinks
            .iter()
            .filter(|(from, to)| {
                let path = installs_dir.join(from);
                namespace.contains(*from)
                    && !alias_names.contains(*from)
                    && path.is_dir()
                    && !is_runtime_symlink(&path)
                    && path
                        .file_name()
                        .zip(to.file_name())
                        .is_some_and(|(f, t)| f != t)
                    && !concrete_installs.contains(*from)
            })
            .map(|(from, _)| from.clone())
            .collect();
        return crate::system_install::links(
            installs_dir,
            symlinks
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            remove,
            replace,
        );
    }
    for (from, to) in &symlinks {
        let from_name = from.clone();
        let from = installs_dir.join(from);
        if from.exists() {
            if is_runtime_symlink(&from) {
                // Existing runtime symlink: only rewrite if the target changed.
                // A Windows text-file alias is rewritten as a real link.
                if runtime_symlink_target(&from).as_ref() == Some(to) && !is_text_file_alias(&from)
                {
                    continue;
                }
                // `make_dir_link` below replaces it, keeping the old link if the
                // new one cannot be made.
                trace!("Retargeting existing symlink: {}", from.display());
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
        if let Err(err) = file::make_dir_link(to, &from) {
            if cfg!(windows) {
                warn!(
                    "could not create {} -> {}: {err:#}. The tool works through mise, but \
                     external programs cannot use this path; run `mise where` for the real \
                     install path.",
                    from.display(),
                    to.display()
                );
            } else {
                return Err(err);
            }
        }
    }
    prune_stale_generated_symlinks(backend, installs_dir, &symlinks, &alias_names)?;
    remove_missing_symlinks_in_dir(installs_dir)?;
    Ok(())
}

/// Build symlinks for versions found in a specific install directory.
fn list_symlinks_for_dir(
    config: &Config,
    ts: Option<&Toolset>,
    global: Option<&Toolset>,
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
) -> IndexMap<String, PathBuf> {
    let mut symlinks = IndexMap::new();
    let rel_path = |x: &String| PathBuf::from(".").join(x.clone());
    for v in installed_versions_in_dir(backend, installs_dir) {
        if is_runtime_selector_label(&v) {
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
            if let Some(to) = pin_target(backend, &tv, installs_dir) {
                symlinks.insert(from, to);
            }
        }
    }
    // `global` is the version global config selects. A real directory by that
    // name is the user's and stays untouched.
    let global_link = installs_dir.join(GLOBAL_LINK);
    if std::fs::symlink_metadata(&global_link).is_err() || is_runtime_symlink(&global_link) {
        let to = match global {
            Some(global) => global
                .list_current_versions()
                .into_iter()
                .filter(|(b, _)| b.ba() == backend.ba())
                .find_map(|(_, tv)| pin_target(backend, &tv, installs_dir)),
            // Global config could not be read: keep the link as it is.
            None => runtime_symlink_target(&global_link),
        };
        if let Some(to) = to {
            symlinks.insert(GLOBAL_LINK.to_string(), to);
        }
    }
    symlinks = symlinks
        .into_iter()
        .sorted_by_cached_key(|(k, _)| (Versioning::new(k), k.to_string()))
        .collect();
    symlinks
}

/// The link name for the version global config selects.
const GLOBAL_LINK: &str = "global";

/// The `./name` link target naming `tv`'s install inside `installs_dir`, or
/// `None` when that install is not there or never finished.
fn pin_target(
    backend: &Arc<dyn Backend>,
    tv: &crate::toolset::ToolVersion,
    installs_dir: &Path,
) -> Option<PathBuf> {
    let install_path = tv.install_path();
    // An identity-layout install is a hashed directory in the installs root;
    // the pin points at the version link beside it, which names the same
    // installation, never at the hash.
    if crate::install_layout::resolver::dir_name_of(&install_path).is_some() {
        let name = tv.tv_pathname();
        return (installs_dir == tv.ba().installs_path()
            && install_path.exists()
            && installs_dir.join(&name).exists()
            && !is_install_incomplete(backend, installs_dir, &name))
        .then(|| PathBuf::from(".").join(name));
    }
    if install_path.parent() != Some(installs_dir) || !install_path.exists() {
        return None;
    }
    let name = install_path.file_name()?;
    if is_install_incomplete(backend, installs_dir, &name.to_string_lossy()) {
        return None;
    }
    Some(PathBuf::from(".").join(name))
}

/// List real (non-symlink) installed versions in a specific directory.
fn installed_versions_in_dir(backend: &Arc<dyn Backend>, installs_dir: &Path) -> Vec<String> {
    real_installs_in_dir(installs_dir)
        .into_iter()
        .filter(|v| !is_install_incomplete(backend, installs_dir, v))
        .filter(|v| !VERSION_REGEX.is_match(v) && !backend.is_backend_prerelease(v))
        .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
        .collect()
}

/// Whether version dir `v` belongs to an install that never finished. The
/// marker is keyed by the tool's name, not by the install dir's basename, which
/// install state can map to a differently named directory.
///
/// An identity-layout version is a link, and its marker is keyed by the
/// installation the link names.
fn is_install_incomplete(backend: &Arc<dyn Backend>, installs_dir: &Path, v: &str) -> bool {
    let key = crate::install_layout::resolver::link_target(&installs_dir.join(v))
        .and_then(|install| install.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| v.to_string());
    install_state::is_incomplete(backend.ba(), &key)
}

/// Real install directories a rebuild must never replace with a selector
/// link. An interrupted install is not eligible for links, but its directory
/// still holds whatever the installer got to: a `1.1` that never finished must
/// not be wiped and turned into a link to a complete `1.1.0`. The marker alone
/// is enough: a directory mise was installing into is protected whatever its
/// name, even one in a selector slot like `latest`.
fn concrete_installs_in_dir(backend: &Arc<dyn Backend>, installs_dir: &Path) -> HashSet<String> {
    installed_versions_in_dir(backend, installs_dir)
        .into_iter()
        .filter(|v| is_concrete_install(v))
        .chain(
            real_installs_in_dir(installs_dir)
                .into_iter()
                .filter(|v| is_install_incomplete(backend, installs_dir, v)),
        )
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
        .filter(|v| !is_runtime_selector_label(v))
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
/// - its name is one mise generates for a real install here,
/// - this rebuild did not ask for that name, and
/// - its target is not an install that is currently eligible for links.
///
/// A generated name holding a relative `./` link is mise's to manage, whoever
/// wrote it. There is no ownership marker, and none is implied: the rebuild
/// loop above already removes and repoints any such link whose target no longer
/// matches, without asking who put it there. What survives a prune is therefore
/// what survives a rewrite — a name mise does not generate (a configured alias,
/// `next`, any hand-picked label) or a link that is not in `./` form.
///
/// The one other writer of links here, a `Sub` request, is named
/// `sub-{sub}-{orig_version}` and so never occupies a generated name, which
/// keeps another directory's pin out of reach of a rebuild that does not know
/// about it.
fn prune_stale_generated_symlinks(
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
    desired: &IndexMap<String, PathBuf>,
    alias_names: &HashSet<String>,
) -> Result<()> {
    for path in stale_generated_symlinks(backend, installs_dir, desired, alias_names)? {
        trace!("Removing stale runtime symlink: {}", path.display());
        file::remove_dir_link(&path)?;
    }
    Ok(())
}

/// Generated symlinks that point at a version no longer eligible for them.
fn stale_generated_symlinks(
    backend: &Arc<dyn Backend>,
    installs_dir: &Path,
    desired: &IndexMap<String, PathBuf>,
    alias_names: &HashSet<String>,
) -> Result<Vec<PathBuf>> {
    let mut stale = vec![];
    let global_link = installs_dir.join(GLOBAL_LINK);
    if !desired.contains_key(GLOBAL_LINK) && is_runtime_symlink(&global_link) {
        stale.push(global_link);
    }
    let namespace = generated_symlink_namespace(installs_dir);
    if namespace.is_empty() {
        return Ok(stale);
    }
    let eligible = installed_versions_in_dir(backend, installs_dir)
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
            stale.push(path);
        }
    }
    Ok(stale)
}

fn is_concrete_install(v: &str) -> bool {
    let (_, version) = split_version_prefix(v);
    version.chars().any(|c| c.is_ascii_digit()) && Versioning::new(version).is_some()
}

fn is_runtime_selector_label(v: &str) -> bool {
    // A real `latest` directory is stale selector state. It must not suppress
    // rebuilding `latest` as a link to the concrete installed version. Numeric
    // prefixes remain eligible because they can be explicit user installs.
    v == "latest"
}

pub fn remove_missing_symlinks(backend: Arc<dyn Backend>) -> Result<()> {
    remove_missing_symlinks_in_dir(backend.ba().installs_path())
}

pub(crate) fn remove_missing_symlinks_in_dir(installs_dir: &Path) -> Result<()> {
    if !installs_dir.exists() {
        return Ok(());
    }
    for path in missing_symlinks_in_dir(installs_dir)? {
        trace!("Removing missing symlink: {}", path.display());
        file::remove_dir_link(&path)?;
    }
    // remove install dir if empty (ignore metadata)
    file::remove_dir_ignore(installs_dir, vec![".mise.backend.json", ".mise.backend"])?;
    Ok(())
}

/// Runtime symlinks whose target no longer exists.
fn missing_symlinks_in_dir(installs_dir: &Path) -> Result<Vec<PathBuf>> {
    if !installs_dir.exists() {
        return Ok(vec![]);
    }
    let mut missing = vec![];
    for entry in std::fs::read_dir(installs_dir)? {
        let path = entry?.path();
        // On Windows runtime symlinks are regular files containing the relative
        // target, so `path.exists()` cannot detect a dangling pointer — resolve
        // the stored target and check that instead. On unix this is equivalent
        // to following the symlink. (#5260)
        if let Some(target) = runtime_symlink_target(&path)
            && !installs_dir.join(target).exists()
        {
            missing.push(path);
        } else if crate::install_layout::resolver::is_compat_link_shape(&path)
            && crate::install_layout::resolver::link_target(&path).is_none()
        {
            // A version link into the identity layout whose installation is gone.
            missing.push(path);
        }
    }
    Ok(missing)
}

pub fn is_runtime_symlink(path: &Path) -> bool {
    runtime_symlink_target(path).is_some()
}

/// Returns the (relative) target a runtime symlink points to, or None if
/// `path` is not a runtime symlink.
fn runtime_symlink_target(path: &Path) -> Option<PathBuf> {
    let link = file::resolve_symlink(path).ok().flatten()?;
    if link.starts_with("./") {
        return Some(link);
    }
    // A Windows junction records an absolute target; one that points at a
    // sibling directory is the same runtime link written as `./name`.
    if !cfg!(windows) {
        return None;
    }
    let (parent, name) = (link.parent()?, link.file_name()?);
    (std::fs::canonicalize(parent).ok()? == std::fs::canonicalize(path.parent()?).ok()?)
        .then(|| Path::new(".").join(name))
}

/// A pre-junction Windows alias: a regular file holding the target path.
fn is_text_file_alias(path: &Path) -> bool {
    cfg!(windows) && path.is_file() && !path.is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::make_symlink_or_file;
    use std::fs;

    fn npm_test_backend() -> Arc<dyn Backend> {
        Arc::new(crate::backend::npm::test_backend("happy", None, None))
    }

    /// A backend whose name no other test shares. The incomplete marker lives
    /// in the cache dir every test process shares, keyed by the tool's name, so
    /// a fixed name would leak one test's marker into another.
    fn unique_backend(temp_dir: &tempfile::TempDir) -> Arc<dyn Backend> {
        let suffix = temp_dir.path().file_name().unwrap().to_string_lossy();
        Arc::new(crate::backend::npm::test_backend(
            &format!("happy{suffix}"),
            None,
            None,
        ))
    }

    /// Removes the marker [`interrupted_install`] wrote when the test ends.
    struct InterruptedInstall(Arc<crate::args::BackendArg>, String);

    impl Drop for InterruptedInstall {
        fn drop(&mut self) {
            let _ = install_state::clear_incomplete_marker(&self.0, &self.1);
        }
    }

    /// Leaves version `v` of `backend` the way an interrupted install does:
    /// the incomplete marker is still in place.
    fn interrupted_install(backend: &Arc<dyn Backend>, v: &str) -> Result<InterruptedInstall> {
        install_state::mark_incomplete(backend.ba(), v)?;
        Ok(InterruptedInstall(backend.ba().clone(), v.to_string()))
    }

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
    fn installed_versions_in_dir_skips_backend_prereleases() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("npm-happy");
        fs::create_dir_all(installs_dir.join("1.2.4"))?;
        fs::create_dir_all(installs_dir.join("1.3.1-3"))?;
        let backend = npm_test_backend();

        assert_eq!(
            installed_versions_in_dir(&backend, &installs_dir),
            ["1.2.4"]
        );
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
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        let _interrupted = interrupted_install(&backend, "2.1.0")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2.1"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(&backend, &installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(fs::symlink_metadata(installs_dir.join("2")).is_err());
        assert!(fs::symlink_metadata(installs_dir.join("2.1")).is_err());
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        // the install itself is never touched
        assert!(installs_dir.join("2.1.0").is_dir());
        Ok(())
    }

    /// An interrupted `1.1` sits in the slot a complete `1.1.0` generates a
    /// `1.1` link for; it is no longer eligible, but it must stay protected.
    #[test]
    fn concrete_installs_in_dir_protects_interrupted_installs() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("1.1.0"))?;
        fs::create_dir_all(installs_dir.join("1.1"))?;
        fs::create_dir_all(installs_dir.join("latest"))?;
        let _interrupted = interrupted_install(&backend, "1.1")?;
        let _interrupted_latest = interrupted_install(&backend, "latest")?;

        assert_eq!(
            installed_versions_in_dir(&backend, &installs_dir),
            ["1.1.0"]
        );
        assert_eq!(
            concrete_installs_in_dir(&backend, &installs_dir),
            HashSet::from(["1.1", "1.1.0", "latest"].map(String::from))
        );
        Ok(())
    }

    /// `1.3.1-3` carries no channel tag, so only the backend knows it is a
    /// pre-release and that links an older mise wrote into it are stale.
    #[test]
    fn prune_stale_generated_symlinks_removes_links_into_backend_prereleases() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("npm-happy");
        fs::create_dir_all(installs_dir.join("1.2.4"))?;
        fs::create_dir_all(installs_dir.join("1.3.1-3"))?;
        make_symlink_or_file(Path::new("./1.2.4"), &installs_dir.join("1.2"))?;
        make_symlink_or_file(Path::new("./1.3.1-3"), &installs_dir.join("1.3"))?;
        make_symlink_or_file(Path::new("./1.3.1-3"), &installs_dir.join("latest"))?;
        make_symlink_or_file(Path::new("./1.3.1-3"), &installs_dir.join("next"))?;

        prune_stale_generated_symlinks(
            &npm_test_backend(),
            &installs_dir,
            &IndexMap::new(),
            &HashSet::new(),
        )?;

        assert!(fs::symlink_metadata(installs_dir.join("1.3")).is_err());
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        assert!(is_runtime_symlink(&installs_dir.join("1.2")));
        assert!(is_runtime_symlink(&installs_dir.join("next")));
        assert!(installs_dir.join("1.3.1-3").is_dir());
        Ok(())
    }

    #[test]
    fn prune_stale_generated_symlinks_keeps_links_to_eligible_installs() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(
            &npm_test_backend(),
            &installs_dir,
            &IndexMap::new(),
            &HashSet::new(),
        )?;

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
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        let _interrupted = interrupted_install(&backend, "2.1.0")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("next"))?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(&backend, &installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(is_runtime_symlink(&installs_dir.join("next")));
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        Ok(())
    }

    #[test]
    fn prune_stale_generated_symlinks_keeps_names_this_rebuild_asked_for() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        let _interrupted = interrupted_install(&backend, "2.1.0")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;
        let desired = IndexMap::from([("latest".to_string(), PathBuf::from("./2.1.0"))]);

        prune_stale_generated_symlinks(&backend, &installs_dir, &desired, &HashSet::new())?;

        assert!(is_runtime_symlink(&installs_dir.join("latest")));
        Ok(())
    }

    /// An alias may be named like a version prefix mise also generates.
    #[test]
    fn prune_stale_generated_symlinks_keeps_configured_aliases() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        let _interrupted = interrupted_install(&backend, "2.1.0")?;
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("2"))?;
        let aliases = IndexMap::from([("2".to_string(), "2.1.0".to_string())]);

        prune_stale_generated_symlinks(
            &backend,
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
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        let _interrupted = interrupted_install(&backend, "2.1.0")?;

        let namespace = generated_symlink_namespace(&installs_dir);

        assert!(installed_versions_in_dir(&backend, &installs_dir).is_empty());
        assert!(namespace.contains("2"));
        assert!(namespace.contains("2.1"));
        assert!(namespace.contains("latest"));
        Ok(())
    }

    /// A relative link in a generated name is mise's to manage whoever wrote
    /// it, matching what `rebuild_symlinks_in_dir` already does when it
    /// repoints one. A name mise does not generate, or a link that is not in
    /// `./` form, is left alone.
    #[test]
    fn prune_stale_generated_symlinks_claims_generated_names_whoever_wrote_them() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("2.1.0"))?;
        let _interrupted = interrupted_install(&backend, "2.1.0")?;
        // hand-made, but occupying a name mise generates
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("latest"))?;
        // hand-made, name mise never generates
        make_symlink_or_file(Path::new("./2.1.0"), &installs_dir.join("mine"))?;
        // generated name, but not a `./` link, so not mise's to touch
        make_symlink_or_file(&temp_dir.path().join("elsewhere"), &installs_dir.join("2"))?;

        prune_stale_generated_symlinks(&backend, &installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        assert!(is_runtime_symlink(&installs_dir.join("mine")));
        assert!(fs::symlink_metadata(installs_dir.join("2")).is_ok());
        Ok(())
    }

    /// A `Sub` request is the only other writer of links in this directory, and
    /// it is only in `desired` while the toolset that asked for it is loaded.
    /// Its `sub-…` name is outside the generated namespace, so a rebuild from
    /// an unrelated directory cannot drop another directory's pin.
    #[test]
    fn prune_stale_generated_symlinks_keeps_sub_request_pins() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("19.0.0"))?;
        let _interrupted = interrupted_install(&backend, "19.0.0")?;
        // `node@sub-1:20` resolving to 19.0.0, pinned from some other directory
        make_symlink_or_file(Path::new("./19.0.0"), &installs_dir.join("sub-1-20"))?;
        make_symlink_or_file(Path::new("./19.0.0"), &installs_dir.join("latest"))?;

        prune_stale_generated_symlinks(&backend, &installs_dir, &IndexMap::new(), &HashSet::new())?;

        assert!(is_runtime_symlink(&installs_dir.join("sub-1-20")));
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        Ok(())
    }

    #[test]
    fn generated_names_for_never_produces_a_sub_request_pathname() {
        // `ToolRequest::Sub::version()` is always `sub-{sub}:{orig_version}`,
        // which `runtime_pathname` turns into `sub-{sub}-{orig_version}`.
        for v in ["19.0.0", "20.1.0", "temurin-21.0.1"] {
            assert!(
                !generated_names_for(v).iter().any(|n| n.starts_with("sub-")),
                "{v} generated a name that could collide with a Sub pin"
            );
        }
    }

    #[test]
    fn generated_names_for_matches_the_names_the_rebuild_writes() {
        assert_eq!(generated_names_for("1.3.1"), ["1", "1.3", "latest"]);
        assert_eq!(
            generated_names_for("temurin-21.0.1"),
            ["temurin-21", "temurin-21.0", "temurin-latest"]
        );
    }

    /// The links `rebuild_symlinks_in_dir` writes now: real symlinks on unix and
    /// junctions on Windows. They must be recognised as runtime links, excluded
    /// from the real installs, pruned when stale and removed once their target
    /// is gone.
    #[test]
    fn dir_links_are_runtime_links_that_are_pruned_and_cleaned_up() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let installs_dir = temp_dir.path().join("installs").join("dummy");
        let backend = unique_backend(&temp_dir);
        fs::create_dir_all(installs_dir.join("1.2.4"))?;
        fs::create_dir_all(installs_dir.join("1.3.1-3"))?;
        let _interrupted = interrupted_install(&backend, "1.3.1-3")?;
        file::make_dir_link(Path::new("./1.2.4"), &installs_dir.join("1.2"))?;
        file::make_dir_link(Path::new("./1.3.1-3"), &installs_dir.join("1.3"))?;
        file::make_dir_link(Path::new("./1.2.4"), &installs_dir.join("latest"))?;

        // Recognised, and never mistaken for an installed version.
        assert!(is_runtime_symlink(&installs_dir.join("1.2")));
        assert_eq!(
            runtime_symlink_target(&installs_dir.join("1.2")),
            Some(PathBuf::from("./1.2.4"))
        );
        assert_eq!(
            real_installs_in_dir(&installs_dir)
                .into_iter()
                .collect::<HashSet<_>>(),
            HashSet::from(["1.2.4".to_string(), "1.3.1-3".to_string()])
        );

        // A link into an ineligible (incomplete) install is stale and removed;
        // the others stay.
        prune_stale_generated_symlinks(&backend, &installs_dir, &IndexMap::new(), &HashSet::new())?;
        assert!(fs::symlink_metadata(installs_dir.join("1.3")).is_err());
        assert!(is_runtime_symlink(&installs_dir.join("1.2")));
        assert!(is_runtime_symlink(&installs_dir.join("latest")));
        assert!(installs_dir.join("1.3.1-3").is_dir(), "target must survive");

        // A link whose target was deleted is cleaned up without touching others.
        fs::remove_dir_all(installs_dir.join("1.2.4"))?;
        remove_missing_symlinks_in_dir(&installs_dir)?;
        assert!(fs::symlink_metadata(installs_dir.join("1.2")).is_err());
        assert!(fs::symlink_metadata(installs_dir.join("latest")).is_err());
        assert!(installs_dir.join("1.3.1-3").is_dir());
        Ok(())
    }
}
