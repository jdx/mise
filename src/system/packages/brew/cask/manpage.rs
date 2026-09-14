use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManpageArtifact {
    source: String,
    target: Option<String>,
    glob: bool,
}

#[derive(Debug)]
pub(super) struct ResolvedManpage {
    source: ManpageSource,
    pub(super) binary: BinaryArtifact,
}

#[derive(Debug)]
enum ManpageSource {
    Staged(PathBuf),
    // Keep the effective app target selected before hooks. Resolving APPDIR
    // again after postflight could silently switch to an unrelated app.
    App { bundle: PathBuf, path: PathBuf },
}

pub(super) fn parse_manpage_artifact(value: &Value) -> Result<Option<ManpageArtifact>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let artifact = if let Some(pattern) = object.get("manpage_glob") {
        reject_unsupported_artifact_fields("manpage glob", object, &["manpage_glob"])?;
        ManpageArtifact {
            source: pattern
                .as_str()
                .ok_or_else(|| eyre!("brew-cask: manpage_glob must be a string"))?
                .to_string(),
            target: None,
            glob: true,
        }
    } else if let Some(manpage) = object.get("manpage") {
        reject_unsupported_artifact_fields("manpage", object, &["manpage"])?;
        if let Some(values) = manpage.as_array() {
            if !(1..=2).contains(&values.len()) {
                bail!("brew-cask: manpage requires a source and optional target");
            }
            if let Some(options) = values.get(1) {
                let options = options
                    .as_object()
                    .ok_or_else(|| eyre!("brew-cask: manpage options must be an object"))?;
                reject_unsupported_artifact_fields("manpage", options, &["target"])?;
                if !options.get("target").is_some_and(Value::is_string) {
                    bail!("brew-cask: manpage target must be a string");
                }
            }
        }
        let (source, target) = artifact_source_target(value, manpage)
            .ok_or_else(|| eyre!("brew-cask: manpage requires a source"))?;
        ManpageArtifact {
            source,
            target,
            glob: false,
        }
    } else {
        return Ok(None);
    };
    validate_manpage_source(&artifact.source, artifact.glob)?;
    if !artifact.glob {
        manpage_target(artifact.target.as_deref().unwrap_or(file_name_str(
            Path::new(&artifact.source),
            "manpage source",
        )?))?;
    }
    Ok(Some(artifact))
}

fn validate_manpage_source(source: &str, glob: bool) -> Result<()> {
    let parts = source.split('/').collect::<Vec<_>>();
    if (glob && source.starts_with("$APPDIR/"))
        || source.contains(['\0', '\\', '[', ']', '{', '}'])
        || parts.iter().any(|part| matches!(*part, "" | "." | ".."))
        || source.contains("**")
        || parts
            .iter()
            .enumerate()
            .any(|(index, part)| part.contains(['*', '?']) && (!glob || index != parts.len() - 1))
    {
        bail!(
            "brew-cask: invalid manpage source '{source}'; use a staged relative path with filename-only * or ? globs"
        );
    }
    Ok(())
}

fn manpage_target(name: &str) -> Result<String> {
    if name.contains(['/', '\\', '\0', '*', '?', '[', ']', '{', '}']) {
        bail!("brew-cask: manpage target must be a filename");
    }
    let uncompressed = name.strip_suffix(".gz").unwrap_or(name);
    let (_, section) = uncompressed
        .rsplit_once('.')
        .filter(|(stem, section)| {
            !stem.is_empty() && section.len() == 1 && matches!(section.as_bytes()[0], b'1'..=b'9')
        })
        .ok_or_else(|| {
            eyre!("brew-cask: manpage '{name}' must end in .1 through .9 (optionally .gz)")
        })?;
    Ok(format!("$HOMEBREW_PREFIX/share/man/man{section}/{name}"))
}

impl ManpageArtifact {
    pub(super) fn print_install_plan(&self) -> Result<()> {
        if self.glob {
            miseprintln!(
                "install manpages matching {} (after extraction)",
                self.source
            );
        } else {
            miseprintln!("install manpage {}", self.source);
        }
        Ok(())
    }
}

/// Resolve staged sources before lifecycle or app installation, without suffix
/// searches. APPDIR declarations are mapped now, but read only after app install.
pub(super) fn resolve_manpages(
    stage: &Path,
    cask: &Cask,
    artifacts: &CaskArtifacts,
) -> Result<Vec<ResolvedManpage>> {
    if artifacts.manpages.is_empty() {
        return Ok(Vec::new());
    }
    let root = stage.canonicalize()?;
    let mut resolved = Vec::new();
    for artifact in &artifacts.manpages {
        if artifact.source.starts_with("$APPDIR/") {
            let (bundle, path) = declared_manpage_app_source(&artifact.source, &artifacts.apps)?;
            resolved.push(ResolvedManpage {
                source: ManpageSource::App { bundle, path },
                binary: BinaryArtifact {
                    source: artifact.source.clone(),
                    target: Some(manpage_target(artifact.target.as_deref().unwrap_or(
                        file_name_str(Path::new(&artifact.source), "manpage source")?,
                    ))?),
                },
            });
            continue;
        }
        let source = Path::new(&artifact.source);
        let sources = if artifact.glob {
            let directory = root.join(source.parent().unwrap_or(Path::new("")));
            ensure_manpage_contained(&directory, &root)?;
            let pattern = glob::Pattern::new(file_name_str(source, "manpage glob")?)?;
            let mut matches = Vec::new();
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let name = entry.file_name();
                if pattern.matches_with(
                    &name.to_string_lossy(),
                    glob::MatchOptions {
                        case_sensitive: true,
                        require_literal_separator: true,
                        require_literal_leading_dot: true,
                    },
                ) {
                    matches.push(entry.path());
                }
            }
            matches.sort();
            if matches.is_empty() {
                bail!(
                    "brew-cask: manpage glob '{}' matched no files",
                    artifact.source
                );
            }
            matches
        } else {
            vec![root.join(source)]
        };
        for source in sources {
            ensure_manpage_file(&source, &root)?;
            let target = manpage_target(
                artifact
                    .target
                    .as_deref()
                    .unwrap_or(file_name_str(&source, "manpage source")?),
            )?;
            let binary = BinaryArtifact {
                source: source.strip_prefix(&root)?.to_string_lossy().into_owned(),
                target: Some(target),
            };
            resolved.push(ResolvedManpage {
                source: ManpageSource::Staged(source),
                binary,
            });
        }
    }
    validate_manpage_target_uniqueness(stage, cask, artifacts, &resolved)?;
    Ok(resolved)
}

fn ensure_manpage_contained(path: &Path, root: &Path) -> Result<()> {
    if !path.canonicalize()?.starts_with(root.canonicalize()?) {
        bail!(
            "brew-cask: manpage source '{}' escapes source root '{}'",
            path.display(),
            root.display()
        );
    }
    Ok(())
}

fn ensure_manpage_file(path: &Path, root: &Path) -> Result<()> {
    ensure_manpage_contained(path, root)?;
    if !path.is_file() {
        bail!(
            "brew-cask: manpage source '{}' is not a file",
            path.display()
        );
    }
    Ok(())
}

fn declared_manpage_app_source(source: &str, apps: &[AppArtifact]) -> Result<(PathBuf, PathBuf)> {
    let candidates = appdir_artifact_candidates(source, apps)?;
    match candidates.as_slice() {
        [candidate] => Ok(candidate.clone()),
        [] => bail!("brew-cask: manpage source '{source}' must refer to a declared app"),
        _ => bail!("brew-cask: manpage source '{source}' is ambiguous"),
    }
}

/// Manpages share binary link ownership and receipts, but must not pass through
/// stage_binary: that makes executables and performs a suffix-based source search.
pub(super) fn stage_manpages(
    stage: &Path,
    caskroom: &Path,
    appdir: &Path,
    manpages: &[ResolvedManpage],
    copied_payload_roots: &[PathBuf],
) -> Result<()> {
    // Check the entire batch before copying: later lifecycle-created conflicts
    // must not leave earlier pages installed or destroy postflight output.
    let mut copies = Vec::new();
    for manpage in manpages {
        let source = match &manpage.source {
            ManpageSource::App { bundle, path } => {
                // Validate the original effective parent, without resolving the
                // configured APPDIR again or creating missing directories.
                #[cfg(unix)]
                let _parent = open_trusted_directory(
                    Path::new("/"),
                    bundle.parent().unwrap().strip_prefix("/")?,
                    true,
                    false,
                )?;
                // The installed/adopted bundle is owned by the app artifact. Do
                // not accept a replacement bundle symlink or escaping resource.
                if !bundle.symlink_metadata()?.is_dir() {
                    bail!("brew-cask: manpage app source is not a real bundle directory");
                }
                ensure_manpage_file(path, bundle)?;
                path
            }
            ManpageSource::Staged(path) => {
                ensure_manpage_file(path, stage)?;
                path
            }
        };
        let target = caskroom_binary_path(caskroom, appdir, &manpage.binary)?;
        let replace_payload = match target.symlink_metadata() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Ok(metadata)
                if metadata.is_file()
                    && copied_payload_roots
                        .iter()
                        .any(|root| newly_copied_manpage_target(&target, root)) =>
            {
                true
            }
            Err(err) => {
                return Err(err).wrap_err("brew-cask: cannot inspect manpage staging target");
            }
            Ok(_) => bail!(
                "brew-cask: manpage staging target '{}' already exists",
                target.display()
            ),
        };
        manpage_target_ancestor(&target)?;
        if !path_starts_with_resolved_root(&target, caskroom) {
            bail!("brew-cask: manpage staging target escapes Caskroom");
        }
        copies.push((source, target, replace_payload));
    }
    for (source, target, replace_payload) in copies {
        if replace_payload {
            std::fs::remove_file(&target)?;
        }
        if let Some(parent) = target.parent() {
            file::create_dir_all(parent)?;
        }
        // create_new also refuses a destination symlink introduced since the
        // precheck. Never remove or truncate another artifact's staged output.
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        std::io::copy(&mut std::fs::File::open(source)?, &mut output)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        }
    }
    Ok(())
}

// The caller supplies only roots just created by durabilize_stage_payload,
// with no intervening artifact phase. Do not follow even an in-Caskroom parent
// symlink: it could redirect a copied entry into preexisting postflight output.
fn newly_copied_manpage_target(target: &Path, root: &Path) -> bool {
    if !target.starts_with(root) {
        return false;
    }
    if target == root {
        return true;
    }
    for parent in target.parent().unwrap().ancestors() {
        if !parent
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return false;
        }
        if parent == root {
            return true;
        }
    }
    false
}

pub(super) fn ensure_manpage_targets_replaceable(
    cask: &Cask,
    manpages: &[ResolvedManpage],
) -> Result<()> {
    for manpage in manpages {
        let target = manpage.binary.target_path(&target_app_dir()?)?;
        if !path_starts_with_resolved_root(target.parent().unwrap(), &prefix::prefix()) {
            bail!("brew-cask: manpage target parent escapes Homebrew prefix");
        }
        let metadata = match target.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).wrap_err("brew-cask: failed to inspect manpage target"),
        };
        if metadata.file_type().is_symlink() {
            let resolved = resolve_symlink_target(&target, std::fs::read_link(&target)?);
            if path_starts_with_resolved_root(&resolved, &caskroom_token_dir(&cask.token)) {
                continue;
            }
        }
        bail!(
            "brew-cask: manpage target '{}' already exists and is not owned by cask '{}'",
            target.display(),
            cask.token
        );
    }
    Ok(())
}

/// Check both external destinations and the relocated Caskroom destinations:
/// e.g. /usr/local and the configured prefix may share a Caskroom-relative path.
pub(super) fn validate_manpage_target_uniqueness(
    stage: &Path,
    cask: &Cask,
    artifacts: &CaskArtifacts,
    manpages: &[ResolvedManpage],
) -> Result<()> {
    if manpages.is_empty() {
        return Ok(());
    }
    let appdir = cask_appdir(&artifacts.apps)?;
    let caskroom = caskroom_tmp_dir(cask);
    let completions = artifacts.completion_target_paths(cask)?;
    let mut targets = artifacts.binary_targets()?;
    targets.extend(artifacts.generic_artifact_targets()?);
    targets.extend(artifacts.app_target_paths()?);
    targets.extend(artifacts.font_target_paths()?);
    targets.extend(completions.iter().cloned());
    let mut staged = artifacts
        .binaries
        .iter()
        .map(|binary| caskroom_binary_path(&caskroom, &appdir, binary))
        .collect::<Result<Vec<_>>>()?;
    // The post-copy check can now see the durable binary sources, which may
    // differ from their link destinations. Protect both the source entry and
    // its contained referent: stage_binary chmods through payload symlinks.
    for binary in &artifacts.binaries {
        if let Some(payload) = payload_binary_path(stage, &caskroom, binary) {
            staged.push(payload.canonicalize()?);
            staged.push(payload);
        }
    }
    staged.extend(
        artifacts
            .command_wrappers
            .iter()
            .map(|wrapper| wrapper.caskroom_path(&caskroom)),
    );
    for completion in completions {
        staged.push(caskroom_completion_path(&caskroom, &completion)?);
    }
    for font in &artifacts.fonts {
        staged.push(caskroom_font_path(&caskroom, font)?);
    }
    for app in &artifacts.apps {
        staged.push(caskroom.join(app_bundle_name(app.target_name()?)?));
    }
    for artifact in &artifacts.generic {
        if let Some(source) = find_artifact_matching(stage, &artifact.source, |_| true)
            && let Some(relative) = staged_relative_path(stage, &source)
        {
            staged.push(caskroom.join(relative));
        }
    }
    for manpage in manpages {
        for (target, previous) in [
            (manpage.binary.target_path(&appdir)?, &mut targets),
            (
                caskroom_binary_path(&caskroom, &appdir, &manpage.binary)?,
                &mut staged,
            ),
        ] {
            for other in previous.iter() {
                if manpage_targets_overlap(&target, other)? {
                    bail!(
                        "brew-cask: duplicate manpage target '{}' overlaps '{}'",
                        target.display(),
                        other.display()
                    );
                }
            }
            previous.push(target);
        }
    }
    Ok(())
}

/// Install-only name comparison on the destination filesystem. Scratch
/// directories model only the not-yet-existing suffix; actual targets are never
/// created or truncated. Let the filesystem decide case/Unicode equivalence.
fn manpage_targets_overlap(left: &Path, right: &Path) -> Result<bool> {
    if left.starts_with(right) || right.starts_with(left) {
        return Ok(true);
    }
    let (left_parent, left_suffix) = manpage_target_ancestor(left)?;
    let (right_parent, right_suffix) = manpage_target_ancestor(right)?;
    if !same_file::is_same_file(&left_parent, &right_parent)? {
        // A directory artifact may already exist and contain the other target's
        // parent (including through a symlink/mount). These have different probe
        // roots, but still overlap. Otherwise distinct existing parents cannot
        // name the same new directory entry.
        return Ok(manpage_directory_contains(left, &right_parent)?
            || manpage_directory_contains(right, &left_parent)?);
    }
    let probe = tempfile::Builder::new()
        .prefix(".mise-manpage-targets-")
        .tempdir_in(&left_parent)
        .wrap_err("brew-cask: cannot check manpage target uniqueness on destination filesystem")?;
    let result =
        (|| {
            let left = probe.path().join(left_suffix);
            let right = probe.path().join(right_suffix);
            std::fs::create_dir_all(&left)?;
            std::fs::create_dir_all(&right)?;
            Ok(manpage_directory_contains(&left, &right)?
                || manpage_directory_contains(&right, &left)?)
        })();
    probe
        .close()
        .wrap_err("brew-cask: failed to clean up manpage target probe")?;
    result
}

fn manpage_directory_contains(directory: &Path, path: &Path) -> Result<bool> {
    match std::fs::metadata(directory) {
        Ok(metadata) if !metadata.is_dir() => return Ok(false),
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).wrap_err("brew-cask: cannot inspect manpage target directory"),
    }
    for ancestor in path.ancestors() {
        if same_file::is_same_file(directory, ancestor)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn manpage_target_ancestor(target: &Path) -> Result<(PathBuf, PathBuf)> {
    let parent = target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: invalid manpage target"))?;
    for ancestor in parent.ancestors() {
        match ancestor.canonicalize() {
            Ok(resolved) => {
                if !resolved.is_dir() {
                    bail!("brew-cask: manpage target ancestor is not a directory");
                }
                let suffix = target.strip_prefix(ancestor)?;
                if suffix
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
                {
                    bail!("brew-cask: invalid manpage target suffix");
                }
                return Ok((resolved, suffix.to_path_buf()));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                match ancestor.symlink_metadata() {
                    Ok(_) => bail!("brew-cask: cannot resolve manpage target ancestor"),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(err)
                            .wrap_err("brew-cask: cannot inspect manpage target ancestor");
                    }
                }
            }
            Err(err) => {
                return Err(err).wrap_err("brew-cask: cannot inspect manpage target ancestor");
            }
        }
    }
    bail!("brew-cask: manpage target has no existing directory ancestor")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manpage_target_uniqueness_distinguishes_siblings_and_ancestors() -> Result<()> {
        let root = tempfile::tempdir()?;
        let left = root.path().join("not-created/man1/example.1");
        let right = root.path().join("not-created/man1/different.1");
        assert!(manpage_targets_overlap(&left, &left)?);
        assert!(manpage_targets_overlap(&left, left.parent().unwrap())?);
        assert!(!manpage_targets_overlap(&left, &right)?);
        // No target directories or scratch directories survive validation.
        assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn manpage_target_probe_cleans_up_after_error() -> Result<()> {
        let root = tempfile::tempdir()?;
        let invalid = root.path().join("x".repeat(1024));
        assert!(manpage_targets_overlap(&root.path().join("valid"), &invalid).is_err());
        assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn manpage_target_uniqueness_resolves_existing_parent_aliases() -> Result<()> {
        let root = tempfile::tempdir()?;
        let real = root.path().join("real");
        file::create_dir_all(&real)?;
        let alias = root.path().join("alias");
        file::make_symlink(&real, &alias)?;
        assert!(manpage_targets_overlap(
            &real.join("new/example.1"),
            &alias.join("new/example.1")
        )?);
        assert!(manpage_targets_overlap(
            &alias,
            &real.join("new/example.1")
        )?);
        assert!(!manpage_targets_overlap(
            &real.join("new/example.1"),
            &alias.join("new/different.1")
        )?);
        assert_eq!(std::fs::read_dir(&real)?.count(), 0);
        Ok(())
    }
}
