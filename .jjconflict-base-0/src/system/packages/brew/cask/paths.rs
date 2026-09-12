use super::*;

pub(super) fn find_app(root: &Path, name: &str) -> Option<PathBuf> {
    // Directory predicate inside the walk so a same-named file cannot shadow.
    find_artifact_matching(root, name, |path| path.is_dir())
}

pub(super) fn find_file_artifact(root: &Path, name: &str) -> Option<PathBuf> {
    find_artifact_matching(root, name, |path| path.is_file())
}

/// Exact path suffix match first, then ASCII case-insensitive suffix (e.g. cask
/// `yaak.app` vs DMG `Yaak.app`). `pred` runs only after a name hit.
pub(super) fn find_artifact_matching(
    root: &Path,
    name: &str,
    pred: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let name_path = Path::new(name);
    let mut case_insensitive = None;
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "__MACOSX")
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        // Cheap path-string checks first; only `stat` via `pred` on name hits
        // (large .app trees have thousands of non-matching entries).
        if relative.ends_with(name_path) {
            if pred(path) {
                return Some(entry.into_path());
            }
        } else if case_insensitive.is_none()
            && path_ends_with_ignore_ascii_case(relative, name_path)
            && pred(path)
        {
            case_insensitive = Some(entry.into_path());
        }
    }
    if let Some(found) = case_insensitive {
        return Some(found);
    }
    // `WalkDir` defaults to `follow_links = false`, so the walk above never
    // descends into a symlink a flight step created: gcloud-cli's last
    // preflight step links `staged_path/google-cloud-sdk` at the SDK it copied
    // into the prefix, and every `binary` beneath it was unreachable. Resolving
    // `name` as an exact path under `root` traverses the link.
    //
    // This only ever fires for that symlinked case. When `root/name` is
    // reachable without traversing a link, its own relative path ends with
    // `name`, so the walk already returned it — which is also why the result is
    // desymlinked: the artifact's real location is what callers need to tell
    // ephemeral stage content apart from a durable directory that outlives the
    // install. Kept as a fallback rather than a fast path so the walk's
    // exact-then-case-insensitive precedence is unchanged on case-insensitive
    // filesystems.
    relative_artifact_path(root, name_path)
        .filter(|path| pred(path))
        .map(|path| file::desymlink_path(&path))
}

/// `name` resolved against `root`, or `None` when `name` cannot be interpreted
/// as a path contained by `root`.
pub(super) fn relative_artifact_path(root: &Path, name: &Path) -> Option<PathBuf> {
    if name.is_absolute() {
        return None;
    }
    let mut named = false;
    for component in name.components() {
        match component {
            // `.` alone would resolve to `root` itself, and `install_app` would
            // then take the whole extraction root as the bundle.
            Component::Normal(component) if component != "__MACOSX" => named = true,
            // The walk skips `__MACOSX` resource-fork copies; an exact-path hit
            // must not reintroduce them.
            Component::Normal(_) => return None,
            Component::CurDir => {}
            _ => return None,
        }
    }
    named.then(|| root.join(name))
}

/// True when `path`'s trailing components match `suffix` with ASCII
/// case-insensitive comparison of normal path components.
pub(super) fn path_ends_with_ignore_ascii_case(path: &Path, suffix: &Path) -> bool {
    if suffix.as_os_str().is_empty() {
        return false;
    }
    let mut path_iter = path.components().rev();
    for b in suffix.components().rev() {
        let Some(a) = path_iter.next() else {
            return false;
        };
        let matches = match (a, b) {
            (Component::Normal(a), Component::Normal(b)) => match (a.to_str(), b.to_str()) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                _ => a == b,
            },
            _ => a == b,
        };
        if !matches {
            return false;
        }
    }
    true
}

pub(super) fn app_target_path(target_name: &str) -> Result<PathBuf> {
    let app_dir = target_app_dir()?;
    if target_name.contains('\0') {
        bail!("brew-cask: app target contains NUL");
    }
    if target_name.contains('/') {
        let target = target_name.replace("$HOMEBREW_PREFIX", &prefix::prefix().to_string_lossy());
        let path = PathBuf::from(target);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            bail!("brew-cask: app target '{target_name}' must not contain '..'");
        }
        if path.is_absolute() {
            let prefix_app_dir = prefix::prefix().join("Applications");
            if path.starts_with(&app_dir) || path.starts_with(&prefix_app_dir) {
                return Ok(path);
            }
            // Casks routinely hardcode an absolute `/Applications/Foo.app`
            // target. When an override appdir is configured, relocate such a
            // target into it (preserving any subdirectories) rather than
            // rejecting it. `$HOMEBREW_PREFIX`-anchored targets are handled by
            // the check above and are never relocated.
            if app_dir != Path::new(DEFAULT_APP_DIR)
                && let Ok(rest) = path.strip_prefix(DEFAULT_APP_DIR)
            {
                return Ok(app_dir.join(rest));
            }
            bail!(
                "brew-cask: app target '{target_name}' must be under {}",
                app_dir.display()
            );
        }
        bail!("brew-cask: app target '{target_name}' must be an absolute path");
    }
    Ok(app_dir.join(target_name))
}

/// The directory `app` artifacts are linked into: `/Applications` unless
/// [`APP_DIR_ENV`] overrides it.
///
/// The override is validated here rather than at the point of use because
/// `app_target_path` treats the result as a containment boundary for symlinks
/// that may be created with elevated privileges. An empty value falls back to
/// the default so that exporting `MISE_BREW_CASK_OPT_APPDIR=` cannot disable
/// that boundary: `Path::starts_with("")` is true for every path.
pub(super) fn target_app_dir() -> Result<PathBuf> {
    let Ok(dir) = crate::env::var(APP_DIR_ENV) else {
        return Ok(PathBuf::from(DEFAULT_APP_DIR));
    };
    if dir.is_empty() {
        return Ok(PathBuf::from(DEFAULT_APP_DIR));
    }
    let dir = PathBuf::from(dir);
    if !dir.is_absolute() {
        bail!(
            "brew-cask: {APP_DIR_ENV} '{}' must be an absolute path",
            dir.display()
        );
    }
    if dir
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: {APP_DIR_ENV} '{}' must not contain '..'",
            dir.display()
        );
    }
    // Resolve the override to a real absolute path: canonicalize its longest
    // existing prefix and re-append the components that do not exist yet. This
    // makes the appdir a symlink-free containment boundary — privileged cask
    // mutations then operate on resolved paths and cannot be redirected through
    // a symlinked component — and it collapses every spelling of the filesystem
    // root (`/`, `//`, `/.`, a symlink to `/`, ...) to `/` so they can all be
    // rejected together.
    let resolved = resolve_appdir(&dir);
    if !resolved
        .components()
        .any(|component| matches!(component, Component::Normal(_)))
    {
        bail!(
            "brew-cask: {APP_DIR_ENV} '{}' must not resolve to the filesystem root",
            dir.display()
        );
    }
    Ok(resolved)
}

/// Resolve `dir` by canonicalizing its longest existing ancestor and
/// re-appending the not-yet-existing tail. Symlinks in the existing portion are
/// followed, so the result is a real path the caller can safely use as a
/// containment boundary. Falls back to `dir` unchanged if nothing along the
/// path can be canonicalized (not expected for an absolute path, where `/`
/// always resolves).
pub(super) fn resolve_appdir(dir: &Path) -> PathBuf {
    for ancestor in dir.ancestors() {
        if let Ok(real) = ancestor.canonicalize() {
            let tail = dir.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return real.join(tail);
        }
    }
    dir.to_path_buf()
}

/// `kind` names what the path is, so the error reads e.g. "invalid app target".
pub(super) fn file_name_str<'a>(path: &'a Path, kind: &str) -> Result<&'a str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("brew-cask: invalid {kind} '{}'", path.display()))
}

pub(super) fn app_bundle_name(target_name: &str) -> Result<&str> {
    file_name_str(Path::new(target_name), "app target")
}

/// Roots that a cask's `binary` artifact may legitimately symlink into.
///
/// The Homebrew prefix (`/opt/homebrew` on arm64, `/usr/local` on Intel) is
/// always allowed. `/usr/local` is additionally allowed even on arm64 because
/// some casks (e.g. docker-desktop) hardcode absolute `/usr/local/bin` targets
/// so their CLIs land on PATH regardless of architecture. Homebrew honors those
/// targets, so mise does too.
pub(super) fn allowed_binary_target_roots() -> Vec<PathBuf> {
    let prefix = prefix::prefix();
    let mut roots = vec![prefix.clone()];
    let usr_local = PathBuf::from("/usr/local");
    if prefix != usr_local {
        roots.push(usr_local);
    }
    roots
}

pub(super) fn allowed_appdir_roots() -> Result<Vec<PathBuf>> {
    let mut roots = vec![PathBuf::from(DEFAULT_APP_DIR)];
    for root in [target_app_dir()?, prefix::prefix().join("Applications")] {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    Ok(roots)
}

pub(super) fn is_appdir_binary_target(target_name: &str) -> bool {
    target_name.starts_with("$APPDIR/")
}

pub(super) fn allowed_binary_target_roots_display(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(" or ")
}

pub(super) fn binary_target_path(target_name: &str, appdir: &Path) -> Result<PathBuf> {
    if target_name.contains('\0') {
        bail!("brew-cask: binary target contains NUL");
    }
    if let Some(relative) = target_name.strip_prefix("$APPDIR/") {
        let relative = Path::new(relative);
        reject_appdir_escape(relative, "binary $APPDIR target", target_name)?;
        if !allowed_appdir_roots()?.iter().any(|root| root == appdir) {
            bail!("brew-cask: invalid appdir '{}'", appdir.display());
        }
        return Ok(appdir.join(relative));
    }
    if target_name.contains("$APPDIR") {
        bail!("brew-cask: $APPDIR must prefix a binary target");
    }
    let prefix = prefix::prefix();
    let prefix_str = prefix.to_string_lossy();
    let target_name = target_name.replace("$HOMEBREW_PREFIX", prefix_str.as_ref());
    let path = PathBuf::from(&target_name);
    let target = if path.is_absolute() {
        path
    } else if target_name.contains('/') {
        prefix.join(path)
    } else {
        prefix.join("bin").join(path)
    };
    if target
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: binary target '{}' must not contain '..'",
            target.display()
        );
    }
    let roots = allowed_binary_target_roots();
    if !roots.iter().any(|root| target.starts_with(root)) {
        bail!(
            "brew-cask: binary target '{}' must be under {}",
            target.display(),
            allowed_binary_target_roots_display(&roots)
        );
    }
    Ok(target)
}
