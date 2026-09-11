//! Generate selected scope from requests with an immutable previous lockfile.
use super::*;
use crate::ui::multi_progress_report::MultiProgressReport;
use std::sync::atomic::{AtomicBool, Ordering};

static INSTALL_FAILED: AtomicBool = AtomicBool::new(false);
static INSTALL_FAILURE_DETAIL: Lazy<Mutex<Option<String>>> = Lazy::new(Default::default);
type ResolutionCell = Arc<tokio::sync::OnceCell<LockResolutionResult>>;
static RESOLUTIONS: Lazy<Mutex<HashMap<String, ResolutionCell>>> = Lazy::new(Default::default);
static PREPARE_SLOTS: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));

#[derive(Default)]
pub(crate) struct PreparationBatch {
    tasks: Mutex<JoinSet<(crate::toolset::ToolRequest, Result<()>)>>,
    pending: Mutex<Vec<(Arc<Config>, ToolVersion)>>,
    requests: Mutex<HashMap<tokio::task::Id, crate::toolset::ToolRequest>>,
    snapshots: Mutex<BTreeMap<PathBuf, InstallSnapshot>>,
}

struct InstallSnapshot {
    request: crate::toolset::ToolRequest,
    content: Option<Vec<u8>>,
}

fn snapshot(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl PreparationBatch {
    pub(crate) fn start(
        &self,
        config: Arc<Config>,
        tv: ToolVersion,
        concurrent: bool,
    ) -> Result<()> {
        {
            let mut snapshots = self.snapshots.lock().unwrap();
            let lock_path =
                lockfile_path_for_tool_source(&config, tv.request.source()).map(|(path, _)| path);
            for path in config.config_files.keys().cloned().chain(lock_path) {
                if let std::collections::btree_map::Entry::Vacant(entry) = snapshots.entry(path) {
                    let content = snapshot(entry.key())?;
                    entry.insert(InstallSnapshot {
                        request: tv.request.clone(),
                        content,
                    });
                }
            }
        }
        if !concurrent {
            self.pending.lock().unwrap().push((config, tv));
            return Ok(());
        }
        let request = tv.request.clone();
        let handle = self.tasks.lock().unwrap().spawn(async move {
            let result = prepare_install(&config, &tv).await;
            (tv.request, result)
        });
        self.requests.lock().unwrap().insert(handle.id(), request);
        Ok(())
    }

    pub(crate) async fn finish(&self) -> Vec<(crate::toolset::ToolRequest, eyre::Report)> {
        let mut tasks = std::mem::take(&mut *self.tasks.lock().unwrap());
        let mut errors = Vec::new();
        let mut requests = std::mem::take(&mut *self.requests.lock().unwrap());
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for (config, tv) in pending {
            if let Err(error) = prepare_install(&config, &tv).await {
                errors.push((tv.request, error));
            }
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((request, Err(error))) => errors.push((request, error)),
                Err(error) => {
                    record_install_failure();
                    if let Some(request) = requests.remove(&error.id()) {
                        errors.push((request, eyre!("lockfile preparation failed: {error}")));
                    }
                }
                Ok((_, Ok(()))) => {}
            }
        }
        for (path, previous) in std::mem::take(&mut *self.snapshots.lock().unwrap()) {
            match snapshot(&path) {
                Ok(content) if content == previous.content => {}
                result => errors.push((
                    previous.request,
                    match result {
                        Err(error) => error,
                        _ => eyre!(
                            "{} changed during installation; retry to regenerate the lockfile",
                            path.display()
                        ),
                    },
                )),
            }
        }
        if !errors.is_empty() {
            *INSTALL_FAILURE_DETAIL.lock().unwrap() = Some(
                errors
                    .iter()
                    .map(|(_, error)| error.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            record_install_failure();
        }
        errors
    }
}

pub(crate) fn record_install_failure() {
    INSTALL_FAILED.store(true, Ordering::Relaxed);
}

pub(crate) fn ensure_install_succeeded() -> Result<()> {
    if INSTALL_FAILED.load(Ordering::Relaxed) {
        if let Some(detail) = INSTALL_FAILURE_DETAIL.lock().unwrap().as_ref() {
            bail!("{detail}\nprevious lockfiles were preserved");
        }
        bail!("installation failed; previous lockfiles were preserved");
    }
    Ok(())
}

pub(crate) fn has_previous_file(config: &Config, path: &Path) -> bool {
    path.exists()
        || monorepo_lockfile_migration_paths(config)
            .iter()
            .any(|(source, target)| target == path && source.exists())
}

pub(crate) fn read_previous(config: &Config, path: &Path, upgrade: bool) -> Result<Lockfile> {
    let mut previous = Lockfile::read(path)?;
    let mut exists = path.exists();
    for (source, target) in monorepo_lockfile_migration_paths(config) {
        if target != path || !source.exists() {
            continue;
        }
        let legacy = Lockfile::read(&source)?;
        if !exists {
            previous.lockfile_version = legacy.lockfile_version;
            previous
                .generated_header_url
                .clone_from(&legacy.generated_header_url);
            exists = true;
        } else if previous.lockfile_version != legacy.lockfile_version {
            if !upgrade {
                bail!("incompatible monorepo lockfile formats; run `mise lock --upgrade`");
            }
            previous.lockfile_version = previous.lockfile_version.max(legacy.lockfile_version);
        }
        merge_lockfile_preserving_root(&mut previous, legacy);
    }
    Ok(previous)
}

pub(crate) async fn prepare_install(config: &Config, tv: &ToolVersion) -> Result<()> {
    let Some((path, _)) = lockfile_path_for_tool_source(config, tv.request.source()) else {
        return Ok(());
    };
    if !has_previous_file(config, &path)
        && (!config.lockfile_creation_enabled()
            || tv
                .request
                .source()
                .path()
                .is_some_and(crate::config::is_global_config))
    {
        return Ok(());
    }
    let previous = read_previous(config, &path, false)?;
    let mut platforms = determine_target_platforms_from_lockfile(&previous.all_platform_keys())?;
    // The installer owns the host artifact (and may fall back to a source build).
    // Start foreign artifacts first instead of contending with that download.
    platforms.retain(|platform| platform.to_key() != Platform::current().to_key());
    // Reserve at most one background worker, leaving the remaining workers for installs.
    let _permit = PREPARE_SLOTS.acquire().await?;
    generate(
        &previous,
        &[(tv.ba().clone(), tv.clone())],
        &platforms,
        true,
        false,
        1,
        &[],
    )
    .await?;
    Ok(())
}

fn resolution_key(
    ba: &crate::cli::args::BackendArg,
    tv: &ToolVersion,
    platform: &Platform,
) -> String {
    let mut options = tv.request.options().clone();
    options.opts.values.sort_keys();
    options.core.install_env.sort_keys();
    let mut install_env = tv.install_env();
    install_env.sort_keys();
    format!(
        "{}\n{}\n{:?}\n{:?}\n{}",
        ba.full(),
        tv.version,
        options,
        install_env,
        platform.to_key()
    )
}

async fn resolve(
    ba: crate::cli::args::BackendArg,
    tv: ToolVersion,
    platform: Platform,
) -> Result<LockResolutionResult> {
    let key = resolution_key(&ba, &tv, &platform);
    let cell = RESOLUTIONS.lock().unwrap().entry(key).or_default().clone();
    Ok(cell
        .get_or_init(|| async {
            let backend = tv.backend().ok();
            let mut resolution = resolve_tool_lock_info(ba, tv, platform, backend).await;
            if let Ok(info) = &mut resolution.4
                && let Err(error) = complete_artifact_checksums(info).await
            {
                resolution.4 = Err(error.to_string());
            }
            resolution
        })
        .await
        .clone())
}

pub(crate) type Tool = (crate::cli::args::BackendArg, ToolVersion);

/// A conservative check for a complete, unfiltered warm install. Compare the
/// resolved inputs, not file timestamps: environment-dependent options and
/// verification settings can change without editing mise.toml.
///
/// Multiple requests for one short and shared dependency tables use the normal
/// generator, which owns binding conflicts and dependency-table cleanup.
pub(crate) fn is_current(
    previous: &Lockfile,
    tools: &[Tool],
    platforms: &[Platform],
) -> Result<bool> {
    if tools.is_empty()
        || platforms.is_empty()
        || previous.tools.len() != tools.len()
        || !previous.conda_packages.is_empty()
        || !previous.pkgx_packages.is_empty()
        || Settings::get().force_provenance_verify()
    {
        return Ok(false);
    }
    let mut shorts = BTreeSet::new();
    for (ba, tv) in tools {
        if !shorts.insert(&ba.short) {
            return Ok(false);
        }
        let Some(entries) = previous.tools.get(&ba.short) else {
            return Ok(false);
        };
        let backend = tv.backend()?;
        let stored_backend = ba.stored_full();
        let specifier = tv.request.version();
        let mut covered: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        for platform in platforms {
            for platform in backend.platform_variants(platform) {
                let options = backend.resolve_lockfile_options(
                    &tv.request,
                    &PlatformTarget::new(platform.clone()),
                )?;
                let Some((index, entry)) = entries.iter().enumerate().find(|(_, entry)| {
                    entry.version == tv.version
                        && entry.backend.as_deref() == Some(stored_backend.as_str())
                        && entry.options == options
                }) else {
                    return Ok(false);
                };
                let bindings_current = if previous.uses_request_bindings() {
                    entry.specifiers.len() == 1 && entry.specifiers.contains(&specifier)
                } else {
                    entry.specifiers.is_empty()
                };
                let key = platform.to_key();
                let Some(info) = entry.platforms.get(&key) else {
                    return Ok(false);
                };
                if !bindings_current || !can_reuse(info) {
                    return Ok(false);
                }
                validate_provenance_settings(ba, tv, &key, info)?;
                if let Some(error) = check_single_tool_provenance(
                    Some(entries),
                    &ba.short,
                    &tv.version,
                    &stored_backend,
                    &key,
                    info.provenance.as_ref(),
                ) {
                    bail!("{error}");
                }
                covered.entry(index).or_default().insert(key);
            }
        }
        // Extra versions, option variants, or platforms must still be pruned.
        // Counting distinct entries also rejects duplicate version/option rows.
        if covered.is_empty()
            || covered.len() != entries.len()
            || covered
                .iter()
                .any(|(index, keys)| keys.len() != entries[*index].platforms.len())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn can_reuse(info: &PlatformInfo) -> bool {
    info.checksum.is_some()
        && info.url.is_some()
        && info.signer.is_none()
        && info
            .additional_artifacts
            .iter()
            .all(|artifact| artifact.checksum.is_some())
        && !Settings::get().force_provenance_verify()
}

pub(crate) async fn generate(
    previous: &Lockfile,
    tools: &[Tool],
    platforms: &[Platform],
    filtered_tools: bool,
    filtered_platforms: bool,
    jobs: usize,
    installed: &[ToolVersion],
) -> Result<Lockfile> {
    let mut candidate = Lockfile {
        lockfile_version: previous.lockfile_version,
        generated_header_url: previous.generated_header_url.clone(),
        ..Default::default()
    };
    let selected: BTreeSet<_> = tools.iter().map(|(ba, _)| ba.short.as_str()).collect();
    let mut targets = Vec::new();
    let mut keys = BTreeSet::new();
    for (ba, tv) in tools {
        let backend = tv.backend()?;
        for platform in platforms {
            for platform in backend.platform_variants(platform) {
                let options = backend.resolve_lockfile_options(
                    &tv.request,
                    &PlatformTarget::new(platform.clone()),
                )?;
                keys.insert((ba.short.clone(), platform.to_key()));
                targets.push((ba.clone(), tv.clone(), platform, options));
            }
        }
    }
    for (short, entries) in &previous.tools {
        for entry in entries {
            let mut untouched = entry.clone();
            if selected.contains(short.as_str()) {
                if !filtered_platforms {
                    continue;
                }
                untouched
                    .platforms
                    .retain(|platform, _| !keys.contains(&(short.clone(), platform.clone())));
                if untouched.platforms.is_empty() {
                    continue;
                }
            } else if !filtered_tools {
                continue;
            }
            candidate
                .tools
                .entry(short.clone())
                .or_default()
                .push(untouched);
        }
    }
    candidate.conda_packages = previous.conda_packages.clone();
    candidate.pkgx_packages = previous.pkgx_packages.clone();
    let report = MultiProgressReport::get().add("lock");
    let mut progress = ProgressGuard {
        report: report.as_ref(),
        finished: false,
    };
    report.set_length(targets.len() as u64);
    let semaphore = Arc::new(Semaphore::new(crate::jobs::normalize(jobs)));
    let mut tasks = JoinSet::new();
    let mut resolved = Vec::new();
    for (ordinal, (ba, tv, platform, options)) in targets.into_iter().enumerate() {
        let actual = installed
            .iter()
            .find(|actual| {
                actual.ba().full() == ba.full()
                    && actual.version == tv.version
                    && actual.request.options() == tv.request.options()
                    && actual.request.source() == tv.request.source()
            })
            .and_then(|actual| {
                let backend = actual.backend().ok()?;
                (platform.to_key() == backend.get_platform_key())
                    .then(|| actual.lock_platforms.get(&platform.to_key()).cloned())
                    .flatten()
            })
            .filter(|info| {
                info.url.is_some()
                    || info.install.is_some()
                    || info.conda_deps.is_some()
                    || info.pkgx_deps.is_some()
            });
        let previous_info = previous.tools.get(&ba.short).and_then(|entries| {
            entries
                .iter()
                .find(|entry| {
                    entry.version == tv.version
                        && entry.options == options
                        && entry.backend.as_deref() == Some(ba.stored_full().as_str())
                })
                .and_then(|entry| entry.platforms.get(&platform.to_key()))
                .cloned()
        });
        if let Some(info) = &previous_info
            && let Err(error) = validate_provenance_settings(&ba, &tv, &platform.to_key(), info)
        {
            tasks.shutdown().await;
            return Err(error);
        }
        // Complete cached artifacts need neither I/O nor a spawned task. Keep them
        // in the same ordered result stream so all downgrade checks still run.
        let reusable = previous_info.as_ref().filter(|info| can_reuse(info));
        if actual.is_none()
            && let Some(info) = reusable
        {
            resolved.push((
                ordinal,
                tv.request.version(),
                (
                    ba.short.clone(),
                    tv.version.clone(),
                    ba.stored_full(),
                    platform,
                    Ok(info.clone()),
                    options,
                    BTreeMap::new(),
                    BTreeMap::new(),
                    LockResolutionStatus::Optional,
                ),
            ));
            continue;
        }
        let semaphore = semaphore.clone();
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await?;
            let mut resolution = if let Some(info) = actual {
                (
                    ba.short.clone(),
                    tv.version.clone(),
                    ba.stored_full(),
                    platform,
                    Ok(info),
                    options,
                    BTreeMap::new(),
                    BTreeMap::new(),
                    LockResolutionStatus::Optional,
                )
            } else {
                resolve(ba, tv.clone(), platform).await?
            };
            if let Ok(info) = &mut resolution.4 {
                complete_artifact_checksums(info).await?;
                if let Some(old) = previous_info {
                    preserve_legacy_metadata(&old, info);
                }
            }
            Ok::<_, eyre::Report>((ordinal, tv.request.version(), resolution))
        });
    }
    let mut completed = resolved.len() as u64;
    report.set_position(completed);
    while let Some(result) = tasks.join_next().await {
        let (ordinal, specifier, resolution) =
            match result.map_err(eyre::Report::from).and_then(|r| r) {
                Ok(result) => result,
                Err(error) => {
                    tasks.shutdown().await;
                    return Err(error);
                }
            };
        completed += 1;
        report.set_position(completed);
        report.set_message(format!(
            "{}@{} {}",
            resolution.0,
            resolution.1,
            resolution.3.to_key()
        ));
        resolved.push((ordinal, specifier, resolution));
    }
    resolved.sort_by_key(|(ordinal, _, _)| *ordinal);
    for (_, specifier, resolution) in resolved {
        let (short, version, backend, platform, info, options, conda, pkgx, status) = resolution;
        if status == LockResolutionStatus::Unsupported {
            continue;
        }
        let info = info.map_err(|error| eyre!(error))?;
        if let Some(entries) = previous.tools.get(&short) {
            for old in entries
                .iter()
                .filter(|old| {
                    old.backend.as_deref() == Some(backend.as_str())
                        && old.options == options
                        && (old.specifiers.contains(&specifier)
                            || old.version == version
                            || old.specifiers.is_empty())
                })
                .filter_map(|old| old.platforms.get(&platform.to_key()))
            {
                ensure_no_downgrade(old, &info)?;
            }
        }
        if let Some(error) = check_single_tool_provenance(
            previous.tools.get(&short).map(Vec::as_slice),
            &short,
            &version,
            &backend,
            &platform.to_key(),
            info.provenance.as_ref(),
        ) {
            report.abandon();
            bail!("{error}");
        }
        candidate.set_platform_info(
            &short,
            &version,
            Some(&backend),
            &options,
            &platform.to_key(),
            info,
        );
        candidate.bind_request(&short, &specifier, &version, &options);
        for (key, value) in conda {
            candidate.set_conda_package(&platform.to_key(), &key, value);
        }
        for (key, value) in pkgx {
            candidate.set_pkgx_package(&platform.to_key(), &key, value);
        }
    }
    candidate.cleanup_unreferenced_conda_packages();
    candidate.cleanup_unreferenced_pkgx_packages();
    report.finish_with_message(format!("{completed} targets checked"));
    progress.finished = true;
    Ok(candidate)
}

fn ensure_no_downgrade(old: &PlatformInfo, new: &PlatformInfo) -> Result<()> {
    if new.provenance < old.provenance {
        bail!(
            "lockfile generation would downgrade recorded provenance; previous files were preserved"
        );
    }
    if let Some(signer) = &old.signer
        && (new.signer.as_ref() != Some(signer) || new.attested_by != old.attested_by)
    {
        bail!(
            "lockfile generation would change the recorded signer; previous files were preserved"
        );
    }
    // Preserve identities across reordering, then pair replaced URLs in their
    // configured order so version upgrades retain the previous trust baseline.
    let mut replacements = new.additional_artifacts.iter().filter(|artifact| {
        !old.additional_artifacts
            .iter()
            .any(|old| old.url == artifact.url)
    });
    for artifact in &old.additional_artifacts {
        let replacement = new
            .additional_artifacts
            .iter()
            .find(|new| new.url == artifact.url)
            .or_else(|| replacements.next());
        if replacement.and_then(|a| a.provenance.as_ref()) < artifact.provenance.as_ref() {
            bail!(
                "lockfile generation would downgrade additional artifact provenance; previous files were preserved"
            );
        }
    }
    Ok(())
}

fn validate_provenance_settings(
    ba: &crate::cli::args::BackendArg,
    tv: &ToolVersion,
    platform: &str,
    info: &PlatformInfo,
) -> Result<()> {
    if info.provenance.is_none()
        && info
            .additional_artifacts
            .iter()
            .all(|artifact| artifact.provenance.is_none())
    {
        return Ok(());
    }
    let mut tv = tv.clone();
    tv.lock_platforms.insert(platform.to_owned(), info.clone());
    let validate = |tv: &ToolVersion| -> Result<()> {
        match tv.backend()?.get_type() {
            BackendType::Aqua => crate::backend::aqua::AquaBackend::from_arg(ba.clone())
                .ensure_provenance_setting_enabled(tv, platform),
            BackendType::Github => crate::backend::github::UnifiedGitBackend::from_arg(ba.clone())
                .ensure_provenance_setting_enabled(tv, platform),
            BackendType::Core
                if matches!(ba.full_without_opts().as_str(), "core:python" | "core:ruby") =>
            {
                crate::backend::ensure_provenance_setting_enabled(tv, platform, |provenance| {
                    let settings = Settings::get();
                    let enabled = if ba.full_without_opts() == "core:python" {
                        settings
                            .python
                            .github_attestations
                            .unwrap_or(settings.github_attestations)
                    } else {
                        settings
                            .ruby
                            .github_attestations
                            .unwrap_or(settings.github_attestations)
                    };
                    if !provenance.is_github_attestations() {
                        bail!("unexpected provenance for {tv}");
                    }
                    Ok(!enabled)
                })
            }
            _ => Ok(()),
        }
    };
    validate(&tv)?;
    for artifact in &info.additional_artifacts {
        tv.lock_platforms.get_mut(platform).unwrap().provenance = artifact.provenance.clone();
        validate(&tv)?;
    }
    Ok(())
}

struct ProgressGuard<'a> {
    report: &'a dyn crate::ui::progress_report::SingleReport,
    finished: bool,
}

impl Drop for ProgressGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.report.abandon();
        }
    }
}

async fn complete_artifact_checksums(info: &mut PlatformInfo) -> Result<()> {
    if info.checksum.is_none()
        && let Some(url) = &info.url
    {
        info.checksum = Some(artifact_checksum(url, info.url_api.as_deref()).await?);
    }
    for artifact in &mut info.additional_artifacts {
        if artifact.checksum.is_none() {
            artifact.checksum =
                Some(artifact_checksum(&artifact.url, artifact.url_api.as_deref()).await?);
        }
    }
    Ok(())
}

async fn artifact_checksum(url: &str, api_url: Option<&str>) -> Result<String> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("artifact");
    let url = if let Some(api_url) = api_url {
        crate::github::pick_reachable_asset_url(url, api_url).await
    } else {
        url.to_owned()
    };
    crate::http::HTTP.download_file(&url, &path, None).await?;
    Ok(format!(
        "sha256:{}",
        crate::hash::file_hash_sha256(&path, None)?
    ))
}

pub(crate) async fn download_for_verification(
    url: &str,
    checksum: &mut Option<String>,
) -> Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("artifact");
    crate::http::HTTP.download_file(url, &path, None).await?;
    if let Some((algorithm, expected)) = checksum.as_deref().and_then(|value| value.split_once(':'))
    {
        crate::hash::ensure_checksum(&path, expected, None, algorithm)?;
    } else {
        *checksum = Some(format!(
            "sha256:{}",
            crate::hash::file_hash_sha256(&path, None)?
        ));
    }
    Ok(directory)
}

fn preserve_legacy_metadata(old: &PlatformInfo, new: &mut PlatformInfo) {
    new.provenance_verified = None;
    if old.url == new.url && old.checksum == new.checksum {
        new.provenance_verified = old.provenance_verified;
    }
    for artifact in &mut new.additional_artifacts {
        artifact.provenance_verified = None;
        if let Some(old) = old
            .additional_artifacts
            .iter()
            .find(|old| old.url == artifact.url && old.checksum == artifact.checksum)
        {
            artifact.provenance_verified = old.provenance_verified;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendArg;
    use crate::toolset::ToolRequest;

    fn tool() -> Tool {
        let ba = BackendArg::new("fixture".into(), Some("http:fixture".into()));
        let request = ToolRequest::new_with_options(
            Arc::new(ba.clone()),
            "1",
            ToolVersionOptions::default(),
            ToolSource::Argument,
        )
        .unwrap();
        (ba, ToolVersion::new(request, "1.0".into()))
    }

    fn previous() -> Lockfile {
        let mut previous = Lockfile::default();
        for platform in ["linux-x64", "macos-arm64"] {
            previous.set_platform_info(
                "fixture",
                "1.0",
                Some("http:fixture"),
                &BTreeMap::new(),
                platform,
                PlatformInfo {
                    url: Some("https://example.invalid/never-download".into()),
                    checksum: Some("sha256:unchanged".into()),
                    provenance_verified: Some(false),
                    ..Default::default()
                },
            );
        }
        previous.bind_request("fixture", "1", "1.0", &BTreeMap::new());
        previous
    }

    #[tokio::test]
    async fn fresh_resolution_uses_the_same_backend_identity_as_reuse() {
        crate::backend::load_tools().await.unwrap();
        let ba = BackendArg::new(
            "fixture".into(),
            Some("http:fixture[rename_exe=fixture]".into()),
        );
        assert_ne!(ba.full(), ba.stored_full());
        let request = ToolRequest::new(Arc::new(ba.clone()), "1.0", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, "1.0".into());
        let result = resolve_tool_lock_info(ba.clone(), tv, Platform::current(), None).await;
        assert_eq!(result.2, ba.stored_full());
    }

    #[test]
    fn resolution_keys_ignore_option_and_environment_insertion_order() {
        let ba = BackendArg::new("fixture".into(), Some("http:fixture".into()));
        let version = |keys: [&str; 2]| {
            let mut options = ToolVersionOptions::default();
            for key in keys {
                options
                    .opts
                    .insert(key.into(), toml::Value::String(key.into()));
                options.core.install_env.insert(
                    key.into(),
                    crate::config::env_directive::EnvValue::String(key.into()),
                );
            }
            let request = ToolRequest::new_with_options(
                Arc::new(ba.clone()),
                "1",
                options,
                ToolSource::Argument,
            )
            .unwrap();
            ToolVersion::new(request, "1.0".into())
        };
        assert_eq!(
            resolution_key(&ba, &version(["a", "b"]), &Platform::current()),
            resolution_key(&ba, &version(["b", "a"]), &Platform::current()),
        );
    }

    #[tokio::test]
    async fn actual_source_fallback_wins_over_reusable_binary_metadata() {
        crate::backend::load_tools().await.unwrap();
        let (ba, mut actual) = tool();
        let platform = Platform::current();
        actual.lock_platforms.insert(
            platform.to_key(),
            PlatformInfo {
                install: Some("source".into()),
                ..Default::default()
            },
        );
        let generated = generate(
            &previous(),
            &[(ba, actual.clone())],
            std::slice::from_ref(&platform),
            false,
            false,
            1,
            &[actual],
        )
        .await
        .unwrap();
        let info = &generated.tools["fixture"][0].platforms[&platform.to_key()];
        assert_eq!(info.install.as_deref(), Some("source"));
        assert!(info.url.is_none());
        assert!(info.provenance_verified.is_none());
    }

    #[tokio::test]
    async fn unsupported_target_is_skipped_without_an_empty_entry() {
        crate::backend::load_tools().await.unwrap();
        let generated = generate(
            &Lockfile::default(),
            &[tool()],
            &[Platform::parse("windows-arm64").unwrap()],
            false,
            false,
            1,
            &[],
        )
        .await
        .unwrap();
        assert!(generated.tools.is_empty());
    }

    #[tokio::test]
    async fn unchanged_entries_reuse_metadata_without_network_and_are_stable() {
        crate::backend::load_tools().await.unwrap();
        let old = previous();
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let generated = generate(&old, &[tool()], &platforms, false, false, 2, &[])
            .await
            .unwrap();
        assert_eq!(old.tools, generated.tools);
        let again = generate(&generated, &[tool()], &platforms, false, false, 2, &[])
            .await
            .unwrap();
        assert_eq!(generated.tools, again.tools);
    }

    #[tokio::test]
    async fn cached_and_unsupported_targets_keep_cached_metadata() {
        crate::backend::load_tools().await.unwrap();
        let old = previous();
        let platforms = vec![
            Platform::parse("windows-arm64").unwrap(),
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let generated = generate(&old, &[tool()], &platforms, false, false, 2, &[])
            .await
            .unwrap();
        assert_eq!(old.tools, generated.tools);
    }

    #[tokio::test]
    async fn cached_additional_artifact_provenance_is_still_validated() {
        crate::backend::load_tools().await.unwrap();
        let ba = BackendArg::new("fixture".into(), Some("github:example/fixture".into()));
        let request = ToolRequest::new(Arc::new(ba.clone()), "1", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, "1.0".into());
        let mut old = previous();
        old.tools.get_mut("fixture").unwrap()[0].backend = Some(ba.stored_full());
        for info in old.tools.get_mut("fixture").unwrap()[0]
            .platforms
            .values_mut()
        {
            // The github backend must reject this provenance even though every
            // artifact has a checksum and the primary artifact has no provenance.
            info.additional_artifacts.push(ArtifactInfo {
                url: "https://example.invalid/extra".into(),
                checksum: Some("sha256:unchanged".into()),
                provenance: Some(ProvenanceType::Minisign),
                ..Default::default()
            });
        }
        let error = generate(
            &old,
            &[(ba, tv)],
            &[Platform::parse("linux-x64").unwrap()],
            false,
            false,
            2,
            &[],
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("unexpected provenance type"));
    }

    #[tokio::test]
    async fn current_lockfile_matches_full_generation_in_both_formats() {
        crate::backend::load_tools().await.unwrap();
        let tools = vec![tool()];
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        for version in [0, 1] {
            let mut old = previous();
            old.lockfile_version = version;
            if version == 0 {
                old.tools.get_mut("fixture").unwrap()[0].specifiers.clear();
            }
            assert!(is_current(&old, &tools, &platforms).unwrap());
            let generated = generate(&old, &tools, &platforms, false, false, 2, &[])
                .await
                .unwrap();
            assert_eq!(old.tools, generated.tools);
        }
    }

    #[tokio::test]
    async fn current_check_requires_exact_scope_and_bindings() {
        crate::backend::load_tools().await.unwrap();
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let tools = vec![tool()];
        let old = previous();
        assert!(!is_current(&old, &tools, &platforms[..1]).unwrap());
        let mut extra_platform = platforms.clone();
        extra_platform.push(Platform::parse("linux-arm64").unwrap());
        assert!(!is_current(&old, &tools, &extra_platform).unwrap());
        assert!(!is_current(&old, &[], &platforms).unwrap());
        assert!(!is_current(&old, &tools, &[]).unwrap());
        assert!(!is_current(&old, &[tool(), tool()], &platforms).unwrap());

        let mut changed = old.clone();
        changed
            .tools
            .insert("obsolete".into(), old.tools["fixture"].clone());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed
            .tools
            .get_mut("fixture")
            .unwrap()
            .push(old.tools["fixture"][0].clone());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0].version = "other-tag".into();
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0].backend = Some("http:other".into());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .options
            .insert("format".into(), "tar.gz".into());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .specifiers
            .clear();
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .specifiers
            .insert("obsolete".into());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed
            .conda_packages
            .insert("linux-x64".into(), BTreeMap::new());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed
            .pkgx_packages
            .insert("linux-x64".into(), BTreeMap::new());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
    }

    #[tokio::test]
    async fn current_check_requires_complete_artifacts_and_valid_provenance() {
        crate::backend::load_tools().await.unwrap();
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let tools = vec![tool()];
        let old = previous();
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .checksum = None;
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .url = None;
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .signer = Some("signer".into());
        assert!(!is_current(&changed, &tools, &platforms).unwrap());
        let mut changed = old.clone();
        changed.tools.get_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .additional_artifacts
            .push(ArtifactInfo {
                url: "https://example.invalid/extra".into(),
                ..Default::default()
            });
        assert!(!is_current(&changed, &tools, &platforms).unwrap());

        let ba = BackendArg::new("fixture".into(), Some("github:example/fixture".into()));
        let request = ToolRequest::new(Arc::new(ba.clone()), "1", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, "1.0".into());
        let entry = &mut changed.tools.get_mut("fixture").unwrap()[0];
        entry.backend = Some(ba.stored_full());
        let artifact = &mut entry
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .additional_artifacts[0];
        artifact.checksum = Some("sha256:unchanged".into());
        artifact.provenance = Some(ProvenanceType::Minisign);
        let error = is_current(&changed, &[(ba, tv)], &platforms).unwrap_err();
        assert!(error.to_string().contains("unexpected provenance type"));
    }

    #[tokio::test]
    async fn filtered_platform_carries_other_platform_forward() {
        crate::backend::load_tools().await.unwrap();
        let old = previous();
        let generated = generate(
            &old,
            &[tool()],
            &[Platform::parse("linux-x64").unwrap()],
            false,
            true,
            2,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(old.tools, generated.tools);
    }

    #[tokio::test]
    async fn fresh_generation_drops_obsolete_tools_but_filter_keeps_them() {
        crate::backend::load_tools().await.unwrap();
        let old = previous();
        let empty = generate(&old, &[], &[], false, false, 1, &[])
            .await
            .unwrap();
        assert!(empty.tools.is_empty());
        let retained = generate(&old, &[], &[], true, false, 1, &[]).await.unwrap();
        assert_eq!(old.tools, retained.tools);
    }

    #[test]
    fn legacy_bits_only_follow_unchanged_artifacts() {
        let old = PlatformInfo {
            url: Some("one".into()),
            checksum: Some("sha256:one".into()),
            provenance_verified: Some(false),
            ..Default::default()
        };
        let mut same = PlatformInfo {
            provenance_verified: None,
            ..old.clone()
        };
        preserve_legacy_metadata(&old, &mut same);
        assert_eq!(same.provenance_verified, Some(false));
        let mut changed = PlatformInfo {
            checksum: Some("sha256:two".into()),
            provenance_verified: None,
            ..old.clone()
        };
        preserve_legacy_metadata(&old, &mut changed);
        assert_eq!(changed.provenance_verified, None);
    }

    #[tokio::test]
    async fn erlang_keeps_distinct_linux_and_macos_options() {
        crate::backend::load_tools().await.unwrap();
        let ba = BackendArg::new("erlang".into(), Some("core:erlang".into()));
        let request = ToolRequest::new_with_options(
            Arc::new(ba.clone()),
            "28",
            ToolVersionOptions::default(),
            ToolSource::Argument,
        )
        .unwrap();
        let tv = ToolVersion::new(request, "28.0".into());
        let backend = tv.backend().unwrap();
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let mut old = Lockfile::default();
        for platform in &platforms {
            let options = backend
                .resolve_lockfile_options(&tv.request, &PlatformTarget::new(platform.clone()))
                .unwrap();
            old.set_platform_info(
                "erlang",
                "28.0",
                Some(&ba.stored_full()),
                &options,
                &platform.to_key(),
                PlatformInfo {
                    url: Some(format!("https://example.invalid/{}", platform.to_key())),
                    checksum: Some("sha256:reused".into()),
                    ..Default::default()
                },
            );
            old.bind_request("erlang", "28", "28.0", &options);
        }
        assert!(is_current(&old, &[(ba.clone(), tv.clone())], &platforms).unwrap());
        let generated = generate(&old, &[(ba, tv)], &platforms, false, false, 2, &[])
            .await
            .unwrap();
        assert_eq!(old.tools, generated.tools);
    }

    #[test]
    fn legacy_false_does_not_allow_provenance_downgrade() {
        let old = PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            provenance_verified: Some(false),
            ..Default::default()
        };
        assert!(ensure_no_downgrade(&old, &PlatformInfo::default()).is_err());
        let mut new = old.clone();
        new.provenance_verified = None;
        assert!(ensure_no_downgrade(&old, &new).is_ok());
    }

    #[test]
    fn additional_artifact_downgrade_is_rejected() {
        let old = PlatformInfo {
            additional_artifacts: vec![ArtifactInfo {
                provenance: Some(ProvenanceType::GithubAttestations),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(ensure_no_downgrade(&old, &PlatformInfo::default()).is_err());
    }

    #[test]
    fn additional_artifact_reordering_preserves_identity_and_upgrade_policy() {
        let old = PlatformInfo {
            additional_artifacts: vec![
                ArtifactInfo {
                    url: "https://example.com/verified-v1".into(),
                    provenance: Some(ProvenanceType::GithubAttestations),
                    ..Default::default()
                },
                ArtifactInfo {
                    url: "https://example.com/checksum-only".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut new = old.clone();
        new.additional_artifacts.reverse();
        assert!(ensure_no_downgrade(&old, &new).is_ok());
        new.additional_artifacts[1].url = "https://example.com/verified-v2".into();
        assert!(ensure_no_downgrade(&old, &new).is_ok());
        new.additional_artifacts[1].provenance = None;
        assert!(ensure_no_downgrade(&old, &new).is_err());
        new.additional_artifacts.pop();
        assert!(ensure_no_downgrade(&old, &new).is_err());
    }
}
