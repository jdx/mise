use super::*;

pub(super) fn installed_version(token: &str) -> Option<String> {
    let versions = installed_versions(token);
    match versions.as_slice() {
        [version] => Some(version.clone()),
        [] => None,
        _ => {
            warn!("brew-cask:{token}: multiple Caskroom versions found; reinstall to reconcile");
            None
        }
    }
}

pub(super) fn installed_versions(token: &str) -> Vec<String> {
    // Version discovery excludes mise's transaction directories. Cleanup is
    // intentionally broader: remove_stale_versions removes those stale temp
    // and backup directories after replace_caskroom completes.
    let dir = caskroom_token_dir(token);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            entry
                .file_type()
                .ok()
                .filter(|ft| ft.is_dir() && name != ".metadata" && !name.starts_with(".mise-"))
                .map(|_| name)
        })
        .collect()
}

pub(super) fn homebrew_installed_versions(token: &str) -> Result<Vec<String>> {
    let dir = caskroom_token_dir(token);
    let entries = std::fs::read_dir(&dir).wrap_err_with(|| {
        format!(
            "brew-cask:{token}: failed to read Homebrew Caskroom directory '{}'",
            dir.display()
        )
    })?;
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.wrap_err_with(|| {
            format!(
                "brew-cask:{token}: failed to read an entry in Homebrew Caskroom directory '{}'",
                dir.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().wrap_err_with(|| {
            format!(
                "brew-cask:{token}: failed to read type of Homebrew Caskroom entry '{}'",
                path.display()
            )
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            eyre!(
                "brew-cask:{token}: Homebrew Caskroom entry name is not valid UTF-8: '{}'",
                path.display()
            )
        })?;
        if file_type.is_dir() && name != ".metadata" && !name.starts_with(".mise-") {
            versions.push(name);
        }
    }
    Ok(versions)
}

pub(super) fn homebrew_installed_version(token: &str) -> Result<Option<String>> {
    if !homebrew_metadata_present(token)? {
        return Ok(None);
    }

    let versions = homebrew_installed_versions(token)?;
    match versions.as_slice() {
        [version] => Ok(Some(version.clone())),
        [] => bail!(
            "brew-cask:{token}: Homebrew metadata exists, but no installed Caskroom version was found; repair it with `brew reinstall --cask {token}`"
        ),
        versions => bail!(
            "brew-cask:{token}: Homebrew metadata exists with multiple Caskroom versions ({}); repair it with Homebrew",
            {
                let mut versions = versions.to_vec();
                versions.sort();
                versions.join(", ")
            }
        ),
    }
}

pub(super) fn ensure_homebrew_did_not_take_ownership(token: &str, stage: &Path) -> Result<()> {
    let metadata_present = match homebrew_metadata_present(token) {
        Ok(present) => present,
        Err(err) => {
            file::remove_all(stage).wrap_err_with(|| {
                format!(
                    "failed to remove mise stage after Homebrew ownership check failed: {err:#}"
                )
            })?;
            return Err(err);
        }
    };
    if metadata_present {
        file::remove_all(stage)?;
        bail!(
            "brew-cask:{token}: Homebrew took ownership of this cask while installation was pending"
        );
    }
    Ok(())
}

pub(super) fn homebrew_metadata_present(token: &str) -> Result<bool> {
    let path = caskroom_token_dir(token).join(".metadata");
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).wrap_err_with(|| {
            format!(
                "brew-cask:{token}: failed to inspect Homebrew metadata at '{}'",
                path.display()
            )
        }),
    }
}

pub(super) fn pkg_id_installed(pkg_id: &str) -> Result<bool> {
    #[cfg(not(target_os = "macos"))]
    bail!("brew-cask: pkgutil receipt check for '{pkg_id}' is only available on macOS");

    #[cfg(target_os = "macos")]
    // Homebrew's pkgutil metadata is a regular expression, not a literal ID.
    // Match it with pkgutil itself to preserve its nonstandard regexp semantics.
    // Like Homebrew, use the returned IDs rather than the exit status because a
    // query with no matches may exit unsuccessfully.
    let output = std::process::Command::new("pkgutil")
        .arg(format!("--pkgs={pkg_id}"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()?;
    #[cfg(target_os = "macos")]
    Ok(pkgutil_output_has_match(&output.stdout))
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn pkgutil_output_has_match(output: &[u8]) -> bool {
    output.iter().any(|byte| !byte.is_ascii_whitespace())
}

pub(super) fn pkg_ids_installed(pkg_ids: &[String]) -> Result<bool> {
    for pkg_id in pkg_ids {
        if !pkg_id_installed(pkg_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn previous_binary_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    Ok(previous_receipt(cask)?
        .map(|receipt| receipt.binaries)
        .unwrap_or_default())
}

pub(super) fn previous_flight_symlink_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    let Some(receipt) = previous_receipt(cask)? else {
        return Ok(Vec::new());
    };
    receipt_flight_symlink_targets(&receipt)
}

pub(super) fn previous_flight_directory_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    Ok(previous_receipt(cask)?
        .map(|receipt| receipt.flight_directories)
        .unwrap_or_default())
}

pub(super) fn remove_obsolete_flight_directories(
    previous: &BTreeSet<PathBuf>,
    current: &[PathBuf],
) -> Result<()> {
    for directory in previous {
        if !current.contains(directory) {
            remove_empty_directory_elevating(directory)?;
        }
    }
    Ok(())
}

pub(super) fn receipt_flight_symlink_targets(receipt: &CaskReceipt) -> Result<Vec<PathBuf>> {
    let standard_targets = receipt.standard_targets().collect::<BTreeSet<_>>();
    let mut targets = Vec::new();
    for record in &receipt.targets {
        if record.fingerprint.kind == CaskTargetKind::Symlink
            && !standard_targets.contains(&record.path)
            && record.uninstall.unwrap_or(true)
            && cask_target_record_matches(record)?
        {
            targets.push(record.path.clone());
        }
    }
    Ok(targets)
}

pub(super) fn remove_obsolete_binary_links(
    cask: &Cask,
    previous_targets: &[PathBuf],
    current_targets: &[PathBuf],
) -> Result<()> {
    let token_dir = file::desymlink_path(&caskroom_token_dir(&cask.token));
    for target in previous_targets {
        if current_targets.contains(target) {
            continue;
        }
        let Ok(metadata) = target.symlink_metadata() else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(link_target) = std::fs::read_link(target) else {
            continue;
        };
        let resolved = resolve_symlink_target(target, link_target);
        if file::desymlink_path(&resolved).starts_with(&token_dir) {
            remove_artifact_target_elevating(target)?;
        }
    }
    Ok(())
}

#[cfg(not(test))]
pub(super) fn mise_installed_cask_version(cask: &Cask) -> Result<Option<String>> {
    installed_cask_version_in(cask, &crate::dirs::STATE)
}

#[cfg(test)]
pub(super) fn mise_installed_cask_version(cask: &Cask) -> Result<Option<String>> {
    installed_cask_version_in(cask, &prefix::prefix().join(".mise-test-state"))
}

pub(super) fn installed_cask_version_in(cask: &Cask, state_dir: &Path) -> Result<Option<String>> {
    if cask_journal_pending_in(state_dir, &cask.token) {
        return Ok(None);
    }
    match previous_receipt(cask)? {
        Some(receipt) => {
            if receipt.schema_version > 3 {
                return Ok(None);
            }
            if receipt.schema_version >= 2 {
                // Presence only. Content fingerprints are for prune/adopt safety
                // and must not mark a cask "missing": replacing an .app because a
                // file inside it changed resets macOS TCC grants, and hashing
                // every app tree on status/apply is expensive.
                let targets_present = receipt.targets.iter().all(cask_target_present);
                let pkgs_installed = pkg_ids_installed(&receipt.pkg_ids)?;
                return Ok((targets_present && pkgs_installed).then_some(receipt.version));
            }

            // Legacy receipts remain usable only from the historical facts they
            // actually contain. Never fill omitted fields from today's API.
            let targets_exist = receipt.standard_targets().all(|target| target.exists());
            let pkgs_installed = pkg_ids_installed(&receipt.pkg_ids)?;
            Ok((targets_exist && pkgs_installed).then_some(receipt.version))
        }
        None => Ok(None),
    }
}

pub(super) fn state_for_version(
    req: &PackageRequest,
    cask: &Cask,
    version: String,
) -> PackageState {
    match &req.version {
        Some(requested) if version != *requested => {
            PackageState::VersionMismatch { installed: version }
        }
        _ if cask.auto_updates => PackageState::InstalledAutoUpdates { version },
        _ => PackageState::Installed { version },
    }
}

pub(super) fn package_state(req: &PackageRequest, cask: &Cask) -> Result<PackageState> {
    if let Some(version) = homebrew_installed_version(&cask.token)? {
        return Ok(state_for_version(req, cask, version));
    }
    let artifacts = cask_artifacts(cask)?;
    if let Some(state) = platform_unavailable_state(cask, &artifacts) {
        return Ok(state);
    }
    Ok(mise_installed_cask_version(cask)?
        .map(|version| state_for_version(req, cask, version))
        .unwrap_or(PackageState::Missing))
}

pub(super) fn cask_prune_blocker(cask: &Cask, artifacts: &CaskArtifacts) -> Option<String> {
    if !artifacts.pkgs.is_empty() {
        return Some("pkg artifacts require uninstall support".to_string());
    }
    if !artifacts.installers.is_empty() {
        return Some("installer artifacts may have untracked side effects".to_string());
    }
    if !artifacts.command_wrappers.is_empty() {
        return Some("command wrapper artifacts are not supported for pruning".to_string());
    }
    if !artifacts.generic.is_empty() {
        return Some("generic artifacts may install external trees".to_string());
    }
    if !artifacts.preflight_steps.is_empty()
        || !artifacts.postflight_steps.is_empty()
        || has_lifecycle_hook(cask, "preflight")
        || has_lifecycle_hook(cask, "postflight")
    {
        return Some("install lifecycle actions may have untracked side effects".to_string());
    }
    if cask.artifacts.iter().any(|artifact| {
        matches!(
            artifact_type(artifact).as_str(),
            "uninstall"
                | "uninstall_preflight"
                | "uninstall_preflight_steps"
                | "uninstall_postflight"
                | "uninstall_postflight_steps"
        )
    }) {
        return Some("uninstall lifecycle actions are not supported".to_string());
    }
    None
}

pub(super) fn write_receipt_with_flight_targets(
    caskroom: &Path,
    cask: &Cask,
    artifacts: &CaskArtifacts,
    flight_targets: &[PathBuf],
    flight_uninstall_targets: &BTreeMap<PathBuf, bool>,
    flight_directories: &[PathBuf],
    metadata_only_apps: &[PathBuf],
) -> Result<()> {
    let mut target_paths = artifacts.app_target_paths()?;
    target_paths.extend(artifacts.binary_targets()?);
    target_paths.extend(artifacts.font_target_paths()?);
    target_paths.extend(artifacts.completion_target_paths(cask)?);
    target_paths.extend(flight_targets.iter().cloned());
    target_paths.sort();
    target_paths.dedup();
    let targets = target_paths
        .iter()
        .map(|path| {
            Ok(CaskTargetRecord {
                path: path.clone(),
                fingerprint: cask_target_fingerprint(path)?,
                uninstall: flight_uninstall_targets.get(path).copied(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let metadata_only_apps = if cask.auto_updates {
        artifacts.app_target_paths()?
    } else {
        metadata_only_apps.to_vec()
    };
    let prune_blocker = cask_prune_blocker(cask, artifacts).or_else(|| {
        (!metadata_only_apps.is_empty()).then(|| {
            "metadata-only app ownership cannot be proven safely during pruning".to_string()
        })
    });
    let receipt = CaskReceipt {
        schema_version: 3,
        version: cask.version.clone(),
        auto_updates: cask.auto_updates,
        metadata_only_apps,
        apps: artifacts.app_target_paths()?,
        binaries: artifacts.binary_targets()?,
        fonts: artifacts.font_target_paths()?,
        completions: artifacts.completion_target_paths(cask)?,
        flight_directories: flight_directories.to_vec(),
        generic: artifacts.generic_artifact_targets()?,
        pkg_ids: artifacts.pkg_ids.clone(),
        targets,
        prune_safe: prune_blocker.is_none(),
        prune_blocker,
    };
    let body = toml::to_string_pretty(&receipt)?;
    write_durable_file(&caskroom.join(".mise-cask.toml"), body.as_bytes())?;
    Ok(())
}

pub(super) fn cask_target_record_matches(record: &CaskTargetRecord) -> Result<bool> {
    let Ok(actual) = cask_target_fingerprint(&record.path) else {
        return Ok(false);
    };
    Ok(actual == record.fingerprint)
}

/// Whether a receipt target still exists at the recorded path and kind.
///
/// Directory and file targets ignore content drift so status/apply stay cheap
/// and do not reinstall app bundles (which resets macOS TCC grants). Symlink
/// targets still compare the recorded link destination — that is a cheap
/// `readlink` — and require the link to resolve, so dangling or retargeted
/// binaries/completions stay repairable on apply.
pub(super) fn cask_target_present(record: &CaskTargetRecord) -> bool {
    let Ok(metadata) = record.path.symlink_metadata() else {
        return false;
    };
    match record.fingerprint.kind {
        CaskTargetKind::Symlink => {
            if !metadata.file_type().is_symlink() {
                return false;
            }
            let Ok(target) = std::fs::read_link(&record.path) else {
                return false;
            };
            let digest = hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes()));
            if digest != record.fingerprint.digest {
                return false;
            }
            // Follow the link so a dangling binary/completion is not "present".
            std::fs::metadata(&record.path).is_ok()
        }
        CaskTargetKind::File => metadata.is_file(),
        CaskTargetKind::Directory => metadata.is_dir(),
    }
}

pub(super) fn cask_target_fingerprint(path: &Path) -> Result<CaskTargetFingerprint> {
    let metadata = path
        .symlink_metadata()
        .wrap_err_with(|| format!("failed to fingerprint {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        return Ok(CaskTargetFingerprint {
            kind: CaskTargetKind::Symlink,
            digest: hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
        });
    }
    if metadata.is_file() {
        return Ok(CaskTargetFingerprint {
            kind: CaskTargetKind::File,
            digest: hash::file_hash_sha256(path, None)?,
        });
    }
    if metadata.is_dir() {
        return Ok(CaskTargetFingerprint {
            kind: CaskTargetKind::Directory,
            digest: cask_directory_digest(path)?,
        });
    }
    bail!("brew-cask: unsupported target type '{}'", path.display())
}

/// Content identity intentionally excludes timestamps, ownership, and modes.
/// It hashes stable relative paths, entry kinds, file bytes, and link targets
/// without following symlinks.
pub(super) fn cask_directory_digest(root: &Path) -> Result<String> {
    let mut entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    let mut digest = Sha256::new();
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let metadata = path.symlink_metadata()?;
        digest.update([if metadata.file_type().is_symlink() {
            b'l'
        } else if metadata.is_dir() {
            b'd'
        } else if metadata.is_file() {
            b'f'
        } else {
            bail!(
                "brew-cask: unsupported directory entry '{}'",
                path.display()
            );
        }]);
        hash_digest_field(&mut digest, relative.as_os_str().as_encoded_bytes());
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(path)?;
            hash_digest_field(&mut digest, target.as_os_str().as_encoded_bytes());
        } else if metadata.is_dir() {
        } else if metadata.is_file() {
            hash_digest_field(&mut digest, hash::file_hash_sha256(path, None)?.as_bytes());
        }
    }
    Ok(hex::encode(digest.finalize()))
}

pub(super) fn hash_digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

pub(super) fn cask_journal_path_in(state_dir: &Path, token: &str, version: &str) -> PathBuf {
    state_dir
        .join("brew-cask")
        .join(token)
        .join(format!("{version}.json"))
}

pub(super) fn cask_journal_pending_in(state_dir: &Path, token: &str) -> bool {
    state_dir
        .join("brew-cask")
        .join(token)
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_some())
}

pub(super) fn write_cask_journal(journal: &CaskTransactionJournal<'_>) -> Result<()> {
    write_cask_journal_in(&crate::dirs::STATE, journal)
}

pub(super) fn write_cask_journal_in(
    state_dir: &Path,
    journal: &CaskTransactionJournal<'_>,
) -> Result<()> {
    let path = cask_journal_path_in(state_dir, journal.token, journal.version);
    let body = serde_json::to_vec_pretty(journal)?;
    write_durable_file(&path, &body)
}

pub(super) fn record_cask_action(
    journal: &mut CaskTransactionJournal<'_>,
    action: &str,
) -> Result<()> {
    record_cask_action_in(&crate::dirs::STATE, journal, action)
}

pub(super) fn record_cask_action_in(
    state_dir: &Path,
    journal: &mut CaskTransactionJournal<'_>,
    action: &str,
) -> Result<()> {
    journal.completed.push(action.to_string());
    write_cask_journal_in(state_dir, journal)
}

pub(super) fn remove_cask_journals(token: &str) -> Result<()> {
    remove_cask_journals_in(&crate::dirs::STATE, token)
}

pub(super) fn remove_cask_journals_in(state_dir: &Path, token: &str) -> Result<()> {
    let path = state_dir.join("brew-cask").join(token);
    if path.symlink_metadata().is_ok() {
        file::remove_all(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

pub(super) fn write_durable_file(path: &Path, body: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("brew-cask: durable file has no parent"))?;
    file::create_dir_all(parent)?;
    let tmp = path.with_extension("tmp");
    {
        let mut output = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut output, body)?;
        output.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub(super) fn read_receipt(caskroom: &Path) -> Result<Option<CaskReceipt>> {
    let path = caskroom.join(".mise-cask.toml");
    if !path.exists() {
        return Ok(None);
    }
    let body = crate::file::read_to_string(&path)?;
    toml::from_str(&body)
        .map(Some)
        .wrap_err_with(|| format!("failed to parse {}", path.display()))
}

pub(crate) async fn cask_prune_plan(configured: &[PackageRequest]) -> Result<CaskPrunePlan> {
    let closure = resolve_cask_dependency_closure(configured).await?;
    let keep = closure.casks.into_values().collect();
    cask_prune_plan_from_tokens(&keep, &crate::dirs::STATE)
}

pub(crate) async fn cask_formula_dependencies(
    configured: &[PackageRequest],
) -> Result<Vec<PackageRequest>> {
    Ok(resolve_cask_dependency_closure(configured)
        .await?
        .formulae
        .into_values()
        .collect())
}

pub(super) async fn resolve_cask_dependency_closure(
    configured: &[PackageRequest],
) -> Result<CaskDependencyClosure> {
    let mut closure = CaskDependencyClosure::default();
    let mut pending = configured.to_vec();
    while let Some(request) = pending.pop() {
        if closure.casks.contains_key(&cask_dependency_key(&request)) {
            continue;
        }
        let cask = fetch_cask(&request).await?;
        extend_cask_dependency_closure(&mut closure, &mut pending, &request, cask);
    }
    Ok(closure)
}

pub(super) fn cask_request_token(name: &str) -> &str {
    split_tap_name(name)
        .map(|(_, _, token)| token)
        .unwrap_or(name)
}

pub(super) fn cask_dependency_key(request: &PackageRequest) -> (String, Option<String>) {
    (
        cask_request_token(&request.name).to_string(),
        request_tap_url(request),
    )
}

pub(super) fn request_tap_url(request: &PackageRequest) -> Option<String> {
    request.tap_url.clone().or_else(|| {
        split_tap_name(&request.name).and_then(|(owner, tap, _)| {
            (owner != "homebrew" || tap != "cask")
                .then(|| format!("https://github.com/{owner}/homebrew-{tap}.git"))
        })
    })
}

pub(super) fn extend_cask_dependency_closure(
    closure: &mut CaskDependencyClosure,
    pending: &mut Vec<PackageRequest>,
    request: &PackageRequest,
    cask: Cask,
) {
    if closure
        .casks
        .insert(cask_dependency_key(request), cask.token)
        .is_some()
    {
        return;
    }
    for name in cask.depends_on.formula {
        let request = PackageRequest {
            tap_url: dependency_tap_url(request, &name),
            name,
            version: None,
        };
        closure
            .formulae
            .entry((request.name.clone(), request_tap_url(&request)))
            .or_insert(request);
    }
    pending.extend(cask.depends_on.cask.into_iter().map(|name| PackageRequest {
        tap_url: dependency_tap_url(request, &name),
        name,
        version: None,
    }));
}

pub(super) fn dependency_tap_url(parent: &PackageRequest, dependency: &str) -> Option<String> {
    let parent_tap = split_tap_name(&parent.name)
        .and_then(|(owner, tap, _)| (owner != "homebrew" || tap != "cask").then_some((owner, tap)));
    if let Some((owner, tap, _)) = split_tap_name(dependency)
        && parent_tap != Some((owner, tap))
    {
        return None;
    }
    parent.tap_url.clone().or_else(|| {
        parent_tap.map(|(owner, tap)| format!("https://github.com/{owner}/homebrew-{tap}.git"))
    })
}

pub(super) fn cask_prune_plan_from_tokens(
    keep: &BTreeSet<String>,
    state_dir: &Path,
) -> Result<CaskPrunePlan> {
    let mut plan = CaskPrunePlan::default();
    let mut candidates = Vec::new();
    let mut claims = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    let mut claims_complete = true;
    let caskroom = prefix::prefix().join("Caskroom");
    let Ok(tokens) = std::fs::read_dir(&caskroom) else {
        return Ok(plan);
    };

    for entry in tokens {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                claims_complete = false;
                plan.skipped.push(CaskPruneSkip {
                    token: "Caskroom".to_string(),
                    reason: format!("Caskroom entry could not be read: {err}"),
                });
                continue;
            }
        };
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(err) => {
                claims_complete = false;
                plan.skipped.push(CaskPruneSkip {
                    token: entry.file_name().to_string_lossy().to_string(),
                    reason: format!("Caskroom entry type could not be read: {err}"),
                });
                continue;
            }
        };
        if !kind.is_dir() {
            continue;
        }
        let Some(token) = entry.file_name().to_str().map(str::to_string) else {
            claims_complete = false;
            plan.skipped.push(CaskPruneSkip {
                token: entry.file_name().to_string_lossy().to_string(),
                reason: "Caskroom token name is not valid UTF-8".to_string(),
            });
            continue;
        };
        if token.starts_with('.') {
            continue;
        }
        let configured = keep.contains(&token);
        let Ok(version_entries) = std::fs::read_dir(entry.path()) else {
            claims_complete = false;
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "Caskroom directory could not be read".to_string(),
                });
            }
            continue;
        };
        let mut versions = Vec::new();
        let mut version_error = None;
        for version in version_entries {
            let version = match version {
                Ok(version) => version,
                Err(err) => {
                    claims_complete = false;
                    version_error =
                        Some(format!("Caskroom version entry could not be read: {err}"));
                    continue;
                }
            };
            let kind = match version.file_type() {
                Ok(kind) => kind,
                Err(err) => {
                    claims_complete = false;
                    version_error = Some(format!(
                        "Caskroom version entry type could not be read: {err}"
                    ));
                    continue;
                }
            };
            if kind.is_dir() && !version.file_name().to_string_lossy().starts_with('.') {
                versions.push(version);
            }
        }
        if let Some(reason) = version_error {
            if !configured {
                plan.skipped.push(CaskPruneSkip { token, reason });
            }
            continue;
        }
        let mut receipts = BTreeMap::new();
        let mut receipt_error = None;
        for version in &versions {
            match read_receipt(&version.path()) {
                Ok(Some(receipt)) => {
                    for target in &receipt.targets {
                        claims
                            .entry(target.path.clone())
                            .or_default()
                            .insert(token.clone());
                    }
                    receipts.insert(version.path(), receipt);
                }
                Ok(None) => {}
                Err(err) => {
                    claims_complete = false;
                    receipt_error =
                        Some(format!("mise ownership receipt could not be read: {err:#}"));
                }
            }
        }
        if let Some(reason) = receipt_error {
            if !configured {
                plan.skipped.push(CaskPruneSkip { token, reason });
            }
            continue;
        }
        if entry.path().join(".metadata").symlink_metadata().is_ok() {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "Homebrew owns this cask".to_string(),
                });
            }
            continue;
        }
        let [version] = versions.as_slice() else {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "expected exactly one installed Caskroom version".to_string(),
                });
            }
            continue;
        };
        let version_dir = version.path();
        let Some(receipt) = receipts.remove(&version_dir) else {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "mise ownership receipt is missing".to_string(),
                });
            }
            continue;
        };
        if configured {
            continue;
        }
        if cask_journal_pending_in(state_dir, &token) {
            plan.skipped.push(CaskPruneSkip {
                token,
                reason: "an incomplete cask transaction is pending".to_string(),
            });
            continue;
        }
        if receipt.schema_version != 3 {
            plan.skipped.push(CaskPruneSkip {
                token,
                reason: "receipt predates safe prune metadata; upgrade or reinstall first"
                    .to_string(),
            });
            continue;
        }
        if !receipt.prune_safe {
            let reason = receipt
                .prune_blocker
                .clone()
                .unwrap_or_else(|| "receipt does not permit pruning".to_string());
            plan.skipped.push(CaskPruneSkip { token, reason });
            continue;
        }
        let version = version.file_name().to_string_lossy().to_string();
        let candidate = CaskPruneCandidate {
            token,
            version,
            version_dir,
            receipt,
        };
        if let Err(reason) = validate_cask_prune_candidate(&candidate) {
            plan.skipped.push(CaskPruneSkip {
                token: candidate.token,
                reason: format!("recorded artifacts cannot be removed safely: {reason:#}"),
            });
            continue;
        }
        candidates.push(candidate);
    }

    for candidate in candidates {
        if !claims_complete {
            plan.skipped.push(CaskPruneSkip {
                token: candidate.token,
                reason: "cask ownership receipts could not be indexed completely".to_string(),
            });
            continue;
        }
        let shared = candidate
            .receipt
            .targets
            .iter()
            .filter_map(|target| {
                claims
                    .get(&target.path)
                    .filter(|tokens| tokens.len() > 1)
                    .map(|_| target.path.clone())
            })
            .collect::<Vec<_>>();
        if shared.is_empty() {
            plan.remove.push(candidate);
        } else {
            plan.skipped.push(CaskPruneSkip {
                token: candidate.token,
                reason: format!(
                    "recorded artifact target is also claimed by another cask: {}",
                    shared
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }
    plan.remove.sort_by(|a, b| a.token.cmp(&b.token));
    plan.skipped.sort_by(|a, b| a.token.cmp(&b.token));
    Ok(plan)
}

pub(crate) fn apply_cask_prune_plan(plan: &CaskPrunePlan, dry_run: bool) -> Result<usize> {
    apply_cask_prune_plan_in(plan, dry_run, &crate::dirs::STATE)
}

pub(super) fn apply_cask_prune_plan_in(
    plan: &CaskPrunePlan,
    dry_run: bool,
    state_dir: &Path,
) -> Result<usize> {
    if dry_run {
        for candidate in &plan.remove {
            miseprintln!("remove brew-cask:{}@{}", candidate.token, candidate.version);
        }
        return Ok(0);
    }

    let _caskroom_lock = lock_caskroom()?;
    let mut removed = 0;
    for candidate in &plan.remove {
        if let Err(reason) = validate_cask_prune_candidate(candidate)
            .and_then(|_| validate_cask_prune_claims(candidate))
        {
            warn!(
                "brew-cask:{}: skipped because recorded artifacts changed after planning: {reason:#}",
                candidate.token
            );
            continue;
        }
        let remove = || -> Result<()> {
            let mut journal = CaskTransactionJournal {
                schema_version: 1,
                token: &candidate.token,
                version: &candidate.version,
                completed: Vec::new(),
            };
            write_cask_journal_in(state_dir, &journal)?;
            for (index, target) in candidate.receipt.targets.iter().enumerate() {
                remove_artifact_target_elevating(&target.path)?;
                record_cask_action_in(state_dir, &mut journal, &format!("prune_target[{index}]"))?;
            }
            file::remove_all(&candidate.version_dir)?;
            record_cask_action_in(state_dir, &mut journal, "prune_caskroom")?;
            if let Some(token_dir) = candidate.version_dir.parent()
                && let Err(err) = file::remove_dir(token_dir)
            {
                debug!(
                    "brew-cask:{}: kept non-empty Caskroom token directory: {err:#}",
                    candidate.token
                );
            }
            remove_cask_journals_in(state_dir, &candidate.token)
        };
        match remove() {
            Ok(()) => removed += 1,
            Err(err) => warn!(
                "brew-cask:{}: failed to apply planned removal; continuing: {err:#}",
                candidate.token
            ),
        }
    }
    Ok(removed)
}

pub(super) fn lock_caskroom() -> Result<fslock::LockFile> {
    let caskroom = prefix::prefix().join("Caskroom");
    file::create_dir_all(&caskroom)?;
    let path = caskroom.join(".mise.lock");
    let mut lock = fslock::LockFile::open(&path)?;
    if !lock.try_lock()? {
        debug!("waiting for brew-cask lock on {}", path.display());
        lock.lock()?;
    }
    Ok(lock)
}

pub(super) fn validate_cask_prune_claims(candidate: &CaskPruneCandidate) -> Result<()> {
    let caskroom = prefix::prefix().join("Caskroom");
    let mut claims = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for entry in std::fs::read_dir(&caskroom)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let token = entry.file_name().to_string_lossy().to_string();
        for version in std::fs::read_dir(entry.path())? {
            let version = version?;
            if !version.file_type()?.is_dir()
                || version.file_name().to_string_lossy().starts_with('.')
            {
                continue;
            }
            if let Some(receipt) = read_receipt(&version.path())? {
                for target in receipt.targets {
                    claims.entry(target.path).or_default().insert(token.clone());
                }
            }
        }
    }
    for target in &candidate.receipt.targets {
        if claims
            .get(&target.path)
            .is_some_and(|tokens| tokens.iter().any(|token| token != &candidate.token))
        {
            bail!(
                "artifact target is now claimed by another cask: {}",
                target.path.display()
            );
        }
    }
    Ok(())
}

pub(super) fn validate_cask_prune_candidate(candidate: &CaskPruneCandidate) -> Result<()> {
    if homebrew_metadata_present(&candidate.token)? {
        bail!("Homebrew now owns this cask");
    }
    let receipt = &candidate.receipt;
    if read_receipt(&candidate.version_dir)?.as_ref() != Some(receipt) {
        bail!("ownership receipt has changed");
    }
    if receipt.schema_version != 3 || !receipt.prune_safe || !receipt.pkg_ids.is_empty() {
        bail!("receipt is not marked safe for direct-artifact pruning");
    }
    if !receipt.metadata_only_apps.is_empty() {
        bail!("metadata-only app ownership cannot be proven safely during pruning");
    }
    let records = receipt
        .targets
        .iter()
        .map(|record| (record.path.clone(), record))
        .collect::<BTreeMap<_, _>>();
    let expected = receipt.standard_targets().cloned().collect::<BTreeSet<_>>();
    if expected.is_empty()
        || records.len() != receipt.targets.len()
        || records.len() != expected.len()
        || receipt
            .metadata_only_apps
            .iter()
            .any(|path| !receipt.apps.contains(path))
    {
        bail!("receipt target inventory is incomplete or duplicated");
    }
    if records.keys().any(|path| !expected.contains(path)) {
        bail!("receipt target inventory contains an unclassified path");
    }

    for path in &receipt.apps {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing app target record"))?;
        if record.fingerprint.kind != CaskTargetKind::Directory
            || !allowed_appdir_roots()?
                .iter()
                .any(|root| path_is_below(path, root))
            || !path.file_name().is_some_and(|name| {
                staged_app_matches_target(record, &candidate.version_dir.join(name))
            })
        {
            bail!(
                "app target is outside an allowed Applications directory: {}",
                path.display()
            );
        }
    }
    for path in &receipt.binaries {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing binary target record"))?;
        if record.fingerprint.kind != CaskTargetKind::Symlink
            || !allowed_binary_target_roots()
                .iter()
                .any(|root| path_is_below(path, root))
            || !symlink_resolves_below(path, &candidate.version_dir)
        {
            bail!(
                "binary target is not an owned Caskroom symlink: {}",
                path.display()
            );
        }
    }
    for path in &receipt.fonts {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing font target record"))?;
        let fonts = font_dir();
        if record.fingerprint.kind != CaskTargetKind::File
            || !path_is_below(path, &fonts)
            || !path.strip_prefix(&fonts).is_ok_and(|relative| {
                staged_target_matches(record, &candidate.version_dir.join(relative))
            })
        {
            bail!(
                "font target is outside the platform font directory: {}",
                path.display()
            );
        }
    }
    let completion_roots = [
        CompletionShell::Bash,
        CompletionShell::Fish,
        CompletionShell::Zsh,
        CompletionShell::Pwsh,
    ]
    .map(default_completion_dir);
    for path in &receipt.completions {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing completion target record"))?;
        if record.fingerprint.kind != CaskTargetKind::Symlink
            || !completion_roots
                .iter()
                .any(|root| path_is_below(path, root))
            || !symlink_resolves_below(path, &candidate.version_dir)
        {
            bail!(
                "completion target is not an owned Caskroom symlink: {}",
                path.display()
            );
        }
    }
    for record in &receipt.targets {
        if !cask_target_record_matches(record)? {
            bail!("artifact target has changed: {}", record.path.display());
        }
    }
    Ok(())
}

pub(super) fn path_is_below(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative.components().next().is_some()
            && !relative
                .components()
                .any(|component| component == Component::ParentDir)
    })
}

pub(super) fn staged_target_matches(record: &CaskTargetRecord, staged: &Path) -> bool {
    cask_target_fingerprint(staged).is_ok_and(|fingerprint| fingerprint == record.fingerprint)
}

pub(super) fn staged_app_matches_target(record: &CaskTargetRecord, staged: &Path) -> bool {
    let Ok(metadata) = staged.symlink_metadata() else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        // Preserve pruning support for casks installed before app artifacts
        // switched from retained copies to Homebrew-compatible symlinks.
        return staged_target_matches(record, staged);
    }
    std::fs::read_link(staged)
        .map(|target| resolve_symlink_target(staged, target) == record.path)
        .unwrap_or(false)
}

pub(super) fn symlink_resolves_below(path: &Path, root: &Path) -> bool {
    let Ok(target) = std::fs::read_link(path) else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or_else(|| Path::new("/")).join(target)
    };
    path_starts_with_resolved_root(&target, root)
}

pub(super) fn caskroom_token_dir(token: &str) -> PathBuf {
    prefix::prefix().join("Caskroom").join(token)
}

pub(super) fn caskroom_version_dir(token: &str, version: &str) -> PathBuf {
    caskroom_token_dir(token).join(version)
}

pub(super) fn caskroom_tmp_dir(cask: &Cask) -> PathBuf {
    let key = format!("{}-{}", cask.token, cask.version);
    caskroom_token_dir(&cask.token).join(format!(".mise-tmp-{}", hash::hash_to_str(&key)))
}

pub(super) fn caskroom_backup_dir(cask: &Cask) -> PathBuf {
    let key = format!("{}-{}", cask.token, cask.version);
    caskroom_token_dir(&cask.token).join(format!(".mise-backup-{}", hash::hash_to_str(&key)))
}

#[derive(Debug)]
pub(super) struct ArtifactLinkBackup {
    pub(super) target: PathBuf,
    pub(super) backup: Option<PathBuf>,
    pub(super) target_parent: PathBuf,
    pub(super) backup_parent: Option<PathBuf>,
    pub(super) elevate: bool,
}

#[derive(Debug)]
pub(super) struct ArtifactLinkTransaction {
    backups: Vec<ArtifactLinkBackup>,
}

impl ArtifactLinkTransaction {
    pub(super) fn begin(mut targets: Vec<PathBuf>) -> Result<Self> {
        targets.sort();
        targets.dedup();
        let mut transaction = Self {
            backups: Vec::with_capacity(targets.len()),
        };
        for target in targets {
            let entry = (|| -> Result<ArtifactLinkBackup> {
                let backup = if target.symlink_metadata().is_ok() {
                    let parent = target
                        .parent()
                        .ok_or_else(|| eyre!("brew-cask: artifact target has no parent"))?;
                    let backup = parent.join(format!(
                        ".mise-link-backup-{}",
                        hash::hash_to_str(&target.display().to_string())
                    ));
                    remove_artifact_target_elevating(&backup)?;
                    rename_elevating(&target, &backup)?;
                    Some(backup)
                } else {
                    None
                };
                let target_parent = resolved_parent(&target)?;
                let backup_parent = backup.as_deref().map(resolved_parent).transpose()?;
                Ok(ArtifactLinkBackup {
                    target,
                    backup,
                    target_parent,
                    backup_parent,
                    elevate: true,
                })
            })();
            match entry {
                Ok(entry) => transaction.backups.push(entry),
                Err(err) => {
                    if let Err(rollback_err) = transaction.rollback() {
                        return Err(err.wrap_err(format!(
                            "failed to restore artifact targets after backup failed: {rollback_err:#}"
                        )));
                    }
                    return Err(err);
                }
            }
        }
        Ok(transaction)
    }

    pub(super) fn rollback(&mut self) -> Result<()> {
        let mut first_error = None;
        for entry in self.backups.iter().rev() {
            match remove_artifact_target_elevating(&entry.target) {
                Ok(()) => {
                    if let Some(backup) = &entry.backup
                        && let Err(err) = rename_elevating(backup, &entry.target)
                    {
                        first_error.get_or_insert(err);
                    }
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            self.backups.clear();
            Ok(())
        }
    }

    pub(super) fn commit(&mut self) -> Result<()> {
        for entry in &self.backups {
            if let Some(backup) = &entry.backup {
                remove_artifact_target_elevating(backup)?;
            }
        }
        self.backups.clear();
        Ok(())
    }
}

pub(super) fn replace_caskroom(
    cask: &Cask,
    staged: &Path,
    destination: &Path,
    link_artifacts: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let backup = caskroom_backup_dir(cask);
    file::remove_all(&backup)?;
    let had_previous = destination.symlink_metadata().is_ok();
    if had_previous {
        file::rename(destination, &backup)?;
    }
    if let Err(err) = file::rename(staged, destination) {
        if had_previous {
            file::rename(&backup, destination)?;
        }
        return Err(err);
    }
    if let Err(err) = link_artifacts() {
        let rollback = (|| -> Result<()> {
            file::remove_all(destination)?;
            if had_previous {
                file::rename(&backup, destination)?;
            }
            Ok(())
        })();
        if let Err(rollback_err) = rollback {
            return Err(err.wrap_err(format!(
                "failed to restore previous cask after activation failed: {rollback_err:#}"
            )));
        }
        return Err(err);
    }
    file::remove_all(backup)?;
    Ok(())
}

pub(super) fn remove_stale_versions(token_dir: &Path, current_version: &str) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(token_dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name();
        if entry.file_type().is_ok_and(|ft| ft.is_dir())
            && name.to_str() != Some(current_version)
            && name != ".metadata"
        {
            file::remove_all(entry.path())?;
        }
    }
    Ok(())
}

pub(super) fn archive_filename(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    url.path_segments()?.next_back().map(str::to_string)
}

pub(super) fn split_tap_name(name: &str) -> Option<(&str, &str, &str)> {
    super::super::api::split_tap_name(name)
}

pub(super) fn artifact_type(value: &Value) -> String {
    value
        .as_object()
        .and_then(|o| o.keys().next())
        .cloned()
        .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn is_non_install_artifact(kind: &str) -> bool {
    matches!(
        kind,
        "caveats"
            | "conflicts_with"
            | "depends_on"
            | "manpage"
            | "postflight"
            | "preflight"
            | "uninstall_postflight_steps"
            | "uninstall_preflight_steps"
            | "uninstall"
            | "uninstall_postflight"
            | "uninstall_preflight"
            | "zap"
    )
}

pub(super) fn has_lifecycle_hook(cask: &Cask, hook: &str) -> bool {
    cask.artifacts
        .iter()
        .any(|artifact| artifact_type(artifact) == hook)
}
