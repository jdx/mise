use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManpageArtifact {
    source: String,
    target: Option<String>,
    glob: bool,
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
    if source.contains(['\0', '\\', '[', ']', '{', '}'])
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

/// Resolve before lifecycle or app installation, without suffix-search fallbacks.
/// Both the directory and every selected file must remain inside the stage.
pub(super) fn resolve_manpages(
    stage: &Path,
    cask: &Cask,
    artifacts: &CaskArtifacts,
) -> Result<Vec<BinaryArtifact>> {
    if artifacts.manpages.is_empty() {
        return Ok(Vec::new());
    }
    let root = stage.canonicalize()?;
    let mut targets = artifacts
        .binary_targets()?
        .into_iter()
        .collect::<BTreeSet<_>>();
    targets.extend(artifacts.generic_artifact_targets()?);
    targets.extend(artifacts.completion_target_paths(cask)?);
    let appdir = cask_appdir(&artifacts.apps)?;
    let mut resolved = Vec::new();
    for artifact in &artifacts.manpages {
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
            let target = binary.target_path(&appdir)?;
            if !targets.insert(target.clone()) {
                bail!("brew-cask: duplicate manpage target '{}'", target.display());
            }
            resolved.push(binary);
        }
    }
    Ok(resolved)
}

fn ensure_manpage_contained(path: &Path, stage: &Path) -> Result<()> {
    if !path.canonicalize()?.starts_with(stage.canonicalize()?) {
        bail!(
            "brew-cask: manpage source '{}' escapes staged_path",
            path.display()
        );
    }
    Ok(())
}

fn ensure_manpage_file(path: &Path, stage: &Path) -> Result<()> {
    ensure_manpage_contained(path, stage)?;
    if !path.is_file() {
        bail!(
            "brew-cask: manpage source '{}' is not a file",
            path.display()
        );
    }
    Ok(())
}

/// Manpages share binary link ownership and receipts, but must not pass through
/// stage_binary: that makes executables and performs a suffix-based source search.
pub(super) fn stage_manpages(
    stage: &Path,
    caskroom: &Path,
    appdir: &Path,
    manpages: &[BinaryArtifact],
) -> Result<()> {
    for manpage in manpages {
        let source = stage.join(&manpage.source);
        ensure_manpage_file(&source, stage)?;
        let target = caskroom_binary_path(caskroom, appdir, manpage)?;
        if !path_starts_with_resolved_root(&target, caskroom) {
            bail!("brew-cask: manpage staging target escapes Caskroom");
        }
        if let Some(parent) = target.parent() {
            file::create_dir_all(parent)?;
        }
        // Never follow a payload symlink at the destination when copying.
        file::remove_all(&target)?;
        file::copy(&source, &target)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))?;
        }
    }
    Ok(())
}

pub(super) fn ensure_manpage_targets_replaceable(
    cask: &Cask,
    manpages: &[BinaryArtifact],
) -> Result<()> {
    for manpage in manpages {
        let target = manpage.target_path(&target_app_dir()?)?;
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
