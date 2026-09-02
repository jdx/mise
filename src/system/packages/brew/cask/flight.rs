use super::*;

#[cfg(test)]
pub(super) fn execute_flight_steps(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
) -> Result<()> {
    let mut targets = FlightTargetTransaction::default();
    execute_flight_steps_with_completion(
        cask,
        steps,
        staged_path,
        appdir,
        kind,
        &mut targets,
        |_, _| Ok(()),
    )?;
    targets.commit()
}

pub(super) fn execute_flight_steps_recording(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    journal: &mut CaskTransactionJournal<'_>,
    targets: &mut FlightTargetTransaction,
) -> Result<()> {
    execute_flight_steps_with_completion(
        cask,
        steps,
        staged_path,
        appdir,
        kind,
        targets,
        |index, step| record_cask_action(journal, &format!("{kind}[{index}]:{}", step.kind())),
    )
}

pub(super) fn execute_flight_steps_with_completion(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    targets: &mut FlightTargetTransaction,
    mut completed: impl FnMut(usize, &FlightStep) -> Result<()>,
) -> Result<()> {
    for (index, step) in steps.iter().enumerate() {
        execute_flight_step(cask, step, staged_path, appdir, targets).wrap_err_with(|| {
            format!("brew-cask:{}: failed to run structured {kind}", cask.token)
        })?;
        completed(index, step)?;
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(super) struct FlightTargetTransaction {
    pub(super) backups: Vec<ArtifactLinkBackup>,
    pub(super) receipt_caskroom: Option<PathBuf>,
    pub(super) installed: Vec<PathBuf>,
    pub(super) uninstall: BTreeMap<PathBuf, bool>,
    pub(super) previous_symlinks: BTreeSet<PathBuf>,
    pub(super) copied_files: BTreeSet<PathBuf>,
    pub(super) previous_directories: BTreeSet<PathBuf>,
    pub(super) installed_directories: Vec<PathBuf>,
    pub(super) committed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct FlightRecoveryRecord {
    pub(super) target: PathBuf,
    #[serde(default)]
    pub(super) backup: Option<PathBuf>,
    pub(super) target_parent: PathBuf,
    #[serde(default)]
    pub(super) backup_parent: Option<PathBuf>,
    #[serde(default)]
    pub(super) receipt_caskroom: Option<PathBuf>,
    #[serde(default = "default_elevate_recovery")]
    pub(super) elevate: bool,
}

pub(super) fn default_elevate_recovery() -> bool {
    true
}

impl FlightTargetTransaction {
    pub(super) fn protect(&mut self, target: &Path) -> Result<()> {
        self.protect_with_elevation(target, true)
    }

    pub(super) fn protect_unprivileged(&mut self, target: &Path) -> Result<()> {
        self.protect_with_elevation(target, false)
    }

    pub(super) fn protect_generic(&mut self, target: &Path) -> Result<Option<PathBuf>> {
        #[cfg(unix)]
        {
            match open_trusted_operation_parent(target, true, true) {
                Ok(parent) if trusted_parent_is_writable(&parent)? => {
                    self.protect_unprivileged(target)?;
                    Ok(None)
                }
                Ok(_) => {
                    let target = ensure_strict_elevated_target(target)?;
                    self.protect(&target)?;
                    Ok(Some(target))
                }
                Err(err) if is_permission_denied(&err) => {
                    let target = ensure_strict_elevated_target(target)?;
                    self.protect(&target)?;
                    Ok(Some(target))
                }
                Err(err) => Err(err),
            }
        }
        #[cfg(not(unix))]
        {
            self.protect(target)?;
            Ok(None)
        }
    }

    pub(super) fn protect_with_elevation(&mut self, target: &Path, elevate: bool) -> Result<()> {
        if self.backups.iter().any(|entry| entry.target == target) {
            return Ok(());
        }
        ensure_no_unresolved_flight_recovery(target)?;
        let target_parent = resolved_parent(target)?;
        let backup = if target.symlink_metadata().is_ok() {
            let parent = flight_backup_parent(target)?;
            let backup = unused_flight_backup_path(parent, target)?;
            let recovery = flight_backup_recovery_path(&backup);
            let record = FlightRecoveryRecord {
                target: target.to_path_buf(),
                backup: Some(backup.clone()),
                target_parent: target_parent.clone(),
                backup_parent: Some(resolved_parent(&backup)?),
                receipt_caskroom: self.receipt_caskroom.clone(),
                elevate,
            };
            write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;
            let rename = if elevate {
                rename_elevating(target, &backup)
            } else {
                rename_trusted_generic_target(target, &backup, &target_parent)
            };
            if let Err(err) = rename {
                let _ = file::remove_all(&recovery);
                return Err(err);
            }
            Some(backup)
        } else {
            let recovery = flight_absent_recovery_path(target);
            let record = FlightRecoveryRecord {
                target: target.to_path_buf(),
                backup: None,
                target_parent: target_parent.clone(),
                backup_parent: None,
                receipt_caskroom: self.receipt_caskroom.clone(),
                elevate,
            };
            write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;
            None
        };
        let backup_parent = backup.as_deref().map(resolved_parent).transpose()?;
        self.backups.push(ArtifactLinkBackup {
            target: target.to_path_buf(),
            backup,
            target_parent,
            backup_parent,
            elevate,
        });
        Ok(())
    }

    pub(super) fn record_installed(&mut self, target: PathBuf) {
        if !self.installed.contains(&target) {
            self.installed.push(target);
        }
    }

    pub(super) fn record_installed_flight(&mut self, target: PathBuf, uninstall: bool) {
        self.record_installed(target.clone());
        self.uninstall.insert(target, uninstall);
    }

    pub(super) fn installed_targets(&self) -> &[PathBuf] {
        &self.installed
    }

    pub(super) fn uninstall_targets(&self) -> &BTreeMap<PathBuf, bool> {
        &self.uninstall
    }

    pub(super) fn record_copied_files(&mut self, source: &Path, target: &Path) -> Result<()> {
        let metadata = source.symlink_metadata()?;
        if metadata.is_file() {
            self.copied_files.insert(file::desymlink_path(target));
            return Ok(());
        }
        for entry in WalkDir::new(source).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(source)?;
                self.copied_files
                    .insert(file::desymlink_path(&target.join(relative)));
            }
        }
        Ok(())
    }

    pub(super) fn copied_files(&self) -> &BTreeSet<PathBuf> {
        &self.copied_files
    }

    pub(super) fn record_installed_directory(&mut self, target: PathBuf) {
        if !self.installed_directories.contains(&target) {
            self.installed_directories.push(target);
        }
    }

    pub(super) fn installed_directories(&self) -> &[PathBuf] {
        &self.installed_directories
    }

    pub(super) fn rollback(&mut self) -> Result<()> {
        let mut first_error = None;
        let mut failed = Vec::new();
        for entry in std::mem::take(&mut self.backups).into_iter().rev() {
            if let Err(err) = validate_backup_parents(&entry) {
                first_error.get_or_insert(err);
                failed.push(entry);
                continue;
            }
            let remove = if entry.elevate {
                remove_artifact_target_elevating(&entry.target)
            } else {
                remove_trusted_generic_target_from(&entry.target, &entry.target_parent)
            };
            if let Err(err) = remove {
                first_error.get_or_insert(err);
                failed.push(entry);
                continue;
            }
            if let Some(backup) = &entry.backup {
                let rename = if entry.elevate {
                    rename_elevating(backup, &entry.target)
                } else {
                    rename_trusted_generic_target(backup, &entry.target, &entry.target_parent)
                };
                if let Err(err) = rename {
                    first_error.get_or_insert(err);
                    failed.push(entry);
                    continue;
                }
            }
            if let Err(err) = file::remove_all(flight_target_recovery_path(&entry)) {
                warn!("brew-cask: failed to remove flight recovery record: {err:#}");
            }
        }
        failed.reverse();
        self.backups = failed;
        self.installed.clear();
        self.uninstall.clear();
        self.copied_files.clear();
        self.installed_directories.clear();
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    pub(super) fn commit(&mut self) -> Result<()> {
        self.committed = true;
        let backups = std::mem::take(&mut self.backups);
        let mut first_error = None;
        for entry in backups {
            if let Err(err) = file::remove_all(flight_target_recovery_path(&entry)) {
                first_error.get_or_insert(err);
            }
            if let Some(backup) = &entry.backup {
                // Attempt both removals independently. Either a missing record
                // or a missing backup is enough to prevent stale recovery from
                // restoring pre-install data over a later target.
                let remove = if entry.elevate {
                    remove_artifact_target_elevating(backup)
                } else {
                    entry
                        .backup_parent
                        .as_ref()
                        .ok_or_else(|| {
                            eyre!(
                                "brew-cask: flight backup {} has no recorded parent",
                                backup.display()
                            )
                        })
                        .and_then(|parent| remove_trusted_generic_target_from(backup, parent))
                };
                if let Err(err) = remove {
                    first_error.get_or_insert(err);
                }
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }
}

pub(super) fn unused_flight_backup_path(parent: &Path, target: &Path) -> Result<PathBuf> {
    let stem = format!(
        ".mise-flight-backup-{}-{}",
        hash::hash_to_str(&target.display().to_string()),
        std::process::id()
    );
    for attempt in 0_u64.. {
        let backup = parent.join(format!("{stem}-{attempt}"));
        let recovery = flight_backup_recovery_path(&backup);
        let backup_missing = match backup.symlink_metadata() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Ok(_) => false,
            Err(err) => return Err(err.into()),
        };
        let recovery_missing = match recovery.symlink_metadata() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Ok(_) => false,
            Err(err) => return Err(err.into()),
        };
        if backup_missing && recovery_missing {
            return Ok(backup);
        }
    }
    unreachable!("the flight backup suffix space is exhausted")
}

pub(super) fn flight_backup_recovery_path(backup: &Path) -> PathBuf {
    flight_recovery_root().join(format!(
        "{}.recovery",
        hash::hash_to_str(&backup.display().to_string())
    ))
}

pub(super) fn flight_absent_recovery_path(target: &Path) -> PathBuf {
    flight_recovery_root().join(format!(
        "absent-{}.recovery",
        hash::hash_to_str(&target.display().to_string())
    ))
}

pub(super) fn flight_target_recovery_path(entry: &ArtifactLinkBackup) -> PathBuf {
    entry
        .backup
        .as_deref()
        .map(flight_backup_recovery_path)
        .unwrap_or_else(|| flight_absent_recovery_path(&entry.target))
}

pub(super) fn flight_recovery_root() -> PathBuf {
    crate::dirs::STATE.join("brew-cask").join("flight-recovery")
}

pub(super) fn ensure_no_unresolved_flight_recovery(target: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(flight_recovery_root()) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "recovery")
        {
            continue;
        }
        let Ok(body) = file::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<FlightRecoveryRecord>(&body) else {
            continue;
        };
        if record.target == target {
            if let Some(backup) = record.backup {
                bail!(
                    "brew-cask: unresolved recovery for {} still preserves its original at {}",
                    target.display(),
                    backup.display()
                );
            }
            bail!(
                "brew-cask: unresolved recovery for newly created target {}",
                target.display()
            );
        }
    }
    Ok(())
}

pub(super) fn recover_flight_backups() -> Result<()> {
    let root = flight_recovery_root();
    recover_flight_backups_in(&root)
}

pub(super) fn recover_flight_backups_in(root: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(());
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                warn!(
                    "brew-cask: failed to inspect a flight recovery entry in {}: {err:#}",
                    root.display()
                );
                continue;
            }
        };
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("recovery") => recover_flight_backup_or_warn(&path),
            // Atomic record writes may leave their temporary file behind if
            // the process dies before rename. It is not a recovery record.
            Some("tmp") => {
                if let Err(err) = file::remove_all(&path) {
                    warn!(
                        "brew-cask: failed to remove stale flight recovery file {}: {err:#}",
                        path.display()
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn recover_flight_backup_or_warn(path: &Path) {
    if let Err(err) = recover_flight_backup(path) {
        warn!(
            "brew-cask: leaving flight recovery record {} for a later retry: {err:#}",
            path.display()
        );
    }
}

pub(super) fn recover_flight_backup(path: &Path) -> Result<()> {
    let body = file::read_to_string(path)
        .wrap_err_with(|| format!("failed to read flight recovery record {}", path.display()))?;
    let record: FlightRecoveryRecord = serde_json::from_str(&body)
        .wrap_err_with(|| format!("invalid flight recovery record {}", path.display()))?;
    let backup = ArtifactLinkBackup {
        target: record.target,
        backup: record.backup,
        target_parent: record.target_parent,
        backup_parent: record.backup_parent,
        elevate: record.elevate,
    };
    validate_backup_parents(&backup)?;
    if let Some(backup_path) = &backup.backup {
        if backup_path.symlink_metadata().is_ok() {
            if backup.target.symlink_metadata().is_ok() {
                // A target created after the interrupted transaction may be user
                // data, a successfully activated replacement, or a replacement
                // that rollback failed to remove. Without enough information to
                // distinguish those cases, preserve both entries and leave the
                // original backup available for manual recovery.
                warn!(
                    "brew-cask: preserving interrupted flight target {} and its original backup {}",
                    backup.target.display(),
                    backup_path.display()
                );
                return Ok(());
            } else if backup.elevate {
                rename_elevating(backup_path, &backup.target)?;
            } else {
                rename_trusted_generic_target(backup_path, &backup.target, &backup.target_parent)?;
            }
        }
    } else if backup.target.symlink_metadata().is_ok()
        && !flight_target_claimed_by_receipt(&backup.target, record.receipt_caskroom.as_deref())?
    {
        if backup.elevate {
            remove_artifact_target_elevating(&backup.target)?;
        } else {
            remove_trusted_generic_target_from(&backup.target, &backup.target_parent)?;
        }
    }
    file::remove_all(path)?;
    Ok(())
}

pub(super) fn flight_target_claimed_by_receipt(
    target: &Path,
    caskroom: Option<&Path>,
) -> Result<bool> {
    let Some(caskroom) = caskroom else {
        return Ok(false);
    };
    let Some(receipt) = read_receipt(caskroom)? else {
        return Ok(false);
    };
    if receipt.flight_directories.iter().any(|path| path == target) && target.is_dir() {
        return Ok(true);
    }
    for record in receipt
        .targets
        .iter()
        .filter(|record| record.path == target)
    {
        if cask_target_record_matches(record)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn resolved_parent(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("brew-cask: flight target has no parent"))?;
    Ok(path_with_resolved_existing_ancestor(parent))
}

pub(super) fn validate_backup_parents(entry: &ArtifactLinkBackup) -> Result<()> {
    let backup_parent_matches = match (&entry.backup, &entry.backup_parent) {
        (Some(backup), Some(expected)) => {
            resolved_parent(backup).is_ok_and(|current| current == *expected)
        }
        (None, None) => true,
        _ => false,
    };
    if resolved_parent(&entry.target)? != entry.target_parent || !backup_parent_matches {
        bail!(
            "brew-cask: refusing to restore flight target through a changed parent: {}",
            entry.target.display()
        );
    }
    Ok(())
}

pub(super) fn flight_backup_parent(target: &Path) -> Result<&Path> {
    if let Some(app) = target.ancestors().find(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    }) {
        return app
            .parent()
            .ok_or_else(|| eyre!("brew-cask: app flight target has no parent"));
    }
    target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: flight target has no parent"))
}

impl Drop for FlightTargetTransaction {
    fn drop(&mut self) {
        if !self.committed
            && let Err(err) = self.rollback()
        {
            warn!("brew-cask: failed to roll back flight targets: {err:#}");
        }
    }
}

impl FlightStep {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::Copy { .. } => "copy",
            Self::Symlink { .. } => "symlink",
            Self::Run { .. } => "run",
            Self::TerminateProcess { .. } => "terminate_process",
        }
    }
}

pub(super) fn execute_flight_step(
    cask: &Cask,
    step: &FlightStep,
    staged_path: &Path,
    appdir: &Path,
    targets: &mut FlightTargetTransaction,
) -> Result<()> {
    match step {
        FlightStep::Move {
            source,
            target,
            source_glob,
        } => {
            let sources = flight_sources(staged_path, source, *source_glob)?;
            let target = resolve_flight_path(staged_path, target)?;
            if sources.len() > 1 && !target.is_dir() {
                bail!(
                    "brew-cask: structured move with multiple sources requires a directory target"
                );
            }
            for source in sources {
                let target = if target.is_dir() {
                    target.join(source.file_name().ok_or_else(|| {
                        eyre!(
                            "brew-cask: structured move source '{}' has no file name",
                            source.display()
                        )
                    })?)
                } else {
                    target.clone()
                };
                if let Some(parent) = target.parent()
                    && !parent.as_os_str().is_empty()
                {
                    file::create_dir_all(parent)?;
                }
                file::remove_all(&target)?;
                file::rename(&source, &target)?;
            }
        }
        FlightStep::Remove { paths, recursive } => {
            for path in paths {
                for path in flight_paths(staged_path, path)? {
                    if *recursive {
                        file::remove_all(&path)?;
                    } else if path.symlink_metadata().is_ok() {
                        file::remove_file_or_dir(&path)?;
                    }
                }
            }
        }
        FlightStep::Copy {
            source,
            target,
            recursive,
            overwrite,
            source_glob,
            guards,
        } => {
            if !flight_guards_pass(cask, guards, staged_path, appdir)? {
                return Ok(());
            }
            let sources = flight_symlink_sources(cask, source, *source_glob, staged_path, appdir)?;
            let [source] = sources.as_slice() else {
                bail!("brew-cask: structured copy source must resolve to exactly one path");
            };
            if !source.exists() {
                bail!(
                    "brew-cask: structured copy source '{}' was not found",
                    source.display()
                );
            }
            if source.is_dir() && !recursive {
                bail!("brew-cask: structured directory copy requires recursive=true");
            }
            let target = resolve_flight_path_with_context(cask, target, staged_path, appdir)?;
            let external = !target.starts_with(staged_path);
            let target_metadata = target.symlink_metadata().ok();
            if target_metadata.is_some() {
                if !overwrite {
                    bail!(
                        "brew-cask: structured copy target '{}' already exists",
                        target.display()
                    );
                }
                if external {
                    targets.protect(&target)?;
                } else {
                    file::remove_all(&target)?;
                }
            }
            if let Some(parent) = target.parent() {
                create_dir_all_elevating(parent)?;
            }
            if external && target_metadata.is_none() {
                // Bind an absent target to its resolved parent only after
                // creating that parent so rollback can validate its identity.
                targets.protect(&target)?;
            }
            copy_cask_artifact(source, &target)?;
            if external {
                targets.record_copied_files(source, &target)?;
            }
            // External copy trees may be modified during normal use. The
            // transaction backup is sufficient for rollback; recording them
            // would fingerprint their contents and force later reinstalls.
        }
        FlightStep::Symlink {
            source,
            target,
            force,
            uninstall,
            source_glob,
            sudo,
            guards,
        } => {
            if !flight_guards_pass(cask, guards, staged_path, appdir)? {
                return Ok(());
            }
            let target = resolve_flight_path_with_context(cask, target, staged_path, appdir)?;
            let sources = flight_symlink_sources(cask, source, *source_glob, staged_path, appdir)?;
            if sources.is_empty() {
                bail!(
                    "brew-cask: structured symlink source '{}' did not match any paths",
                    source.path
                );
            }
            let target_metadata = target.symlink_metadata().ok();
            let target_is_real_dir = target_metadata
                .as_ref()
                .is_some_and(|metadata| metadata.is_dir());
            let target_is_dir = target_is_real_dir || sources.len() > 1;
            if sources.len() > 1 {
                let created_external_directory =
                    target_metadata.is_none() && !target.starts_with(staged_path);
                if target_metadata.is_some() && !target_is_real_dir {
                    if target.exists() && !force && !targets.previous_symlinks.contains(&target) {
                        bail!(
                            "brew-cask: structured symlink target '{}' already exists",
                            target.display()
                        );
                    }
                    targets.protect(&target)?;
                } else if created_external_directory {
                    // Record the absent directory itself so rollback removes
                    // the container after removing the links created below.
                    if let Some(parent) = target.parent() {
                        create_flight_dir_all(parent, *sudo)?;
                    }
                    targets.protect(&target)?;
                }
                create_flight_dir_all(&target, *sudo)?;
                if created_external_directory || targets.previous_directories.contains(&target) {
                    targets.record_installed_directory(target.clone());
                }
            }
            for source in sources {
                let link = if target_is_dir {
                    let source_name = Path::new(&source).file_name().ok_or_else(|| {
                        eyre!("brew-cask: structured symlink source has no file name")
                    })?;
                    target.join(source_name)
                } else {
                    target.clone()
                };
                let external = !link.starts_with(staged_path);
                let link_metadata = link.symlink_metadata().ok();
                if let Some(metadata) = &link_metadata {
                    if metadata.is_dir() {
                        bail!(
                            "brew-cask: refusing to replace structured symlink directory '{}'",
                            link.display()
                        );
                    }
                    if link.exists() && !force && !targets.previous_symlinks.contains(&link) {
                        bail!(
                            "brew-cask: structured symlink target '{}' already exists",
                            link.display()
                        );
                    }
                    if external {
                        targets.protect(&link)?;
                    } else if metadata.file_type().is_symlink() {
                        file::remove_file(&link)?;
                    } else {
                        file::remove_all(&link)?;
                    }
                }
                if let Some(parent) = link.parent() {
                    create_flight_dir_all(parent, *sudo)?;
                }
                if external && link_metadata.is_none() {
                    // Bind an absent target to its resolved parent only after
                    // creating that parent; otherwise rollback observes a
                    // different path identity and leaves the new link behind.
                    targets.protect(&link)?;
                }
                let source =
                    durable_internal_symlink_source(staged_path, &source, &link).unwrap_or(source);
                create_flight_symlink(&source, &link, *sudo)?;
                if external {
                    targets.record_installed_flight(link, *uninstall);
                }
            }
        }
        FlightStep::Run {
            command,
            args,
            env,
            sudo,
            guards,
        } => {
            if !flight_guards_pass(cask, guards, staged_path, appdir)? {
                return Ok(());
            }
            let command = resolve_flight_path_with_context(cask, command, staged_path, appdir)?;
            let command =
                expand_flight_template(cask, &command.to_string_lossy(), staged_path, appdir);
            let args = args
                .iter()
                .map(|arg| expand_flight_template(cask, arg, staged_path, appdir))
                .collect::<Vec<_>>();
            let env = env
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        expand_flight_template(cask, value, staged_path, appdir),
                    )
                })
                .collect::<Vec<_>>();
            if *sudo {
                sudo::run(&command, &args, &env)?;
            } else {
                let mut runner = CmdLineRunner::new(&command);
                for arg in &args {
                    runner = runner.arg(arg);
                }
                for (key, value) in &env {
                    runner = runner.env(key, value);
                }
                runner.raw(true).execute()?;
            }
        }
        FlightStep::TerminateProcess { .. } => {
            execute_terminate_process(
                step,
                staged_path,
                appdir,
                &cask.version,
                |command, args, sudo| {
                    if sudo {
                        sudo::run(&command.to_string_lossy(), args, &[])
                    } else {
                        let mut runner = CmdLineRunner::new(command);
                        for arg in args {
                            runner = runner.arg(arg);
                        }
                        runner.raw(true).execute()
                    }
                },
                std::thread::sleep,
            )?;
        }
    }
    Ok(())
}

pub(super) fn execute_terminate_process(
    step: &FlightStep,
    staged_path: &Path,
    appdir: &Path,
    version: &str,
    mut run: impl FnMut(&Path, &[String], bool) -> Result<()>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<()> {
    let FlightStep::TerminateProcess {
        name,
        match_mode,
        sudo,
        attempts,
        must_succeed,
        notices,
        failure_message,
    } = step
    else {
        bail!("brew-cask: internal non-terminate flight step");
    };
    let expand = |value: &str| expand_cask_template(value, staged_path, appdir, Some(version));
    for notice in notices {
        miseprintln!("{}", expand(notice));
    }
    let name = expand(name);
    let (command, args) = match *match_mode {
        ProcessMatch::Name => (Path::new("/usr/bin/killall"), vec![name]),
        ProcessMatch::Full => (Path::new("/usr/bin/pkill"), vec!["-f".to_string(), name]),
    };
    let mut last_error = None;
    for attempt in 0..*attempts {
        match run(command, &args, *sudo) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
        if attempt + 1 < *attempts {
            sleep(std::time::Duration::from_secs(1));
        }
    }
    if let Some(message) = failure_message.as_deref() {
        warn!("{}", expand(message));
    }
    if *must_succeed {
        return Err(last_error.unwrap_or_else(|| eyre!("failed to terminate process")));
    }
    Ok(())
}

/// Every guard is evaluated, so a failure in a later guard still surfaces.
pub(super) fn flight_guards_pass(
    cask: &Cask,
    guards: &[FlightGuard],
    staged_path: &Path,
    appdir: &Path,
) -> Result<bool> {
    Ok(guards
        .iter()
        .map(|guard| flight_guard_matches(cask, guard, staged_path, appdir))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .all(|matches| matches))
}

pub(super) fn flight_guard_matches(
    cask: &Cask,
    guard: &FlightGuard,
    staged_path: &Path,
    appdir: &Path,
) -> Result<bool> {
    match guard {
        FlightGuard::OnMacos => Ok(cfg!(target_os = "macos")),
        FlightGuard::OnLinux => Ok(cfg!(target_os = "linux")),
        FlightGuard::IfExists(path) => {
            Ok(resolve_flight_path_with_context(cask, path, staged_path, appdir)?.exists())
        }
        FlightGuard::UnlessExists(path) => {
            Ok(!resolve_flight_path_with_context(cask, path, staged_path, appdir)?.exists())
        }
    }
}

pub(super) fn flight_symlink_sources(
    cask: &Cask,
    source: &FlightPath,
    source_glob: bool,
    staged_path: &Path,
    appdir: &Path,
) -> Result<Vec<PathBuf>> {
    if source_glob {
        if source.base != FlightPathBase::StagedPath {
            bail!("brew-cask: structured symlink globs must use staged_path");
        }
        let pattern = expand_flight_template(cask, &source.path, staged_path, appdir);
        return expand_staged_glob(staged_path, &pattern);
    }
    Ok(vec![resolve_flight_path_with_context(
        cask,
        source,
        staged_path,
        appdir,
    )?])
}

pub(super) fn create_flight_symlink(source: &Path, target: &Path, sudo: FlightSudo) -> Result<()> {
    match sudo {
        FlightSudo::Never => file::make_symlink(source, target).map(|_| ()),
        FlightSudo::IfNeeded => make_symlink_elevating(source, target),
        FlightSudo::Always => sudo::run("/bin/ln", &symlink_command_args(source, target), &[]),
    }
}

pub(super) fn create_flight_dir_all(target: &Path, sudo: FlightSudo) -> Result<()> {
    match sudo {
        FlightSudo::Never => file::create_dir_all(target),
        FlightSudo::IfNeeded => create_dir_all_elevating(target),
        FlightSudo::Always => sudo::run(
            "/bin/mkdir",
            &["-p".into(), "--".into(), target.display().to_string()],
            &[],
        ),
    }
}

pub(super) fn flight_sources(
    staged_path: &Path,
    source: &FlightPath,
    source_glob: bool,
) -> Result<Vec<PathBuf>> {
    if !source_glob {
        let source = resolve_flight_path(staged_path, source)?;
        if !source.exists() {
            bail!(
                "brew-cask: structured move source '{}' was not found",
                source.display()
            );
        }
        return Ok(vec![source]);
    }
    // Homebrew marks move sources as globs explicitly; non-glob move sources
    // may contain literal glob-like characters and should be resolved literally.
    let sources = expand_staged_glob(staged_path, &source.path)?;
    if sources.is_empty() {
        bail!(
            "brew-cask: structured move source '{}' was not found",
            source.path
        );
    }
    Ok(sources)
}

pub(super) fn flight_paths(staged_path: &Path, path: &FlightPath) -> Result<Vec<PathBuf>> {
    if !is_flight_glob(&path.path) {
        return Ok(vec![resolve_flight_path(staged_path, path)?]);
    }
    // Remove steps do not have a `source_glob` flag, so path globs are detected
    // from the path syntax instead.
    expand_staged_glob(staged_path, &path.path)
}

pub(super) fn expand_staged_glob(staged_path: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();
    let escaped_root = glob::Pattern::escape(staged_path.to_string_lossy().as_ref());
    for pattern in expand_braces(pattern) {
        validate_flight_relative_path(&pattern)?;
        let rooted_pattern = Path::new(&escaped_root)
            .join(Path::new(&pattern))
            .to_string_lossy()
            .to_string();
        for path in glob::glob_with(
            &rooted_pattern,
            glob::MatchOptions {
                require_literal_separator: true,
                ..Default::default()
            },
        )
        .wrap_err_with(|| format!("brew-cask: invalid structured flight glob '{pattern}'"))?
        {
            let path = path?;
            if !path.starts_with(staged_path) {
                bail!(
                    "brew-cask: structured flight glob '{}' matched outside staged path",
                    pattern
                );
            }
            matches.push(path);
        }
    }
    matches.sort();
    matches.dedup();
    Ok(matches)
}

pub(super) fn is_flight_glob(path: &str) -> bool {
    path.chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
}

pub(super) fn resolve_flight_path(staged_path: &Path, path: &FlightPath) -> Result<PathBuf> {
    match path.base {
        FlightPathBase::StagedPath => {}
        _ => bail!("brew-cask: structured file operation must use staged_path"),
    }
    let relative = Path::new(&path.path);
    validate_flight_relative_path(&path.path)?;
    Ok(staged_path.join(relative))
}

pub(super) fn resolve_flight_path_with_context(
    cask: &Cask,
    path: &FlightPath,
    staged_path: &Path,
    appdir: &Path,
) -> Result<PathBuf> {
    let expanded = expand_flight_template(cask, &path.path, staged_path, appdir);
    match path.base {
        FlightPathBase::StagedPath => {
            validate_flight_relative_path(&expanded)?;
            Ok(staged_path.join(expanded))
        }
        FlightPathBase::AppDir => {
            validate_flight_relative_path(&expanded)?;
            Ok(appdir.join(expanded))
        }
        FlightPathBase::HomebrewPrefix => {
            validate_flight_relative_path(&expanded)?;
            Ok(prefix::prefix().join(expanded))
        }
        FlightPathBase::Literal => Ok(PathBuf::from(expanded)),
    }
}

pub(super) fn expand_flight_template(
    cask: &Cask,
    value: &str,
    staged_path: &Path,
    appdir: &Path,
) -> String {
    let caskroom_path = caskroom_token_dir(&cask.token);
    let version_major = cask
        .version
        .split(['.', ','])
        .next()
        .unwrap_or(&cask.version);
    let value = value
        .replace("{{version.major}}", version_major)
        .replace("{{caskroom_path}}", &caskroom_path.to_string_lossy());
    expand_cask_template(&value, staged_path, appdir, Some(&cask.version))
}

pub(super) fn expand_cask_template(
    value: &str,
    staged_path: &Path,
    appdir: &Path,
    version: Option<&str>,
) -> String {
    let prefix = prefix::prefix();
    let mut value = value
        .replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy())
        .replace("$APPDIR", &appdir.to_string_lossy())
        .replace("$HOME", &crate::dirs::HOME.to_string_lossy())
        .replace("{{HOMEBREW_PREFIX}}", &prefix.to_string_lossy())
        .replace("{{staged_path}}", &staged_path.to_string_lossy())
        .replace("{{appdir}}", &appdir.to_string_lossy());
    if let Some(version) = version {
        value = value.replace("{{version}}", version);
    }
    if let Some(rest) = value.strip_prefix("~/") {
        value = crate::dirs::HOME.join(rest).to_string_lossy().to_string();
    }
    value
}

pub(super) fn validate_flight_relative_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: invalid structured flight path '{}'",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(end_offset) = pattern[start + 1..].find('}') else {
        return vec![pattern.to_string()];
    };
    let end = start + 1 + end_offset;
    let prefix = &pattern[..start];
    let suffix = &pattern[end + 1..];
    let mut expanded = Vec::new();
    for alternative in pattern[start + 1..end].split(',') {
        for suffix in expand_braces(suffix) {
            expanded.push(format!("{prefix}{alternative}{suffix}"));
        }
    }
    expanded
}
