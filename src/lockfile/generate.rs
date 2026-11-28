//! Generate selected scope from requests with an immutable previous lockfile.
use super::*;
use crate::backend::static_helpers::is_weak_checksum;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{QuietReport, SingleReport};
use eyre::WrapErr;
use std::sync::atomic::{AtomicBool, Ordering};

static INSTALL_FAILED: AtomicBool = AtomicBool::new(false);
static INSTALL_FAILURE_DETAIL: Lazy<Mutex<Option<String>>> = Lazy::new(Default::default);
type ResolutionCell = Arc<tokio::sync::OnceCell<LockResolutionResult>>;
static RESOLUTIONS: Lazy<Mutex<HashMap<String, ResolutionCell>>> = Lazy::new(Default::default);
static PREPARE_SLOTS: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));

tokio::task_local! {
    /// Set while `prepare_install` generates in the background of an install, which
    /// draws no rows of its own: the lockfile is written, and reported, once the
    /// install is done.
    static IN_BACKGROUND: ();
}

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

pub fn has_previous_file(config: &Config, path: &Path) -> bool {
    path.exists()
        || monorepo_lockfile_migration_paths(config)
            .iter()
            .any(|(source, target)| target == path && source.exists())
}

pub fn read_previous(config: &Config, path: &Path, upgrade: bool) -> Result<Lockfile> {
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
        let stubs = live_tool_stub_paths(&legacy, &source);
        merge_lockfile_preserving_root(&mut previous, legacy);
        for stub in stubs {
            previous.add_tool_stub(path, &stub)?;
        }
    }
    Ok(previous)
}

pub(crate) async fn prepare_install(config: &Arc<Config>, tv: &ToolVersion) -> Result<()> {
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
    let mut candidate = IN_BACKGROUND
        .scope(
            (),
            generate(
                &previous,
                &[(tv.ba().clone(), tv.clone())],
                &platforms,
                true,
                PlatformSelection::Default,
                1,
                &[],
            ),
        )
        .await?;
    // With no foreign target (for example `lockfile_platforms` limited to the
    // host) the candidate has no entry for the tool, so there is no graph to
    // attach to.
    let graph_tools = tools_on_platforms(&[(tv.ba().clone(), tv.clone())], &platforms);
    Box::pin(populate_uv_locks(
        config,
        &mut candidate,
        &graph_tools,
        false,
        true,
        false,
    ))
    .await?;
    Ok(())
}

fn resolution_key(ba: &crate::args::BackendArg, tv: &ToolVersion, platform: &Platform) -> String {
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
    ba: crate::args::BackendArg,
    tv: ToolVersion,
    platform: Platform,
) -> Result<LockResolutionResult> {
    let key = resolution_key(&ba, &tv, &platform);
    let cell = RESOLUTIONS.lock().unwrap().entry(key).or_default().clone();
    Ok(cell
        .get_or_init(|| async {
            let backend = tv.backend().ok();
            let tool = ba.clone();
            let mut resolution = resolve_tool_lock_info(ba, tv, platform, backend).await;
            // Fetched in the tool's safe mode scope, as its install fetches them: a
            // project tool's artifact is wherever its release says it is.
            if let Ok(info) = &mut resolution.4
                && let Err(error) = crate::toolset::with_safe_mode_tool_scope(
                    &tool,
                    complete_artifact_checksums(info),
                )
                .await
            {
                resolution.4 = Err(error.to_string());
            }
            resolution
        })
        .await
        .clone())
}

pub(crate) type Tool = (crate::args::BackendArg, ToolVersion);

/// A conservative check for a complete, unfiltered warm install. Compare the
/// resolved inputs, not file timestamps: environment-dependent options and
/// verification settings can change without editing mise.toml.
///
/// Multiple requests for one short and shared dependency tables use the normal
/// generator, which owns binding conflicts and dependency-table cleanup.
pub fn is_current(
    previous: &Lockfile,
    tools: &[Tool],
    platforms: &[Platform],
    auto_prune: bool,
) -> Result<bool> {
    if tools.is_empty()
        || platforms.is_empty()
        || (auto_prune
            && previous.tools.len()
                != tools
                    .iter()
                    .filter(|(_, tv)| platforms.iter().any(|p| tv.request.is_os_supported_on(p)))
                    .count())
        || !previous.conda_packages.is_empty()
        || Settings::get().force_provenance_verify()
    {
        return Ok(false);
    }
    let mut shorts = BTreeSet::new();
    for (ba, tv) in tools {
        if !shorts.insert(&ba.short) {
            return Ok(false);
        }
        // A tool the `os` option excludes from every target has nothing to
        // lock; it is current as long as the lockfile holds no entry for it.
        if !platforms.iter().any(|p| tv.request.is_os_supported_on(p)) {
            if previous.tools_for(&ba.short).is_some() {
                return Ok(false);
            }
            continue;
        }
        let Some(entries) = previous.tools_for(&ba.short) else {
            return Ok(false);
        };
        if entries.iter().any(|entry| {
            matches!(entry.uv, Some(super::GraphRef::Inline { .. }))
                || matches!(entry.aube, Some(super::GraphRef::Inline { .. }))
        }) {
            return Ok(false);
        }
        let backend = tv.backend()?;
        let stored_backend = ba.stored_full();
        let specifier = tv.request.version();
        let mut covered: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        for platform in platforms {
            if !tv.request.is_os_supported_on(platform) {
                continue;
            }
            for platform in backend.platform_variants_for(tv, platform) {
                let options = backend
                    .lockfile_options(&tv.request, &PlatformTarget::new(platform.clone()))?;
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
                // Native graphs are universal and have no per-platform artifact rows.
                // Compare recorded identities only; hot auto-lock must stay lazy.
                if entry.uv.is_some() || entry.aube.is_some() {
                    if !bindings_current
                        || !entry.platforms.is_empty()
                        || entry.uv != tv.uv_lock
                        || entry.aube != tv.aube_lock
                    {
                        return Ok(false);
                    }
                    covered.entry(index).or_default();
                    continue;
                }
                let key = platform.to_key();
                let Some(info) = entry.platforms.get(&key) else {
                    // A platform the backend has no build for was tried already.
                    if !bindings_current || !entry.unsupported_platforms.contains(&key) {
                        return Ok(false);
                    }
                    covered.entry(index).or_default().insert(key);
                    continue;
                };
                if !bindings_current || !can_reuse(info, &tv.version) {
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
            || covered.iter().any(|(index, keys)| {
                keys.len()
                    != entries[*index].platforms.len() + entries[*index].unsupported_platforms.len()
            })
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn has_login(url: &str) -> bool {
    crate::backend::static_helpers::lock_url(url) != url
}

/// An entry whose URL names another release was left behind when its `version`
/// was rewritten (Renovate does this), so it is refreshed, not reused. An entry
/// whose URL holds a login is not current either, so the lockfile is written
/// again without it ([`reusable_without_login`]).
///
/// An entry pinned by md5 or sha1 is refreshed too, so it gets a stronger pin.
fn can_reuse(info: &PlatformInfo, version: &str) -> bool {
    info.checksum
        .as_deref()
        .is_some_and(|c| !is_weak_checksum(c))
        && info.url.as_deref().is_some_and(|url| {
            super::url_contradicts_version(version, url).is_none() && !has_login(url)
        })
        && !info.url_api.as_deref().is_some_and(has_login)
        && info.signer.is_none()
        && info.additional_artifacts.iter().all(|artifact| {
            artifact
                .checksum
                .as_deref()
                .is_some_and(|c| !is_weak_checksum(c))
                && super::url_contradicts_version(version, &artifact.url).is_none()
                && !has_login(&artifact.url)
                && !artifact.url_api.as_deref().is_some_and(has_login)
        })
        && !Settings::get().force_provenance_verify()
}

/// `info` with the login left out of its URLs, the way a backend records them
/// ([`lock_url`](crate::backend::static_helpers::lock_url)), when it is then
/// reusable. Leaving a login out does not change the artifact, so the entry
/// keeps its checksums without asking the host again, which may be down.
fn reusable_without_login(info: &PlatformInfo, version: &str) -> Option<PlatformInfo> {
    use crate::backend::static_helpers::lock_url;
    let mut info = info.clone();
    info.url = info.url.as_deref().map(lock_url);
    info.url_api = info.url_api.as_deref().map(lock_url);
    for artifact in &mut info.additional_artifacts {
        artifact.url = lock_url(&artifact.url);
        artifact.url_api = artifact.url_api.as_deref().map(lock_url);
    }
    can_reuse(&info, version).then_some(info)
}

/// The checksums of `old`, an entry whose URLs held a login, carried into
/// `new`, the entry the backend resolved again, for each artifact both name
/// (the same URL once the login is left out). Taking the login out of a URL
/// does not change the artifact, so it does not re-pin it: an entry with no
/// login is reused with the checksums it has, and so is this one.
///
/// A kept checksum keeps what `old` said about those bytes: when it is not
/// the checksum just resolved (another algorithm counts as another), the
/// provenance, its legacy `provenance_verified` and, for the main artifact,
/// the `github_attestations` probe status are the old ones too, none when
/// `old` had none, since the new provenance was checked for other bytes.
/// When the checksums are the same, the new provenance stays: it covers them.
///
/// Only a sha256, blake3 or sha512 pin is carried. An md5 or sha1 one is
/// replaced, as [`can_reuse`] has it, by [`upgrade_weak_pins_recorded_with_a_login`].
fn keep_pins_recorded_with_a_login(old: &PlatformInfo, new: &mut PlatformInfo) {
    use crate::backend::static_helpers::is_lock_url;
    let strong = |checksum: Option<&str>| checksum.is_some_and(|c| !is_weak_checksum(c));
    let same_bytes = |old: Option<&str>, new: Option<&str>| match (
        old.and_then(|c| c.split_once(':')),
        new.and_then(|c| c.split_once(':')),
    ) {
        (Some((old_algo, old_hash)), Some((new_algo, new_hash))) => {
            old_algo.eq_ignore_ascii_case(new_algo) && old_hash.eq_ignore_ascii_case(new_hash)
        }
        _ => old.is_some() && old == new,
    };
    let old_has_login = old.url.as_deref().is_some_and(has_login)
        || old.url_api.as_deref().is_some_and(has_login)
        || old
            .additional_artifacts
            .iter()
            .any(|a| has_login(&a.url) || a.url_api.as_deref().is_some_and(has_login));
    if !old_has_login {
        return;
    }
    if strong(old.checksum.as_deref())
        && let Some(url) = new.url.as_deref()
        && is_lock_url(old.url.as_deref(), url)
    {
        if !same_bytes(old.checksum.as_deref(), new.checksum.as_deref()) {
            new.provenance = old.provenance.clone();
            new.provenance_verified = old.provenance_verified;
            new.github_attestations = old.github_attestations;
        }
        new.checksum = old.checksum.clone();
        new.checksum_source = old.checksum_source;
    }
    for artifact in &mut new.additional_artifacts {
        if let Some(old) = old.additional_artifacts.iter().find(|old| {
            strong(old.checksum.as_deref()) && is_lock_url(Some(old.url.as_str()), &artifact.url)
        }) {
            if !same_bytes(old.checksum.as_deref(), artifact.checksum.as_deref()) {
                artifact.provenance = old.provenance.clone();
                artifact.provenance_verified = old.provenance_verified;
            }
            artifact.checksum = old.checksum.clone();
            artifact.checksum_source = old.checksum_source;
        }
    }
}

/// `headers`, resolved for the endpoint `new_url` and `new_api` name, when
/// [`artifact_checksum`] would send them to an endpoint of `old_url` and
/// `old_api` on the same server and scheme. A forge that moved its API keeps
/// its token: the old endpoint in `mise.lock` is another server.
fn headers_for_old_urls<'a>(
    old_url: &str,
    old_api: Option<&str>,
    new_url: Option<&str>,
    new_api: Option<&str>,
    headers: Option<&'a DownloadHeaders>,
) -> Option<&'a DownloadHeaders> {
    let sent_to = url::Url::parse(old_api.unwrap_or(old_url)).ok()?;
    let built_for = url::Url::parse(new_api.or(new_url)?).ok()?;
    (sent_to.scheme() == built_for.scheme() && crate::http::same_server(&built_for, &sent_to))
        .then_some(headers)
        .flatten()
}

/// The API URL an artifact whose entry names `download_url` and
/// `download_url_api` is downloaded from when the install downloads it from
/// that URL alone, as from a forge on a custom API (see `lock_download_urls`
/// in the github backend). It holds the login the install sends there.
pub(crate) fn api_only_download_url<'a>(
    download_url: Option<&crate::lockfile::LoginUrl>,
    download_url_api: Option<&'a crate::lockfile::LoginUrl>,
) -> Option<&'a str> {
    download_url_api
        .filter(|api| download_url == Some(*api))
        .map(crate::lockfile::LoginUrl::as_str)
}

/// Where [`upgrade_weak_pins_recorded_with_a_login`] downloads an artifact
/// recorded at `old_url` and `old_api` to check its old pin: the URL, the API
/// URL and the headers to send. An artifact the install downloads from the
/// API URL `install_api` alone is checked there, with the login that URL
/// holds and the `headers` the install sends: the browser URL could serve
/// other bytes than the attachment the new checksum hashes, and the install
/// sends no login `old` recorded. Anything else is downloaded from the URLs
/// `old` recorded, with their logins, and `headers` only reach the server of
/// `new_url` and `new_api` they were resolved for.
pub(crate) fn old_pin_download<'a>(
    (old_url, old_api): (&str, Option<&str>),
    install_api: Option<&str>,
    (new_url, new_api): (Option<&str>, Option<&str>),
    headers: Option<&'a DownloadHeaders>,
) -> (String, Option<String>, Option<&'a DownloadHeaders>) {
    if let Some(api) = install_api {
        return (api.to_owned(), Some(api.to_owned()), headers);
    }
    (
        old_url.to_owned(),
        old_api.map(str::to_owned),
        headers_for_old_urls(old_url, old_api, new_url, new_api, headers),
    )
}

/// The md5 or sha1 pins of `old`, an entry whose URLs held a login, replaced in
/// `new` by the sha256 of each artifact both name. The artifact is downloaded
/// from `old`'s URLs, with the login, or from the API URL alone the install
/// downloads it from, as [`old_pin_download`] picks, and checked against the
/// old pin and the checksum just resolved. [`complete_artifact_checksums`] would download it
/// from `new`'s URL, which leaves the login out, and a server that needs the
/// login would refuse it.
///
/// Returns the artifacts of `new` it checked, each with the old pin it checked
/// it against, so [`check_replaced_weak_pins`] does not download them again
/// without the login.
async fn upgrade_weak_pins_recorded_with_a_login(
    old: &PlatformInfo,
    new: &mut PlatformInfo,
) -> Result<Vec<CheckedPin>> {
    let mut verified = Vec::new();
    use crate::backend::static_helpers::is_lock_url;
    let weak = |checksum: Option<&str>| checksum.filter(|c| is_weak_checksum(c)).map(str::to_owned);
    if let Some(pin) = weak(old.checksum.as_deref())
        && let Some(old_url) = old.url.as_deref()
        && (has_login(old_url) || old.url_api.as_deref().is_some_and(has_login))
        && is_lock_url(Some(old_url), new.url.as_deref().unwrap_or_default())
    {
        let mut expected = vec![(pin.as_str(), LOCKED_CHECKSUM)];
        expected.extend(new.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM)));
        let (url, url_api, headers) = old_pin_download(
            (old_url, old.url_api.as_deref()),
            api_only_download_url(new.download_url.as_ref(), new.download_url_api.as_ref()),
            (new.url.as_deref(), new.url_api.as_deref()),
            new.download_headers.as_ref(),
        );
        new.checksum = Some(artifact_checksum(&url, url_api.as_deref(), headers, &expected).await?);
        new.checksum_source = None;
        verified.push(CheckedPin {
            artifact: None,
            pin,
        });
    }
    for (index, artifact) in new.additional_artifacts.iter_mut().enumerate() {
        let Some((old, pin)) = old.additional_artifacts.iter().find_map(|old| {
            let login = has_login(&old.url) || old.url_api.as_deref().is_some_and(has_login);
            (login && is_lock_url(Some(old.url.as_str()), &artifact.url))
                .then(|| weak(old.checksum.as_deref()).map(|pin| (old, pin)))
                .flatten()
        }) else {
            continue;
        };
        let mut expected = vec![(pin.as_str(), LOCKED_CHECKSUM)];
        expected.extend(artifact.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM)));
        let (url, url_api, headers) = old_pin_download(
            (&old.url, old.url_api.as_deref()),
            api_only_download_url(
                artifact.download_url.as_ref(),
                artifact.download_url_api.as_ref(),
            ),
            (Some(&artifact.url), artifact.url_api.as_deref()),
            artifact.download_headers.as_ref(),
        );
        artifact.checksum =
            Some(artifact_checksum(&url, url_api.as_deref(), headers, &expected).await?);
        artifact.checksum_source = None;
        verified.push(CheckedPin {
            artifact: Some(index),
            pin,
        });
    }
    Ok(verified)
}

/// An artifact of an entry, checked against the old pin `pin`: the main
/// artifact when `artifact` is `None`, else that additional artifact.
#[derive(Debug, PartialEq, Eq)]
struct CheckedPin {
    artifact: Option<usize>,
    pin: String,
}

/// The checksum of `old`, a conda package recorded with the channel's login,
/// carried into `new`, the same package resolved again at the same URL without
/// it: as for [`keep_pins_recorded_with_a_login`], leaving the login out does
/// not re-pin the package. An md5 or sha1 pin is replaced instead, by the
/// sha256 of the package downloaded with the login and checked against the old
/// pin and the checksum just resolved, as
/// [`upgrade_weak_pins_recorded_with_a_login`] does.
async fn keep_conda_pin_recorded_with_a_login(
    old: &crate::backend::conda::CondaPackageInfo,
    new: &mut crate::backend::conda::CondaPackageInfo,
) -> Result<()> {
    use crate::backend::static_helpers::is_lock_url;
    if !has_login(&old.url) || !is_lock_url(Some(&old.url), &new.url) {
        return Ok(());
    }
    match old.checksum.as_deref() {
        Some(pin) if is_weak_checksum(pin) => {
            let mut expected = vec![(pin, LOCKED_CHECKSUM)];
            expected.extend(new.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM)));
            new.checksum = Some(artifact_checksum(&old.url, None, None, &expected).await?);
        }
        Some(_) => new.checksum = old.checksum.clone(),
        None => {}
    }
    Ok(())
}

/// How the target platforms were chosen. Platforms the user named are replaced
/// individually and warn when a tool has no build for one of them.
pub enum PlatformSelection {
    /// Taken from the lockfile or the common default set
    Default,
    /// Listed in the `lockfile_platforms` setting
    Configured(BTreeSet<String>),
    /// Passed with `mise lock --platform`
    Explicit(BTreeSet<String>),
}

impl PlatformSelection {
    /// The platforms the user named, if they chose any
    pub fn named(&self) -> Option<BTreeSet<String>> {
        match self {
            Self::Default => None,
            Self::Configured(named) | Self::Explicit(named) => Some(named.clone()),
        }
    }
}

pub async fn generate(
    previous: &Lockfile,
    tools: &[Tool],
    platforms: &[Platform],
    retain_unselected_tools: bool,
    platform_selection: PlatformSelection,
    jobs: usize,
    installed: &[ToolVersion],
) -> Result<Lockfile> {
    let mut candidate = Lockfile {
        lockfile_version: previous.lockfile_version,
        generated_header_url: previous.generated_header_url.clone(),
        tool_stubs: previous.tool_stubs.clone(),
        ..Default::default()
    };
    // Compared with lockfile keys, which are credential-free.
    let selected: BTreeSet<String> = tools
        .iter()
        .map(|(ba, _)| crate::toolset::install_state::state_short(&ba.short).into_owned())
        .collect();
    let mut targets = Vec::new();
    let mut keys = BTreeSet::new();
    for (ba, tv) in tools {
        let backend = tv.backend()?;
        for platform in platforms {
            if !tv.request.is_os_supported_on(platform) {
                // Nothing is locked here, but an explicit selection still owns
                // these keys, so rows an earlier `os` option allowed are dropped.
                for platform in backend.platform_variants_for(tv, platform) {
                    keys.insert((
                        crate::toolset::install_state::state_short(&ba.short).into_owned(),
                        platform.to_key(),
                    ));
                }
                continue;
            }
            for platform in backend.platform_variants_for(tv, platform) {
                let options = backend
                    .lockfile_options(&tv.request, &PlatformTarget::new(platform.clone()))?;
                keys.insert((
                    crate::toolset::install_state::state_short(&ba.short).into_owned(),
                    platform.to_key(),
                ));
                targets.push((ba.clone(), tv.clone(), platform, options));
            }
        }
    }
    for (short, entries) in &previous.tools {
        for entry in entries {
            let mut untouched = entry.clone();
            if selected.contains(short) {
                if !matches!(platform_selection, PlatformSelection::Explicit(_)) {
                    continue;
                }
                let selected_key =
                    |platform: &String| keys.contains(&(short.clone(), platform.clone()));
                untouched
                    .platforms
                    .retain(|platform, _| !selected_key(platform));
                untouched
                    .unsupported_platforms
                    .retain(|platform| !selected_key(platform));
                if untouched.platforms.is_empty() && untouched.unsupported_platforms.is_empty() {
                    continue;
                }
            } else if !retain_unselected_tools {
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
    let report: Box<dyn SingleReport> = if IN_BACKGROUND.try_with(|_| ()).is_ok() {
        Box::new(QuietReport::new())
    } else {
        MultiProgressReport::get().add("lock")
    };
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
                let key = platform.to_key();
                let info = (key == backend.get_platform_key())
                    .then(|| actual.lock_platforms.get(&key).cloned())
                    .flatten()
                    .filter(|info| {
                        info.url.is_some() || info.install.is_some() || info.conda_deps.is_some()
                    })?;
                // The dependency packages the install downloaded, with the
                // checksums it pinned, go in with the entry that names them.
                let conda = actual
                    .conda_packages
                    .iter()
                    .filter(|((platform, _), _)| *platform == key)
                    .map(|((_, basename), package)| (basename.clone(), package.clone()))
                    .collect::<BTreeMap<_, _>>();
                Some((info, conda))
            });
        let previous_info = previous.tools_for(&ba.short).and_then(|entries| {
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
        let reusable = previous_info
            .as_ref()
            .and_then(|info| reusable_without_login(info, &tv.version));
        if actual.is_none()
            && let Some(info) = reusable
        {
            resolved.push((
                ordinal,
                tv.request.version(),
                (
                    crate::toolset::install_state::state_short(&ba.short).into_owned(),
                    tv.version.clone(),
                    ba.stored_full(),
                    platform,
                    Ok(info),
                    options,
                    BTreeMap::new(),
                    LockResolutionStatus::Optional,
                ),
            ));
            continue;
        }
        let semaphore = semaphore.clone();
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await?;
            let tool = ba.clone();
            let mut resolution = if let Some((info, conda)) = actual {
                (
                    crate::toolset::install_state::state_short(&ba.short).into_owned(),
                    tv.version.clone(),
                    ba.stored_full(),
                    platform,
                    Ok(info),
                    options,
                    conda,
                    LockResolutionStatus::Optional,
                )
            } else {
                resolve(ba, tv.clone(), platform).await?
            };
            if let Ok(info) = &mut resolution.4 {
                // They all download the tool's artifacts, so they run in its
                // safe mode scope, as its install does: a project tool's
                // release chooses those hosts, and no netrc login goes to them.
                let checksums = async {
                    // First: an artifact the install downloaded through a login
                    // the release gave is written only if a locked install can
                    // download it.
                    crate::lockfile::check_locked_downloads(&tool.short, &tv.version, info).await?;
                    let mut verified = Vec::new();
                    if let Some(old) = &previous_info {
                        verified = upgrade_weak_pins_recorded_with_a_login(old, info).await?;
                        keep_pins_recorded_with_a_login(old, info);
                    }
                    complete_artifact_checksums(info).await?;
                    if let Some(old) = &previous_info {
                        check_replaced_weak_pins(old, info, &verified).await?;
                    }
                    eyre::Ok(())
                };
                crate::toolset::with_safe_mode_tool_scope(&tool, checksums).await?;
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
    let mut unsupported = crate::lockfile::UnsupportedPlatforms::new(platform_selection.named());
    for (_, specifier, resolution) in resolved {
        let (short, version, backend, platform, info, options, conda, status) = resolution;
        if status == LockResolutionStatus::Unsupported {
            unsupported.record(&short, &version, &platform.to_key());
            candidate.set_unsupported_platform(
                &short,
                &version,
                Some(&backend),
                &options,
                &platform.to_key(),
            );
            candidate.bind_request(&short, &specifier, &version, &options);
            continue;
        }
        if info.as_ref().is_ok_and(|info| {
            info.url.is_some() || info.install.is_some() || info.conda_deps.is_some()
        }) {
            unsupported.record_resolved(&short, &version, &platform.to_key());
        }
        let mut info = info.map_err(|error| eyre!(error))?;
        if let Some(entries) = previous.tools_for(&short) {
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
                ensure_no_downgrade(old, &info, &backend).map_err(|error| {
                    eyre!(
                        "{error} ({short}@{version} on {}, {backend})",
                        platform.to_key()
                    )
                })?;
                carry_forge_ids(old, &mut info);
            }
        }
        if let Some(error) = check_single_tool_provenance(
            previous.tools_for(&short).map(Vec::as_slice),
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
        for (key, mut value) in conda {
            // `previous`, not `candidate`: another tool that shares the
            // package may already have written it there without the login.
            if let Some(old) = previous.get_conda_package(&platform.to_key(), &key)
                && let Err(error) = keep_conda_pin_recorded_with_a_login(old, &mut value).await
            {
                report.abandon();
                return Err(error);
            }
            candidate.set_conda_package(&platform.to_key(), &key, value);
        }
    }
    for warning in unsupported.messages() {
        warn!("{warning}");
    }
    for (short, entries) in &mut candidate.tools {
        for entry in entries.iter_mut().filter(|entry| entry.aube.is_none()) {
            entry.aube = previous.tools_for(short).and_then(|previous_entries| {
                previous_entries
                    .iter()
                    .find(|previous| {
                        previous.version == entry.version
                            && previous.backend == entry.backend
                            && previous.options == entry.options
                    })
                    .and_then(|previous| previous.aube.clone())
            });
        }
    }
    // A pin is the repository the install verified, or else the one already
    // recorded for this version. An install that showed another repository
    // than the recorded one must not replace the pin.
    for (short, entries) in &mut candidate.tools {
        for entry in entries.iter_mut() {
            let verified = installed
                .iter()
                .filter(|tv| {
                    crate::toolset::install_state::state_short(&tv.ba().short) == short.as_str()
                        && tv.version == entry.version
                        && tv.ba().stored_full() == entry.backend.clone().unwrap_or_default()
                })
                .find_map(|tv| tv.npm_provenance.clone());
            let recorded = previous
                .tools_for(short)
                .and_then(|previous_entries| {
                    // The repository is a property of the package, so a pin
                    // survives a change of install options: prefer the entry
                    // with the same options, else any of the same version.
                    let same_package = |previous: &&LockfileTool| {
                        previous.version == entry.version && previous.backend == entry.backend
                    };
                    previous_entries
                        .iter()
                        .filter(same_package)
                        .find(|previous| {
                            previous.options == entry.options && previous.provenance.is_some()
                        })
                        .or_else(|| {
                            previous_entries
                                .iter()
                                .filter(same_package)
                                .find(|previous| previous.provenance.is_some())
                        })
                })
                .and_then(|previous| previous.provenance.clone());
            if let (Some(verified), Some(recorded)) = (&verified, &recorded)
                && verified != recorded
            {
                bail!(
                    "{short}@{} was built from {}, but mise.lock pins its provenance to {}; previous files were preserved",
                    entry.version,
                    verified.repository,
                    recorded.repository
                );
            }
            entry.provenance = verified.or(recorded);
        }
    }
    for (short, entries) in &mut candidate.tools {
        for entry in entries.iter_mut().filter(|entry| entry.uv.is_none()) {
            entry.uv = previous
                .tools_for(short)
                .and_then(|entries| {
                    entries.iter().find(|old| {
                        old.version == entry.version
                            && old.backend == entry.backend
                            && old.options == entry.options
                    })
                })
                .and_then(|old| old.uv.clone());
        }
    }
    // A tool the `os` option excludes from every target has no graph to
    // resolve. Without embedded aube the full list stays, so stale graphs are
    // still cleared.
    let aube_tools = if crate::backend::npm::NPMBackend::uses_embedded_aube() {
        tools_on_platforms(tools, platforms)
    } else {
        tools.to_vec()
    };
    Box::pin(populate_aube_locks(
        &mut candidate,
        &aube_tools,
        Some(report.as_ref()),
        false,
    ))
    .await?;
    candidate.cleanup_unreferenced_conda_packages();
    report.finish_with_message(format!("{completed} targets checked"));
    progress.finished = true;
    Ok(candidate)
}

/// The tools whose `os` option allows at least one of `platforms`. The others
/// get no lockfile entry, so graph passes that bind to an entry must skip them.
pub fn tools_on_platforms(tools: &[Tool], platforms: &[Platform]) -> Vec<Tool> {
    tools
        .iter()
        .filter(|(_, tv)| platforms.iter().any(|p| tv.request.is_os_supported_on(p)))
        .cloned()
        .collect()
}

pub async fn populate_aube_locks(
    lockfile: &mut Lockfile,
    tools: &[Tool],
    report: Option<&dyn crate::ui::progress_report::SingleReport>,
    force: bool,
) -> Result<()> {
    if lockfile.lockfile_version() < 2 {
        return Ok(());
    }
    if !crate::backend::npm::NPMBackend::uses_embedded_aube() {
        let selected_npm_tools: BTreeSet<String> = tools
            .iter()
            .filter(|(ba, _)| ba.backend_type() == BackendType::Npm)
            .map(|(ba, _)| crate::toolset::install_state::state_short(&ba.short).into_owned())
            .collect();
        for (short, entries) in &mut lockfile.tools {
            if selected_npm_tools.contains(short) {
                for entry in entries {
                    entry.aube = None;
                }
            }
        }
        return Ok(());
    }
    for (ba, tv) in tools {
        let backend = tv.backend()?;
        if backend.get_type() != BackendType::Npm {
            continue;
        }
        // aube resolves a tarball URL without its optionalDependencies, so a
        // graph for it would lack packages; the lockfile holds only the version.
        if crate::backend::npm::is_tarball_spec(&ba.tool_name) {
            continue;
        }
        let options = backend.lockfile_options(&tv.request, &PlatformTarget::from_current())?;
        let backend_name = ba.stored_full();
        let npm = crate::backend::npm::NPMBackend::from_arg(ba.clone());
        if !force
            && let Some(old) = lockfile
                .tools_for(&ba.short)
                .and_then(|entries| {
                    entries.iter().find(|entry| {
                        entry.version == tv.version
                            && entry.backend.as_deref().is_none_or(|b| b == backend_name)
                            && entry.options == options
                    })
                })
                .and_then(|entry| entry.aube.as_ref())
        {
            let candidate = if old.load().is_ok() {
                Some(old.clone())
            } else {
                old.refresh().ok()
            };
            if let Some(candidate) = candidate {
                npm.validate_aube_lock(tv, candidate.load()?)?;
                lockfile.set_aube_lock(
                    &ba.short,
                    &tv.version,
                    &backend_name,
                    &options,
                    candidate,
                )?;
                continue;
            }
        }
        if let Some(report) = report {
            report.set_message(format!("{}@{} dependencies", ba.short, tv.version));
        }
        let graph = npm.resolve_aube_lock(tv).await?;
        lockfile.set_aube_lock(&ba.short, &tv.version, &backend_name, &options, graph)?;
    }
    Ok(())
}

pub async fn populate_uv_locks(
    config: &Arc<Config>,
    lockfile: &mut Lockfile,
    tools: &[Tool],
    force: bool,
    best_effort: bool,
    strict_options: bool,
) -> Result<()> {
    if lockfile.lockfile_version() < 2 {
        return Ok(());
    }
    for (ba, tv) in tools {
        if ba.backend_type() != BackendType::Pipx {
            continue;
        }
        let backend = crate::backend::pipx::PIPXBackend::from_arg(ba.clone());
        let options = backend.lockfile_options(&tv.request, &PlatformTarget::from_current())?;
        let backend_name = ba.stored_full();
        // Tools the user named explicitly still fail on installer arguments uv
        // cannot lock; a sweep over every tool degrades to version-only.
        let options_supported = backend.uv_lock_options_supported(tv);
        if strict_options && !options_supported {
            backend.validate_lock_options(tv)?;
        }
        if !options_supported || !backend.uv_lock_allowed(tv) {
            if !options_supported && !best_effort {
                warn!(
                    "{} dependency graph skipped because uvx_args or pipx_args are configured; locking with version-only resolution",
                    ba.short
                );
            }
            if let Some(entries) = lockfile.tools_for_mut(&ba.short) {
                for entry in entries
                    .iter_mut()
                    .filter(|entry| entry.version == tv.version && entry.options == options)
                {
                    entry.uv = None;
                }
            }
            continue;
        }
        let locked = lockfile
            .tools_for(&ba.short)
            .and_then(|entries| {
                entries.iter().find(|entry| {
                    entry.version == tv.version
                        && entry.backend.as_deref().is_none_or(|b| b == backend_name)
                        && entry.options == options
                })
            })
            .and_then(|entry| entry.uv.clone());
        if let Some(lock) = &locked
            && !force
        {
            let candidate = if lock.load().is_ok() {
                Some(lock.clone())
            } else {
                lock.refresh().ok()
            };
            if let Some(candidate) = candidate {
                backend.validate_uv_lock(tv, candidate.load()?)?;
                lockfile.set_uv_lock(&ba.short, &tv.version, &backend_name, &options, candidate)?;
                continue;
            }
        }
        if backend
            .spawnable_dependency(config, None, "uv")
            .await
            .is_none()
        {
            warn!(
                "{} dependency graph skipped: uv >= 0.12.10 is unavailable; install uv and run `mise lock` again",
                ba.short
            );
            continue;
        }
        if let Err(error) = backend.validate_lock_options(tv) {
            if best_effort {
                debug!(
                    "{} dependency graph skipped during automatic locking: {error:#}",
                    ba.short
                );
                continue;
            }
            return Err(error);
        }
        let graph = match backend.resolve_uv_lock(config, tv).await {
            Ok(graph) => graph,
            Err(error) if best_effort => {
                debug!(
                    "{} dependency graph skipped during automatic locking: {error:#}",
                    ba.short
                );
                continue;
            }
            Err(error) => {
                warn!(
                    "{} dependency graph unavailable; leaving the lock entry as it is: {error:#}",
                    ba.short
                );
                // Keep whatever graph the lockfile already has: a failure to resolve
                // here may only reflect this machine, not that the graph is stale.
                continue;
            }
        };
        // A forced resolution runs under a cutoff taken from the current time.
        // When it selects the packages that are already locked, the locked
        // graph stays, so the lockfile changes only when the resolution does.
        let graph = match locked {
            Some(locked) if locked.resolves_like(&graph) => locked,
            _ => graph,
        };
        lockfile.set_uv_lock(&ba.short, &tv.version, &backend_name, &options, graph)?;
    }
    Ok(())
}

/// Keep the forge IDs an old Packslip entry recorded when the new one, for
/// the same signer, recorded none: explicit signer options verify a release
/// without the forge check, and an older certificate carries no IDs. Neither
/// says the repository changed, so the commitment stays for the next
/// resolution that does check it. Another signer cannot drop them:
/// [`crate::packslip_forge::lock_entry_continues`] refuses it.
///
/// Likewise the owner ID alone, which only an older mise recorded, when the
/// new entry records the same repository: it is ignored, but kept so that an
/// unchanged entry is not rewritten.
fn carry_forge_ids(old: &PlatformInfo, new: &mut PlatformInfo) {
    if new.signer.is_some() && new.signer == old.signer && new.repository_id.is_none() {
        new.repository_id = old.repository_id.clone();
        new.repository_owner_id = old.repository_owner_id.clone();
    }
    if new.repository_owner_id.is_none()
        && new.repository_id.is_some()
        && new.repository_id == old.repository_id
    {
        new.repository_owner_id = old.repository_owner_id.clone();
    }
}

/// Names a recorded provenance for an error message.
fn describe_provenance(provenance: Option<&ProvenanceType>) -> String {
    provenance.map_or_else(|| "no provenance".to_string(), ToString::to_string)
}

/// Names a signer and its attestation for an error message.
fn describe_signer(info: &PlatformInfo) -> String {
    match (&info.signer, &info.attested_by) {
        (Some(signer), Some(by)) => format!("{signer} (attested by {by})"),
        (Some(signer), None) => signer.clone(),
        (None, _) => "no signer".to_string(),
    }
}

/// An aqua recipe that declares SLSA without a signer cannot continue a
/// recorded SLSA entry; the tool options supply the signer and keep it verified.
fn slsa_signer_hint(old: &PlatformInfo, new: &PlatformInfo, backend: &str) -> &'static str {
    if backend.starts_with("aqua:")
        && old.provenance.as_ref().is_some_and(ProvenanceType::is_slsa)
        && new.provenance.is_none()
    {
        ", or, if the recipe names the SLSA provenance asset but no signer, set \
         slsa_signer_identity and slsa_signer_issuer in the tool options so the SLSA \
         provenance can be verified and recorded again"
    } else {
        ""
    }
}

fn ensure_no_downgrade(old: &PlatformInfo, new: &PlatformInfo, backend: &str) -> Result<()> {
    // A verified Packslip signer is the replacement trust baseline for an
    // artifact authenticated by its signed release manifest. Older mise versions
    // could leave provenance from the previous backend (GitHub attestations,
    // cosign, minisign) on a Packslip entry, but complete generation
    // intentionally does not persist that unverified link. SLSA stays
    // ratcheted. Without a signer, retain the ordinary provenance ratchet.
    let packslip_signer_replaces_provenance =
        backend.starts_with("packslip:") && new.signer.is_some();
    if provenance_is_downgrade(
        old.provenance.as_ref(),
        new.provenance.as_ref(),
        packslip_signer_replaces_provenance,
    ) {
        bail!(
            "lockfile generation would downgrade recorded provenance; previous files were \
             preserved: recorded {} but the new entry has {}. If the registry recipe no longer \
             provides what mise needs to verify it, fix the recipe or lock the tool through \
             another backend{}",
            describe_provenance(old.provenance.as_ref()),
            describe_provenance(new.provenance.as_ref()),
            slsa_signer_hint(old, new, backend),
        );
    }
    // A Packslip entry's signer continues as the same string, or, when both
    // entries record the same forge repository ID, as the same workflow of
    // it under the name it has now. A changed repository ID is a different
    // repository even under the same name and signer: install refuses it,
    // and so does this.
    let project = backend
        .strip_prefix("packslip:")
        .and_then(|name| crate::backend::packslip::project_name(name).ok());
    if old.signer.is_some()
        && (!crate::packslip_forge::lock_entry_continues(project.as_deref(), old, new)
            || new.attested_by != old.attested_by)
    {
        bail!(
            "lockfile generation would change the recorded signer; previous files were \
             preserved: recorded {} but the new entry has {}",
            describe_signer(old),
            describe_signer(new),
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
        if provenance_is_downgrade(
            artifact.provenance.as_ref(),
            replacement.and_then(|a| a.provenance.as_ref()),
            packslip_signer_replaces_provenance,
        ) {
            bail!(
                "lockfile generation would downgrade additional artifact provenance; previous \
                 files were preserved: {} recorded {} but the new entry has {}",
                artifact.url,
                describe_provenance(artifact.provenance.as_ref()),
                describe_provenance(replacement.and_then(|a| a.provenance.as_ref())),
            );
        }
    }
    Ok(())
}

pub(super) fn provenance_is_downgrade(
    old: Option<&ProvenanceType>,
    new: Option<&ProvenanceType>,
    packslip_signer_replaces_provenance: bool,
) -> bool {
    if packslip_signer_replaces_provenance && old.is_some_and(|old| !old.is_slsa()) {
        return false;
    }
    new < old
}

fn validate_provenance_settings(
    ba: &crate::args::BackendArg,
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

/// Gives every artifact a sha256, blake3 or sha512 checksum. One with none is
/// downloaded and hashed. One with an md5 or sha1 checksum, which the release
/// publisher chose and could make a second file match, is downloaded, checked
/// against it, and pinned by its sha256 instead. A checksum taken from the
/// download is confirmed, so it loses any [`ChecksumSource`](crate::lockfile::ChecksumSource) mark.
async fn complete_artifact_checksums(info: &mut PlatformInfo) -> Result<()> {
    if info.checksum.as_deref().is_none_or(is_weak_checksum)
        && let Some(url) = download_url(info)
    {
        let expected = info.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM));
        info.checksum = Some(
            artifact_checksum(
                url,
                download_url_api(info),
                info.download_headers.as_ref(),
                expected.as_slice(),
            )
            .await?,
        );
        info.checksum_source = None;
    }
    for artifact in &mut info.additional_artifacts {
        if artifact.checksum.as_deref().is_none_or(is_weak_checksum) {
            let expected = artifact.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM));
            let (url, url_api) = artifact_download_urls(artifact);
            artifact.checksum = Some(
                artifact_checksum(
                    &url,
                    url_api.as_deref(),
                    artifact.download_headers.as_ref(),
                    expected.as_slice(),
                )
                .await?,
            );
            artifact.checksum_source = None;
        }
    }
    Ok(())
}

/// The URL the main artifact of `info` is downloaded from: the configured one,
/// which may hold a login the entry leaves out.
fn download_url(info: &PlatformInfo) -> Option<&str> {
    info.download_url
        .as_ref()
        .map(crate::lockfile::LoginUrl::as_str)
        .or(info.url.as_deref())
}

/// The API URL the main artifact of `info` is downloaded from, as
/// [`download_url`] is the browser URL.
fn download_url_api(info: &PlatformInfo) -> Option<&str> {
    info.download_url_api
        .as_ref()
        .map(crate::lockfile::LoginUrl::as_str)
        .or(info.url_api.as_deref())
}

/// The URL and API URL `artifact` is downloaded from, with the logins of its
/// configured URLs, as [`download_url`] and [`download_url_api`] give them.
fn artifact_download_urls(artifact: &ArtifactInfo) -> (String, Option<String>) {
    let url = artifact
        .download_url
        .as_ref()
        .map_or(artifact.url.as_str(), crate::lockfile::LoginUrl::as_str);
    let url_api = artifact
        .download_url_api
        .as_ref()
        .map(crate::lockfile::LoginUrl::as_str)
        .or(artifact.url_api.as_deref());
    (url.to_owned(), url_api.map(str::to_owned))
}

const RELEASE_CHECKSUM: &str = "the checksum its release lists";
const LOCKED_CHECKSUM: &str = "the checksum mise.lock pinned it by";

/// A refreshed entry replaces the md5 or sha1 checksum `old` pinned an
/// artifact by. When the artifact is still at the same URL, it is downloaded
/// and checked against that pin as well as the new checksum, so the refresh
/// cannot move the pin to other bytes the URL now serves. Each artifact is
/// downloaded through the logins of its configured URL and API URL, which the
/// entry leaves out. The artifacts in `verified` were already checked against that same old
/// pin, through the login the old entry recorded, and are not downloaded again.
async fn check_replaced_weak_pins(
    old: &PlatformInfo,
    new: &PlatformInfo,
    verified: &[CheckedPin],
) -> Result<()> {
    let checked = |artifact: Option<usize>, pin: &str| {
        verified
            .iter()
            .any(|checked| checked.artifact == artifact && checked.pin == pin)
    };
    let replaced = |old: &Option<String>, new: &Option<String>| {
        old.as_deref()
            .filter(|old| is_weak_checksum(old) && Some(*old) != new.as_deref())
            .map(str::to_owned)
    };
    if let Some(url) = download_url(new)
        && new.url.is_some()
        && old.url == new.url
        && let Some(pin) = replaced(&old.checksum, &new.checksum)
        && !checked(None, &pin)
    {
        let mut expected = vec![(pin.as_str(), LOCKED_CHECKSUM)];
        expected.extend(new.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM)));
        artifact_checksum(
            url,
            download_url_api(new),
            new.download_headers.as_ref(),
            &expected,
        )
        .await?;
    }
    for (index, artifact) in new.additional_artifacts.iter().enumerate() {
        let Some(pin) = old
            .additional_artifacts
            .iter()
            .find(|old| old.url == artifact.url)
            .and_then(|old| replaced(&old.checksum, &artifact.checksum))
            .filter(|pin| !checked(Some(index), pin))
        else {
            continue;
        };
        let mut expected = vec![(pin.as_str(), LOCKED_CHECKSUM)];
        expected.extend(artifact.checksum.as_deref().map(|c| (c, RELEASE_CHECKSUM)));
        let (url, url_api) = artifact_download_urls(artifact);
        artifact_checksum(
            &url,
            url_api.as_deref(),
            artifact.download_headers.as_ref(),
            &expected,
        )
        .await?;
    }
    Ok(())
}

/// The sha256 of the artifact at `url`, after checking it against each
/// `expected` checksum (`algorithm:hash`, and what it is for the error).
async fn artifact_checksum(
    url: &str,
    api_url: Option<&str>,
    headers: Option<&DownloadHeaders>,
    expected: &[(&str, &str)],
) -> Result<String> {
    let check = |path: &std::path::Path| -> Result<()> {
        for (checksum, what) in expected {
            if let Some((algorithm, hash)) = checksum.split_once(':') {
                crate::hash::ensure_checksum(path, hash, None, algorithm).wrap_err_with(|| {
                    let url = crate::backend::static_helpers::lock_url(url);
                    format!("{url} does not match {what}")
                })?;
            }
        }
        Ok(())
    };
    // A `file://` artifact is hashed in place, as the http backend installs it.
    if let Some(local) = crate::backend::http::local_artifact_path(url)? {
        check(&local)?;
        return Ok(format!(
            "sha256:{}",
            crate::hash::file_hash_sha256(&local, None)?
        ));
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("artifact");
    let url = if let Some(api_url) = api_url {
        crate::github::pick_reachable_asset_url(url, api_url).await
    } else {
        url.to_owned()
    };
    // Headers resolved for the API asset endpoint (a forge token) are only valid
    // there; the browser URL was reachable without them.
    let headers = headers.filter(|_| api_url.is_none_or(|api_url| api_url == url));
    download_with_headers(&url, &path, headers).await?;
    check(&path)?;
    Ok(format!(
        "sha256:{}",
        crate::hash::file_hash_sha256(&path, None)?
    ))
}

/// Download `url` to `path` with `headers`, through the downloader that sends
/// them at install time (see [`DownloadHeaders`]).
async fn download_with_headers(
    url: &str,
    path: &std::path::Path,
    headers: Option<&DownloadHeaders>,
) -> Result<()> {
    match headers {
        Some(DownloadHeaders::Plugin(headers)) if !headers.is_empty() => {
            crate::backend::vfox::download_with_plugin_headers(url, path, headers).await
        }
        Some(DownloadHeaders::Client(headers)) if !headers.is_empty() => {
            let headers = crate::http::with_host_auth(&reqwest::Url::parse(url)?, headers)?;
            crate::http::HTTP
                .download_file_with_headers(url, path, &headers, None)
                .await
        }
        _ => crate::http::HTTP.download_file(url, path, None).await,
    }
}

/// [`download_for_verification`], after which an md5 or sha1 `checksum` the
/// download matched is replaced by the download's sha256.
pub(crate) async fn download_for_strong_checksum(
    url: &str,
    headers: Option<&DownloadHeaders>,
    checksum: &mut Option<String>,
) -> Result<()> {
    let directory = download_with_headers_for_verification(url, headers, checksum).await?;
    if checksum.as_deref().is_some_and(is_weak_checksum) {
        *checksum = Some(format!(
            "sha256:{}",
            crate::hash::file_hash_sha256(&directory.path().join("artifact"), None)?
        ));
    }
    Ok(())
}

pub(crate) async fn download_for_verification(
    url: &str,
    checksum: &mut Option<String>,
) -> Result<tempfile::TempDir> {
    download_with_headers_for_verification(url, None, checksum).await
}

/// [`download_for_verification`], sending `headers` with the request.
async fn download_with_headers_for_verification(
    url: &str,
    headers: Option<&DownloadHeaders>,
    checksum: &mut Option<String>,
) -> Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("artifact");
    match crate::backend::http::local_artifact_path(url)? {
        Some(local) => crate::file::copy(&local, &path)?,
        None => download_with_headers(url, &path, headers).await?,
    }
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

/// `provenance_verified` of `old` carried into `new` for each artifact that is
/// the same: the same checksum at the same URL, a login left out, since the
/// entry is written without the login an old one recorded.
fn preserve_legacy_metadata(old: &PlatformInfo, new: &mut PlatformInfo) {
    use crate::backend::static_helpers::lock_url;
    new.provenance_verified = None;
    let same_url = match (old.url.as_deref(), new.url.as_deref()) {
        (Some(old), Some(new)) => lock_url(old) == lock_url(new),
        (old, new) => old == new,
    };
    if same_url && old.checksum == new.checksum {
        new.provenance_verified = old.provenance_verified;
    }
    for artifact in &mut new.additional_artifacts {
        artifact.provenance_verified = None;
        if let Some(old) = old.additional_artifacts.iter().find(|old| {
            lock_url(&old.url) == lock_url(&artifact.url) && old.checksum == artifact.checksum
        }) {
            artifact.provenance_verified = old.provenance_verified;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::BackendArg;
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
            PlatformSelection::Default,
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

    /// The installed tool, marked by an install that downloaded its artifact
    /// through a login the release gave: what `generate` writes for it. A
    /// download is shared by request within a process, so each call names
    /// its own `file`.
    async fn generate_marked_install(
        server: &mockito::Server,
        file: &str,
        checksum: Option<String>,
    ) -> Result<PlatformInfo> {
        crate::backend::load_tools().await.unwrap();
        let (ba, mut actual) = tool();
        let platform = Platform::current();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("private-token", "t0ken".parse().unwrap());
        let url = format!("{}/{file}", server.url());
        actual.lock_platforms.insert(
            platform.to_key(),
            PlatformInfo {
                url: Some(url.clone()),
                checksum,
                locked_download: Some(crate::lockfile::LockedDownload {
                    name: file.into(),
                    url: crate::lockfile::LoginUrl::new(&url),
                    headers,
                }),
                ..Default::default()
            },
        );
        let generated = generate(
            &previous(),
            &[(ba, actual.clone())],
            std::slice::from_ref(&platform),
            false,
            PlatformSelection::Default,
            1,
            &[actual],
        )
        .await?;
        Ok(generated.tools["fixture"][0].platforms[&platform.to_key()].clone())
    }

    /// An installed artifact downloaded through a login the release gave is
    /// written once a locked install's download of it, with only the forge
    /// token, serves the bytes the install pinned. A missing pin becomes the
    /// sha256 of what that download served.
    #[tokio::test]
    async fn installed_release_login_artifact_is_written_once_a_locked_install_can_fetch_it() {
        let mut server = mockito::Server::new_async().await;
        let mut mocks = Vec::new();
        for file in ["pinned.tar.gz", "unpinned.tar.gz"] {
            let mock = server
                .mock("GET", format!("/{file}").as_str())
                .match_header("private-token", "t0ken")
                .with_body("artifact")
                .create_async()
                .await;
            mocks.push(mock);
        }
        let pinned = format!("sha256:{ARTIFACT_SHA256}");
        let info = generate_marked_install(&server, "pinned.tar.gz", Some(pinned.clone()))
            .await
            .unwrap();
        assert_eq!(info.checksum.as_deref(), Some(pinned.as_str()));
        assert!(info.locked_download.is_none());
        let info = generate_marked_install(&server, "unpinned.tar.gz", None)
            .await
            .unwrap();
        assert_eq!(info.checksum.as_deref(), Some(pinned.as_str()));
        for mock in mocks {
            mock.assert_async().await;
        }
    }

    /// When a locked install could not fetch the installed artifact, the
    /// lockfile update fails with `mise lock`'s advice and says the tool is
    /// installed; so it does when the server serves other bytes than the
    /// install pinned.
    #[tokio::test]
    async fn installed_release_login_artifact_a_locked_install_cannot_fetch_is_not_written() {
        let mut server = mockito::Server::new_async().await;
        let refused = server
            .mock("GET", "/refused.tar.gz")
            .with_status(401)
            .create_async()
            .await;
        let pinned = format!("sha256:{ARTIFACT_SHA256}");
        let error = generate_marked_install(&server, "refused.tar.gz", Some(pinned.clone()))
            .await
            .unwrap_err();
        refused.assert_async().await;
        let message = format!("{error:#}");
        assert!(
            message.contains("fixture@1.0 is installed, but mise.lock was not updated for it"),
            "{message}"
        );
        assert!(
            message.contains("Put the login in one of those options"),
            "{message}"
        );

        let other = server
            .mock("GET", "/other.tar.gz")
            .with_body("other bytes")
            .create_async()
            .await;
        let error = generate_marked_install(&server, "other.tar.gz", Some(pinned))
            .await
            .unwrap_err();
        other.assert_async().await;
        assert!(
            format!("{error:#}").contains("mise.lock was not updated for it"),
            "{error:#}"
        );
    }

    /// The dependency packages an install recorded, with their checksums, are
    /// written with the installed entry that names them.
    #[tokio::test]
    async fn installed_conda_dependencies_are_written_with_the_installed_entry() {
        use crate::backend::conda::CondaPackageInfo;
        crate::backend::load_tools().await.unwrap();
        let (ba, mut actual) = tool();
        let platform = Platform::current();
        actual.lock_platforms.insert(
            platform.to_key(),
            PlatformInfo {
                url: Some("https://conda.example/linux-64/fixture-1.0-h0_0.conda".into()),
                checksum: Some("sha256:main".into()),
                conda_deps: Some(vec!["zlib-1.3-h0_0".into()]),
                ..Default::default()
            },
        );
        let zlib = CondaPackageInfo {
            url: "https://conda.example/linux-64/zlib-1.3-h0_0.conda".into(),
            checksum: Some("sha256:zlib".into()),
        };
        actual
            .conda_packages
            .insert((platform.to_key(), "zlib-1.3-h0_0".into()), zlib.clone());
        actual.conda_packages.insert(
            ("other-platform".into(), "zlib-1.3-h0_0".into()),
            CondaPackageInfo {
                url: "https://conda.example/other/zlib-1.3-h0_0.conda".into(),
                checksum: None,
            },
        );
        let generated = generate(
            &previous(),
            &[(ba, actual.clone())],
            std::slice::from_ref(&platform),
            false,
            PlatformSelection::Default,
            1,
            &[actual],
        )
        .await
        .unwrap();
        assert_eq!(
            generated.get_conda_package(&platform.to_key(), "zlib-1.3-h0_0"),
            Some(&zlib)
        );
        assert!(
            generated
                .get_conda_package("other-platform", "zlib-1.3-h0_0")
                .is_none()
        );
    }

    #[tokio::test]
    async fn unsupported_target_is_recorded_without_an_empty_entry() {
        crate::backend::load_tools().await.unwrap();
        let generated = generate(
            &Lockfile::default(),
            &[tool()],
            &[Platform::parse("windows-arm64").unwrap()],
            false,
            PlatformSelection::Default,
            1,
            &[],
        )
        .await
        .unwrap();
        let entry = &generated.tools["fixture"][0];
        assert!(entry.platforms.is_empty());
        assert_eq!(
            entry.unsupported_platforms,
            BTreeSet::from(["windows-arm64".to_string()])
        );
    }

    #[tokio::test]
    async fn supported_target_is_kept_when_another_target_is_unsupported() {
        crate::backend::load_tools().await.unwrap();
        let generated = generate(
            &previous(),
            &[tool()],
            &[
                Platform::parse("linux-x64").unwrap(),
                Platform::parse("windows-arm64").unwrap(),
            ],
            false,
            PlatformSelection::Default,
            2,
            &[],
        )
        .await
        .unwrap();
        let entry = &generated.tools["fixture"][0];
        assert!(entry.platforms.contains_key("linux-x64"));
        assert!(!entry.platforms.contains_key("windows-arm64"));
        assert!(entry.unsupported_platforms.contains("windows-arm64"));
    }

    #[tokio::test]
    async fn unchanged_entries_reuse_metadata_without_network_and_are_stable() {
        crate::backend::load_tools().await.unwrap();
        let old = previous();
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let generated = generate(
            &old,
            &[tool()],
            &platforms,
            false,
            PlatformSelection::Default,
            2,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(old.tools, generated.tools);
        let again = generate(
            &generated,
            &[tool()],
            &platforms,
            false,
            PlatformSelection::Default,
            2,
            &[],
        )
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
        let generated = generate(
            &old,
            &[tool()],
            &platforms,
            false,
            PlatformSelection::Default,
            2,
            &[],
        )
        .await
        .unwrap();
        let mut expected = old.tools.clone();
        expected.get_mut("fixture").unwrap()[0]
            .unsupported_platforms
            .insert("windows-arm64".into());
        assert_eq!(expected, generated.tools);
        // The next run finds the listed platform current.
        assert!(is_current(&generated, &[tool()], &platforms, true).unwrap());
    }

    #[tokio::test]
    async fn cached_additional_artifact_provenance_is_still_validated() {
        crate::backend::load_tools().await.unwrap();
        let ba = BackendArg::new("fixture".into(), Some("github:example/fixture".into()));
        let request = ToolRequest::new(Arc::new(ba.clone()), "1", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, "1.0".into());
        let mut old = previous();
        old.tools_for_mut("fixture").unwrap()[0].backend = Some(ba.stored_full());
        for info in old.tools_for_mut("fixture").unwrap()[0]
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
            PlatformSelection::Default,
            2,
            &[],
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("unexpected provenance type"));
    }

    #[tokio::test]
    async fn current_native_graph_does_not_read_sidecars() {
        crate::backend::load_tools().await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let graph = super::super::GraphRef::Sidecar {
            dir: temp.path().join("missing"),
            base: temp.path().to_path_buf(),
            digest: "sha256:recorded".into(),
            cell: std::sync::OnceLock::new(),
        };
        let mut old = previous();
        let entry = &mut old.tools_for_mut("fixture").unwrap()[0];
        entry.platforms.clear();
        entry.uv = Some(graph.clone());
        let (ba, mut tv) = tool();
        tv.uv_lock = Some(graph);
        assert!(
            is_current(
                &old,
                &[(ba, tv)],
                &[Platform::parse("linux-x64").unwrap()],
                true,
            )
            .unwrap()
        );
        assert!(!temp.path().join("missing").exists());
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
                old.tools_for_mut("fixture").unwrap()[0].specifiers.clear();
            }
            assert!(is_current(&old, &tools, &platforms, true).unwrap());
            let generated = generate(
                &old,
                &tools,
                &platforms,
                false,
                PlatformSelection::Default,
                2,
                &[],
            )
            .await
            .unwrap();
            assert_eq!(old.tools, generated.tools);
        }
    }

    #[tokio::test]
    async fn tool_excluded_from_every_platform_by_os_is_current() {
        crate::backend::load_tools().await.unwrap();
        let ba = BackendArg::new("fixture".into(), Some("http:fixture".into()));
        let mut options = ToolVersionOptions::default();
        options.core.os = Some(vec!["windows".into()]);
        let request =
            ToolRequest::new_with_options(Arc::new(ba.clone()), "1", options, ToolSource::Argument)
                .unwrap();
        let tools = vec![(ba, ToolVersion::new(request, "1.0".into()))];
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        for auto_prune in [false, true] {
            assert!(is_current(&Lockfile::default(), &tools, &platforms, auto_prune).unwrap());
        }
        // A stale entry for the excluded tool must still be pruned.
        for auto_prune in [false, true] {
            assert!(!is_current(&previous(), &tools, &platforms, auto_prune).unwrap());
        }
    }

    #[test]
    fn entry_with_another_releases_url_is_not_reusable() {
        let mut info = PlatformInfo {
            url: Some("https://github.com/o/r/releases/download/v2.1.17/r_2.1.17_arm64.gz".into()),
            checksum: Some("sha256:old".into()),
            ..Default::default()
        };
        assert!(can_reuse(&info, "2.1.17"));
        assert!(!can_reuse(&info, "2.2.2"));
        info.url = Some("https://example.invalid/never-download".into());
        assert!(can_reuse(&info, "2.2.2"));
        info.additional_artifacts
            .push(crate::lockfile::ArtifactInfo {
                checksum: Some("sha256:old".into()),
                url: "https://github.com/o/r/releases/download/v2.1.17/r.sig".into(),
                ..Default::default()
            });
        assert!(can_reuse(&info, "2.1.17"));
        assert!(!can_reuse(&info, "2.2.2"));
    }

    #[test]
    fn entry_whose_url_holds_a_login_is_not_reusable() {
        let mut info = PlatformInfo {
            url: Some("https://ci:token@mirror.example/v20.0.0/node-linux-x64.tar.gz".into()),
            checksum: Some("sha256:old".into()),
            ..Default::default()
        };
        assert!(!can_reuse(&info, "20.0.0"));
        info.url = Some("https://mirror.example/v20.0.0/node-linux-x64.tar.gz".into());
        assert!(can_reuse(&info, "20.0.0"));
        info.additional_artifacts
            .push(crate::lockfile::ArtifactInfo {
                checksum: Some("sha256:old".into()),
                url: "https://ci@mirror.example/v20.0.0/SHASUMS256.txt.sig".into(),
                ..Default::default()
            });
        assert!(!can_reuse(&info, "20.0.0"));
    }

    /// An entry that is complete but for the login is reused with it left
    /// out, so `mise lock` does not need the host to keep its checksums.
    #[test]
    fn entry_whose_url_holds_a_login_is_reused_without_it() {
        let info = PlatformInfo {
            url: Some("https://ci:token@mirror.example/v20.0.0/node-linux-x64.tar.gz".into()),
            checksum: Some("sha256:old".into()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                checksum: Some("sha256:sig".into()),
                url: "https://ci@mirror.example/v20.0.0/SHASUMS256.txt.sig".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let reused = reusable_without_login(&info, "20.0.0").unwrap();
        assert_eq!(
            reused.url.as_deref(),
            Some("https://mirror.example/v20.0.0/node-linux-x64.tar.gz")
        );
        assert_eq!(
            reused.additional_artifacts[0].url,
            "https://mirror.example/v20.0.0/SHASUMS256.txt.sig"
        );
        assert_eq!(reused.checksum.as_deref(), Some("sha256:old"));
        assert_eq!(
            reused.additional_artifacts[0].checksum.as_deref(),
            Some("sha256:sig")
        );
        // The other checks still hold.
        let mut signed = info.clone();
        signed.signer = Some("ed25519:key".into());
        assert!(reusable_without_login(&signed, "20.0.0").is_none());
        let mut incomplete = info.clone();
        incomplete.checksum = None;
        assert!(reusable_without_login(&incomplete, "20.0.0").is_none());
    }

    #[test]
    fn checksums_recorded_with_a_login_stay_pinned() {
        let entry = |url: &str, checksum: &str, artifact: &str| PlatformInfo {
            url: Some(url.into()),
            checksum: Some(checksum.into()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                checksum: Some(format!("{checksum}-sig")),
                url: artifact.into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let tarball = "https://mirror.example/v20.0.0/node-linux-x64.tar.gz";
        let sig = "https://mirror.example/v20.0.0/SHASUMS256.txt.sig";
        let with_login = |url: &str| url.replace("https://", "https://ci:token@");
        // (old entry, newly resolved entry, checksums the new entry keeps)
        let cases = [
            // The same artifacts without the login keep the recorded pins.
            (
                entry(&with_login(tarball), "sha256:old", &with_login(sig)),
                entry(tarball, "sha256:new", sig),
                ("sha256:old", "sha256:old-sig"),
            ),
            // Only the artifact URL held a login.
            (
                entry(tarball, "sha256:old", &with_login(sig)),
                entry(tarball, "sha256:new", sig),
                ("sha256:old", "sha256:old-sig"),
            ),
            // Another artifact is resolved on its own.
            (
                entry(&with_login(tarball), "sha256:old", &with_login(sig)),
                entry(
                    "https://mirror.example/v20.0.1/node-linux-x64.tar.gz",
                    "sha256:new",
                    "https://mirror.example/v20.0.1/SHASUMS256.txt.sig",
                ),
                ("sha256:new", "sha256:new-sig"),
            ),
            // An entry with no login is not touched.
            (
                entry(tarball, "sha256:old", sig),
                entry(tarball, "sha256:new", sig),
                ("sha256:new", "sha256:new-sig"),
            ),
        ];
        for (old, mut new, (checksum, artifact_checksum)) in cases {
            keep_pins_recorded_with_a_login(&old, &mut new);
            assert_eq!(new.checksum.as_deref(), Some(checksum), "{old:?}");
            assert_eq!(
                new.additional_artifacts[0].checksum.as_deref(),
                Some(artifact_checksum),
                "{old:?}"
            );
        }
    }

    #[test]
    fn kept_checksums_keep_the_provenance_recorded_with_them() {
        let entry = |url: &str, checksum: &str, provenance: Option<ProvenanceType>| PlatformInfo {
            url: Some(url.into()),
            checksum: Some(checksum.into()),
            provenance: provenance.clone(),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                checksum: Some(format!("{checksum}-sig")),
                url: format!("{url}.sig"),
                provenance,
                ..Default::default()
            }],
            ..Default::default()
        };
        let old_url = "https://ci:token@mirror.example/v20.0.0/node-linux-x64.tar.gz";
        let new_url = "https://mirror.example/v20.0.0/node-linux-x64.tar.gz";
        let fresh = Some(ProvenanceType::GithubAttestations);
        let recorded = Some(ProvenanceType::Minisign);
        // (old provenance, newly resolved checksum, provenance the entry keeps)
        let cases = [
            // Other bytes: the provenance of the new ones does not cover the kept pin.
            (None, "sha256:new", None),
            (recorded.clone(), "sha256:new", recorded.clone()),
            // Another algorithm does not show the bytes are the same.
            (None, "blake3:old", None),
            // The same bytes: the new provenance covers them.
            (None, "sha256:old", fresh.clone()),
            (recorded.clone(), "SHA256:OLD", fresh.clone()),
        ];
        for (old_provenance, new_checksum, kept) in cases {
            let old = entry(old_url, "sha256:old", old_provenance.clone());
            let mut new = entry(new_url, new_checksum, fresh.clone());
            keep_pins_recorded_with_a_login(&old, &mut new);
            let case = format!("{old_provenance:?} {new_checksum}");
            assert_eq!(new.checksum.as_deref(), Some("sha256:old"), "{case}");
            assert_eq!(new.provenance, kept, "{case}");
            let artifact = &new.additional_artifacts[0];
            assert_eq!(
                artifact.checksum.as_deref(),
                Some("sha256:old-sig"),
                "{case}"
            );
            assert_eq!(artifact.provenance, kept, "{case}");
        }
    }

    #[test]
    fn entry_pinned_by_md5_or_sha1_is_not_reusable() {
        let url = "https://example.invalid/tool.tar.gz";
        let entry = |checksum: &str| PlatformInfo {
            url: Some(url.into()),
            checksum: Some(checksum.into()),
            ..Default::default()
        };
        assert!(can_reuse(&entry("sha256:old"), "1.0.0"));
        assert!(can_reuse(&entry("blake3:old"), "1.0.0"));
        assert!(!can_reuse(&entry("md5:old"), "1.0.0"));
        assert!(!can_reuse(&entry("sha1:old"), "1.0.0"));
        let mut info = entry("sha256:old");
        info.additional_artifacts
            .push(crate::lockfile::ArtifactInfo {
                checksum: Some("md5:old".into()),
                url: "https://example.invalid/tool.sig".into(),
                ..Default::default()
            });
        assert!(!can_reuse(&info, "1.0.0"));
    }

    #[tokio::test]
    async fn md5_checksum_is_checked_and_replaced_by_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let artifact = tmp.path().join("probe");
        std::fs::write(&artifact, b"artifact").unwrap();
        let url = url::Url::from_file_path(&artifact).unwrap().to_string();
        let sha256 = format!(
            "sha256:{}",
            crate::hash::file_hash_sha256(&artifact, None).unwrap()
        );
        let with_checksum = |checksum: &str| PlatformInfo {
            url: Some(url.clone()),
            checksum: Some(checksum.into()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: url.clone(),
                checksum: Some(checksum.into()),
                ..Default::default()
            }],
            ..Default::default()
        };

        let mut info = with_checksum("md5:8e5b948a454515dbabfc7eb718daa52f");
        info.checksum_source = Some(crate::lockfile::ChecksumSource::VersionsHost);
        info.additional_artifacts[0].checksum_source =
            Some(crate::lockfile::ChecksumSource::VersionsHost);
        complete_artifact_checksums(&mut info).await.unwrap();
        assert_eq!(info.checksum.as_deref(), Some(sha256.as_str()));
        assert_eq!(
            info.additional_artifacts[0].checksum.as_deref(),
            Some(sha256.as_str())
        );
        // The download confirmed the sha256, so it can pin a kept download.
        assert_eq!(info.checksum_source, None);
        assert_eq!(info.pin_checksum(), Some(&sha256));
        assert_eq!(info.additional_artifacts[0].checksum_source, None);

        // The md5 the release lists still has to match.
        let mut info = with_checksum("md5:00000000000000000000000000000000");
        let err = complete_artifact_checksums(&mut info).await.unwrap_err();
        assert!(
            format!("{err:#}").contains("does not match the checksum its release lists"),
            "{err:#}"
        );

        // A strong checksum is kept as it is.
        let blake3 = format!(
            "blake3:{}",
            crate::hash::file_hash_blake3(&artifact, None).unwrap()
        );
        let mut info = with_checksum(&blake3);
        complete_artifact_checksums(&mut info).await.unwrap();
        assert_eq!(info.checksum.as_deref(), Some(blake3.as_str()));
    }

    #[tokio::test]
    async fn refresh_checks_the_artifact_against_the_weak_pin_it_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let artifact = tmp.path().join("probe");
        std::fs::write(&artifact, b"artifact").unwrap();
        let url = url::Url::from_file_path(&artifact).unwrap().to_string();
        let sha256 = format!(
            "sha256:{}",
            crate::hash::file_hash_sha256(&artifact, None).unwrap()
        );
        let entry = |url: &str, checksum: &str| PlatformInfo {
            url: Some(url.into()),
            checksum: Some(checksum.into()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: url.into(),
                checksum: Some(checksum.into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let refreshed = entry(&url, &sha256);

        // The bytes the URL serves are the ones mise.lock pinned.
        let old = entry(&url, "md5:8e5b948a454515dbabfc7eb718daa52f");
        check_replaced_weak_pins(&old, &refreshed, &[])
            .await
            .unwrap();

        // Other bytes at the same URL stop the refresh, for the primary
        // artifact and for an additional one.
        let other = "md5:00000000000000000000000000000000";
        let err = check_replaced_weak_pins(&entry(&url, other), &refreshed, &[])
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("does not match the checksum mise.lock pinned it by"),
            "{err:#}"
        );
        let mut old = entry(&url, &sha256);
        old.additional_artifacts[0].checksum = Some(other.into());
        assert!(
            check_replaced_weak_pins(&old, &refreshed, &[])
                .await
                .is_err()
        );

        // A pin for another URL, or a strong one, says nothing about these bytes.
        let elsewhere = entry("file:///nonexistent/other", other);
        check_replaced_weak_pins(&elsewhere, &refreshed, &[])
            .await
            .unwrap();
        let blake3 = format!("blake3:{}", "0".repeat(64));
        check_replaced_weak_pins(&entry(&url, &blake3), &refreshed, &[])
            .await
            .unwrap();

        // A pin checked through the login covers only that artifact and that
        // pin: the additional artifact at the same URL, pinned to other
        // bytes, is still checked.
        let good = "md5:8e5b948a454515dbabfc7eb718daa52f";
        let mut old = entry(&url, good);
        old.additional_artifacts[0].checksum = Some(other.into());
        let checked = |artifact, pin: &str| CheckedPin {
            artifact,
            pin: pin.into(),
        };
        assert!(
            check_replaced_weak_pins(&old, &refreshed, &[checked(None, good)])
                .await
                .is_err()
        );
        assert!(
            check_replaced_weak_pins(&old, &refreshed, &[checked(Some(0), good)])
                .await
                .is_err()
        );
        check_replaced_weak_pins(
            &old,
            &refreshed,
            &[checked(None, good), checked(Some(0), other)],
        )
        .await
        .unwrap();
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
        assert!(!is_current(&old, &tools, &platforms[..1], true).unwrap());
        let mut extra_platform = platforms.clone();
        extra_platform.push(Platform::parse("linux-arm64").unwrap());
        assert!(!is_current(&old, &tools, &extra_platform, true).unwrap());
        assert!(!is_current(&old, &[], &platforms, true).unwrap());
        assert!(!is_current(&old, &tools, &[], true).unwrap());
        assert!(!is_current(&old, &[tool(), tool()], &platforms, true).unwrap());

        let mut changed = old.clone();
        changed
            .tools
            .insert("obsolete".into(), old.tools["fixture"].clone());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed
            .tools_for_mut("fixture")
            .unwrap()
            .push(old.tools["fixture"][0].clone());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0].version = "other-tag".into();
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0].backend = Some("http:other".into());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0]
            .options
            .insert("format".into(), "tar.gz".into());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0]
            .specifiers
            .clear();
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0]
            .specifiers
            .insert("obsolete".into());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed
            .conda_packages
            .insert("linux-x64".into(), BTreeMap::new());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
    }

    #[tokio::test]
    async fn platform_recorded_as_unsupported_is_current() {
        crate::backend::load_tools().await.unwrap();
        let tools = vec![tool()];
        let mut platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let mut old = previous();
        old.tools_for_mut("fixture").unwrap()[0]
            .unsupported_platforms
            .insert("linux-arm64".into());
        // A listed platform outside the requested scope must still be pruned.
        assert!(!is_current(&old, &tools, &platforms, true).unwrap());
        platforms.push(Platform::parse("linux-arm64").unwrap());
        assert!(is_current(&old, &tools, &platforms, true).unwrap());
        // A platform that is neither locked nor listed is not current.
        platforms.push(Platform::parse("windows-x64").unwrap());
        assert!(!is_current(&old, &tools, &platforms, true).unwrap());
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
        changed.tools_for_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .checksum = None;
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .url = None;
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .signer = Some("signer".into());
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());
        let mut changed = old.clone();
        changed.tools_for_mut("fixture").unwrap()[0]
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .additional_artifacts
            .push(ArtifactInfo {
                url: "https://example.invalid/extra".into(),
                ..Default::default()
            });
        assert!(!is_current(&changed, &tools, &platforms, true).unwrap());

        let ba = BackendArg::new("fixture".into(), Some("github:example/fixture".into()));
        let request = ToolRequest::new(Arc::new(ba.clone()), "1", ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, "1.0".into());
        let entry = &mut changed.tools_for_mut("fixture").unwrap()[0];
        entry.backend = Some(ba.stored_full());
        let artifact = &mut entry
            .platforms
            .get_mut("linux-x64")
            .unwrap()
            .additional_artifacts[0];
        artifact.checksum = Some("sha256:unchanged".into());
        artifact.provenance = Some(ProvenanceType::Minisign);
        let error = is_current(&changed, &[(ba, tv)], &platforms, true).unwrap_err();
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
            PlatformSelection::Explicit(BTreeSet::new()),
            2,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(old.tools, generated.tools);
    }

    #[tokio::test]
    async fn explicit_generation_drops_rows_for_os_excluded_platforms() {
        crate::backend::load_tools().await.unwrap();
        let ba = BackendArg::new("fixture".into(), Some("http:fixture".into()));
        let mut options = ToolVersionOptions::default();
        options.core.os = Some(vec!["linux".into()]);
        let request =
            ToolRequest::new_with_options(Arc::new(ba.clone()), "1", options, ToolSource::Argument)
                .unwrap();
        let tools = vec![(ba, ToolVersion::new(request, "1.0".into()))];
        let platforms = [
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];
        let generated = generate(
            &previous(),
            &tools,
            &platforms,
            false,
            PlatformSelection::Explicit(BTreeSet::new()),
            2,
            &[],
        )
        .await
        .unwrap();
        let platforms = &generated.tools_for("fixture").unwrap()[0].platforms;
        assert!(platforms.contains_key("linux-x64"));
        assert!(!platforms.contains_key("macos-arm64"));
    }

    /// A tool configured with URL credentials is selected under its
    /// credential-free lockfile key, so neither a partial run nor explicit
    /// platforms keep its old entry as untouched next to the new one.
    #[tokio::test]
    async fn a_credentialed_tool_is_selected_by_its_lockfile_key() {
        crate::backend::load_tools().await.unwrap();
        let raw = "http:https://ci:t0ken@example.invalid/fixture";
        let clean = "http:https://example.invalid/fixture";
        let ba = BackendArg::new(raw.into(), Some(raw.into()));
        let request = ToolRequest::new_with_options(
            Arc::new(ba.clone()),
            "1",
            ToolVersionOptions::default(),
            ToolSource::Argument,
        )
        .unwrap();
        let tool = (ba, ToolVersion::new(request, "1.0".into()));
        let mut old = Lockfile::default();
        for platform in ["linux-x64", "macos-arm64"] {
            old.set_platform_info(
                clean,
                "1.0",
                Some(clean),
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
        let linux = [Platform::parse("linux-x64").unwrap()];
        for (retain, selection) in [
            (true, PlatformSelection::Default),
            (false, PlatformSelection::Explicit(BTreeSet::new())),
        ] {
            let generated = generate(
                &old,
                std::slice::from_ref(&tool),
                &linux,
                retain,
                selection,
                1,
                &[],
            )
            .await
            .unwrap();
            assert_eq!(generated.tools.keys().collect::<Vec<_>>(), [clean]);
            assert_eq!(generated.tools[clean].len(), 1, "{:?}", generated.tools);
            // A selected tool is rebuilt for the targeted platforms; only an
            // explicit platform selection carries the others forward.
            let platforms = generated.tools[clean][0]
                .platforms
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let expected: &[&str] = if retain {
                &["linux-x64"]
            } else {
                &["linux-x64", "macos-arm64"]
            };
            assert_eq!(platforms, expected, "retain={retain}");
        }
    }

    #[tokio::test]
    async fn fresh_generation_drops_obsolete_tools_but_filter_keeps_them() {
        crate::backend::load_tools().await.unwrap();
        let old = previous();
        let empty = generate(&old, &[], &[], false, PlatformSelection::Default, 1, &[])
            .await
            .unwrap();
        assert!(empty.tools.is_empty());
        let retained = generate(&old, &[], &[], true, PlatformSelection::Default, 1, &[])
            .await
            .unwrap();
        assert_eq!(old.tools, retained.tools);
    }

    #[tokio::test]
    async fn retaining_unselected_tools_carries_graphs_but_refreshes_selected_tools() {
        crate::backend::load_tools().await.unwrap();
        let mut old = previous();
        let mut retained = old.tools["fixture"][0].clone();
        retained.aube = Some(super::super::GraphRef::Sidecar {
            dir: PathBuf::from(".mise/locks/npm-retained/1.0.0"),
            base: PathBuf::new(),
            digest: "sha256:recorded".into(),
            cell: std::sync::OnceLock::new(),
        });
        old.tools
            .insert("npm:retained".into(), vec![retained.clone()]);
        let current = old.clone();
        let mut stale_option = old.tools["fixture"][0].clone();
        stale_option
            .options
            .insert("obsolete".into(), "true".into());
        old.tools_for_mut("fixture").unwrap().push(stale_option);
        let platforms = vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
        ];

        assert!(!is_current(&current, &[tool()], &platforms, true).unwrap());
        assert!(is_current(&current, &[tool()], &platforms, false).unwrap());

        let generated = generate(
            &old,
            &[tool()],
            &platforms,
            true,
            PlatformSelection::Default,
            2,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(generated.tools["npm:retained"], vec![retained]);
        assert_eq!(generated.tools["fixture"].len(), 1);
        assert!(generated.tools["fixture"][0].options.is_empty());
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
    async fn conda_pins_recorded_with_a_login_are_kept() {
        use crate::backend::conda::CondaPackageInfo;
        let package = |url: &str, checksum: &str| CondaPackageInfo {
            url: url.into(),
            checksum: Some(checksum.into()),
        };
        let url = "https://conda.example/linux-64/zlib-1.3-h0_0.conda";
        let with_login = "https://ci:token@conda.example/linux-64/zlib-1.3-h0_0.conda";
        let other = "https://conda.example/linux-64/zlib-1.3-h1_0.conda";
        // (old, resolved checksum at `new_url`, checksum written)
        for (old, new_url, written) in [
            (package(with_login, "sha256:old"), url, "sha256:old"),
            (package(with_login, "sha256:old"), other, "sha256:new"),
            // A record with no login is resolved as before.
            (package(url, "sha256:old"), url, "sha256:new"),
            (package(url, "md5:old"), url, "sha256:new"),
        ] {
            let mut new = package(new_url, "sha256:new");
            keep_conda_pin_recorded_with_a_login(&old, &mut new)
                .await
                .unwrap();
            assert_eq!(new.checksum.as_deref(), Some(written), "{old:?} {new_url}");
            assert_eq!(new.url, new_url);
        }
    }

    /// A conda package pinned by md5 with the channel's login is not pinned by
    /// md5 again: it is downloaded with the login, checked against the md5, and
    /// written with its sha256 and the URL without the login.
    #[tokio::test]
    async fn conda_md5_pin_recorded_with_a_login_is_replaced_from_the_login_url() {
        use crate::backend::conda::CondaPackageInfo;
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/linux-64/zlib.conda")
            .match_header("authorization", "Basic Y2k6dG9rZW4=")
            .with_body("artifact")
            .create_async()
            .await;
        let url = format!("{}/linux-64/zlib.conda", server.url());
        let old = CondaPackageInfo {
            url: url.replace("http://", "http://ci:token@"),
            checksum: Some(format!("md5:{ARTIFACT_MD5}")),
        };
        let mut new = CondaPackageInfo {
            url: url.clone(),
            checksum: None,
        };
        keep_conda_pin_recorded_with_a_login(&old, &mut new)
            .await
            .unwrap();
        mock.assert_async().await;
        assert_eq!(new.checksum, Some(format!("sha256:{ARTIFACT_SHA256}")));
        assert_eq!(new.url, url);

        let mut wrong = CondaPackageInfo {
            url: url.clone(),
            checksum: None,
        };
        let tampered = CondaPackageInfo {
            checksum: Some("md5:00000000000000000000000000000000".into()),
            ..old
        };
        assert!(
            keep_conda_pin_recorded_with_a_login(&tampered, &mut wrong)
                .await
                .is_err()
        );
    }

    const ARTIFACT_MD5: &str = "8e5b948a454515dbabfc7eb718daa52f";
    const ARTIFACT_SHA256: &str =
        "c7c5c1d70c5dec4416ab6158afd0b223ef40c29b1dc1f97ed9428b94d4cadb1c";

    /// An entry pinned by md5 with a login does not keep the md5 pin (which
    /// [`can_reuse`] refused so it would be replaced). The artifacts are
    /// downloaded from the URLs with the login, checked against the md5, and
    /// pinned by sha256 under the URLs without the login.
    #[tokio::test]
    async fn md5_pins_recorded_with_a_login_are_replaced_from_the_login_url() {
        let mut server = mockito::Server::new_async().await;
        let mocks = [
            server
                .mock("GET", "/tool.tar.gz")
                .match_header("authorization", "Basic Y2k6dG9rZW4=")
                .with_body("artifact")
                .create_async()
                .await,
            server
                .mock("GET", "/tool.tar.gz.sig")
                .match_header("authorization", "Basic Y2k6dG9rZW4=")
                .with_body("artifact")
                .create_async()
                .await,
        ];
        let tarball = format!("{}/tool.tar.gz", server.url());
        let sig = format!("{tarball}.sig");
        let with_login = |url: &str| url.replace("http://", "http://ci:token@");
        let md5 = format!("md5:{ARTIFACT_MD5}");
        let entry = |url: &str, sig: &str, checksum: Option<&str>| PlatformInfo {
            url: Some(url.into()),
            checksum: checksum.map(str::to_owned),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: sig.into(),
                checksum: checksum.map(str::to_owned),
                ..Default::default()
            }],
            ..Default::default()
        };
        let old = entry(&with_login(&tarball), &with_login(&sig), Some(&md5));

        // The weak pin is not carried.
        let mut new = entry(&tarball, &sig, None);
        keep_pins_recorded_with_a_login(&old, &mut new);
        assert_eq!(new.checksum, None);
        assert_eq!(new.additional_artifacts[0].checksum, None);

        upgrade_weak_pins_recorded_with_a_login(&old, &mut new)
            .await
            .unwrap();
        keep_pins_recorded_with_a_login(&old, &mut new);
        complete_artifact_checksums(&mut new).await.unwrap();
        for mock in mocks {
            mock.assert_async().await;
        }
        let sha256 = format!("sha256:{ARTIFACT_SHA256}");
        assert_eq!(new.checksum.as_deref(), Some(sha256.as_str()));
        assert_eq!(
            new.additional_artifacts[0].checksum.as_deref(),
            Some(sha256.as_str())
        );
        assert_eq!(new.url.as_deref(), Some(tarball.as_str()));
        assert_eq!(new.additional_artifacts[0].url, sig);

        // Bytes that do not match the old pin are refused.
        let tampered = entry(
            &with_login(&tarball),
            &with_login(&sig),
            Some("md5:00000000000000000000000000000000"),
        );
        let mut new = entry(&tarball, &sig, None);
        assert!(
            upgrade_weak_pins_recorded_with_a_login(&tampered, &mut new)
                .await
                .is_err()
        );
    }

    /// A private forge asset whose login is only in `url_api` is checked
    /// against its old pin once, through the login. The check of replaced weak
    /// pins that follows would download the API URL without the login, which
    /// the forge refuses.
    #[tokio::test]
    async fn a_weak_pin_checked_through_the_api_login_is_not_checked_again() {
        let mut server = mockito::Server::new_async().await;
        // Unique paths: downloads are cached per invocation, and mockito
        // reuses ports across tests.
        let browser = server
            .mock("HEAD", "/api-login/browser.tar.gz")
            .with_status(404)
            .expect_at_least(1)
            .create_async()
            .await;
        let with_login = server
            .mock("GET", "/api-login/asset.tar.gz")
            .match_header("authorization", "Basic Y2k6dG9rZW4=")
            .with_body("artifact")
            .expect(1)
            .create_async()
            .await;
        let without_login = server
            .mock("GET", "/api-login/asset.tar.gz")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(401)
            .expect(0)
            .create_async()
            .await;
        let url = format!("{}/api-login/browser.tar.gz", server.url());
        let api = format!("{}/api-login/asset.tar.gz", server.url());
        let old = PlatformInfo {
            url: Some(url.clone()),
            url_api: Some(api.replace("http://", "http://ci:token@")),
            checksum: Some(format!("md5:{ARTIFACT_MD5}")),
            ..Default::default()
        };
        let mut new = PlatformInfo {
            url: Some(url.clone()),
            url_api: Some(api.clone()),
            ..Default::default()
        };

        let verified = upgrade_weak_pins_recorded_with_a_login(&old, &mut new)
            .await
            .unwrap();
        assert_eq!(
            verified,
            vec![CheckedPin {
                artifact: None,
                pin: format!("md5:{ARTIFACT_MD5}"),
            }]
        );
        let sha256 = format!("sha256:{ARTIFACT_SHA256}");
        assert_eq!(new.checksum.as_deref(), Some(sha256.as_str()));
        complete_artifact_checksums(&mut new).await.unwrap();
        check_replaced_weak_pins(&old, &new, &verified)
            .await
            .unwrap();
        assert_eq!(new.url_api.as_deref(), Some(api.as_str()));
        browser.assert_async().await;
        with_login.assert_async().await;
        without_login.assert_async().await;
    }

    /// An artifact whose resolved checksum is an md5 is hashed through the URL
    /// with the login, before the lockfile drops it: the md5 is checked and
    /// replaced by the sha256, which `mise lock` then has no need to download.
    #[tokio::test]
    async fn weak_checksum_is_replaced_from_the_login_url() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/weak-checksum-login.tar.gz")
            .match_header("authorization", "Basic Y2k6dG9rZW4=")
            .with_body("artifact")
            .create_async()
            .await;
        // A path no other test serves: the invocation download cache is keyed by
        // URL, and mockito hands the same ports to later tests.
        let url = format!("{}/weak-checksum-login.tar.gz", server.url())
            .replace("http://", "http://ci:token@");
        let mut checksum = Some(format!("md5:{ARTIFACT_MD5}"));
        download_for_strong_checksum(&url, None, &mut checksum)
            .await
            .unwrap();
        assert_eq!(checksum, Some(format!("sha256:{ARTIFACT_SHA256}")));

        let mut checksum = Some("md5:00000000000000000000000000000000".to_string());
        assert!(
            download_for_strong_checksum(&url, None, &mut checksum)
                .await
                .is_err()
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn weak_checksum_download_sends_the_configured_headers() {
        let mut server = mockito::Server::new_async().await;
        let with_headers = server
            .mock("GET", "/weak-checksum-headers.tar.gz")
            .match_header("authorization", "Basic Y2k6dG9rZW4=")
            .match_header("x-mirror-token", "s3cret")
            .with_body("artifact")
            .expect(1)
            .create_async()
            .await;
        let url = format!("{}/weak-checksum-headers.tar.gz", server.url())
            .replace("http://", "http://ci:token@");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-mirror-token", "s3cret".parse().unwrap());
        let mut checksum = Some(format!("md5:{ARTIFACT_MD5}"));
        download_for_strong_checksum(&url, Some(&DownloadHeaders::Client(headers)), &mut checksum)
            .await
            .unwrap();
        assert_eq!(checksum, Some(format!("sha256:{ARTIFACT_SHA256}")));
        with_headers.assert_async().await;
    }

    /// An http backend's headers are sent as its install sends them, through
    /// mise's HTTP client: a redirect to another origin gets the headers that
    /// `headers_forward` lets through, and not the login of the configured URL.
    #[tokio::test]
    async fn configured_headers_follow_a_redirect_as_the_install_sends_them() {
        let mut origin = mockito::Server::new_async().await;
        let mut storage = mockito::Server::new_async().await;
        // Paths no other test serves: downloads are cached per invocation.
        let redirect = origin
            .mock("GET", "/forwarded-headers.tar.gz")
            .match_header("authorization", "Basic Y2k6dG9rZW4=")
            .match_header("x-mirror-token", "s3cret")
            .with_status(302)
            .with_header(
                "location",
                &format!("{}/forwarded-headers-storage.tar.gz", storage.url()),
            )
            .expect(1)
            .create_async()
            .await;
        let stored = storage
            .mock("GET", "/forwarded-headers-storage.tar.gz")
            .match_header("authorization", mockito::Matcher::Missing)
            .match_header("x-mirror-token", "s3cret")
            .with_body("artifact")
            .expect(1)
            .create_async()
            .await;
        let url = format!("{}/forwarded-headers.tar.gz", origin.url());
        let name = reqwest::header::HeaderName::from_static("x-mirror-token");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(name.clone(), "s3cret".parse().unwrap());
        let host = reqwest::Url::parse(&storage.url())
            .unwrap()
            .host_str()
            .unwrap()
            .to_string();
        let (rule, hosts) = mise_util::http::forward_rule_header(&name, &[host]).unwrap();
        headers.insert(rule, hosts);
        let mut info = PlatformInfo {
            url: Some(url.clone()),
            download_url: crate::lockfile::LoginUrl::of(
                &url.replace("http://", "http://ci:token@"),
            ),
            download_headers: Some(DownloadHeaders::Client(headers)),
            ..Default::default()
        };
        complete_artifact_checksums(&mut info).await.unwrap();
        assert_eq!(info.checksum, Some(format!("sha256:{ARTIFACT_SHA256}")));
        redirect.assert_async().await;
        stored.assert_async().await;
    }

    #[tokio::test]
    async fn a_replaced_weak_pin_is_checked_through_the_login_of_the_configured_url() {
        let mut server = mockito::Server::new_async().await;
        let with_login = server
            .mock("GET", "/replaced-pin-login.tar.gz")
            .match_header("authorization", "Basic Y2k6dG9rZW4=")
            .with_body("artifact")
            .expect_at_least(1)
            .create_async()
            .await;
        let without_login = server
            .mock("GET", "/replaced-pin-login.tar.gz")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(401)
            .expect(0)
            .create_async()
            .await;
        // A path no other test serves: downloads are cached per invocation.
        let url = format!("{}/replaced-pin-login.tar.gz", server.url());
        let login_url = url.replace("http://", "http://ci:token@");
        let entry = |checksum: &str| PlatformInfo {
            url: Some(url.clone()),
            checksum: Some(checksum.into()),
            ..Default::default()
        };
        let refreshed = PlatformInfo {
            download_url: crate::lockfile::LoginUrl::of(&login_url),
            ..entry(&format!("sha256:{ARTIFACT_SHA256}"))
        };
        assert!(!format!("{refreshed:?}").contains("token"), "{refreshed:?}");

        check_replaced_weak_pins(&entry(&format!("md5:{ARTIFACT_MD5}")), &refreshed, &[])
            .await
            .unwrap();
        let err = check_replaced_weak_pins(
            &entry("md5:00000000000000000000000000000000"),
            &refreshed,
            &[],
        )
        .await
        .unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("does not match the checksum mise.lock pinned it by"),
            "{err}"
        );
        assert!(!err.contains("token"), "{err}");
        with_login.assert_async().await;
        without_login.assert_async().await;
    }

    /// A forge entry recorded with no login and an md5 pin, refreshed through
    /// an API URL that holds the configured login: the old pin is checked
    /// through that login, for the main artifact and an additional one, since
    /// the recorded API URL leaves it out and a private API refuses it.
    #[tokio::test]
    async fn a_replaced_weak_pin_is_checked_through_the_login_of_the_configured_api_url() {
        let mut server = mockito::Server::new_async().await;
        let mut mocks = Vec::new();
        for name in ["api-login-main", "api-login-extra"] {
            // The browser URL answers with a sign-in page, so the API URL is used.
            mocks.push(
                server
                    .mock("HEAD", format!("/download/{name}.tar.gz").as_str())
                    .with_header("content-type", "text/html")
                    .create_async()
                    .await,
            );
            mocks.push(
                server
                    .mock("GET", format!("/api/assets/{name}").as_str())
                    .match_header("authorization", "Basic Y2k6dG9rZW4=")
                    .with_body("artifact")
                    .expect_at_least(1)
                    .create_async()
                    .await,
            );
            mocks.push(
                server
                    .mock("GET", format!("/api/assets/{name}").as_str())
                    .match_header("authorization", mockito::Matcher::Missing)
                    .with_status(401)
                    .expect(0)
                    .create_async()
                    .await,
            );
        }
        let urls = |name: &str| {
            (
                format!("{}/download/{name}.tar.gz", server.url()),
                format!("{}/api/assets/{name}", server.url()),
            )
        };
        let login =
            |url: &str| crate::lockfile::LoginUrl::of(&url.replace("http://", "http://ci:token@"));
        let (main_url, main_api) = urls("api-login-main");
        let (extra_url, extra_api) = urls("api-login-extra");
        let md5 = format!("md5:{ARTIFACT_MD5}");
        let sha256 = format!("sha256:{ARTIFACT_SHA256}");
        let old = PlatformInfo {
            url: Some(main_url.clone()),
            url_api: Some(main_api.clone()),
            checksum: Some(md5.clone()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: extra_url.clone(),
                url_api: Some(extra_api.clone()),
                checksum: Some(md5.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let refreshed = PlatformInfo {
            checksum: Some(sha256.clone()),
            download_url_api: login(&main_api),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: extra_url.clone(),
                url_api: Some(extra_api.clone()),
                checksum: Some(sha256.clone()),
                download_url_api: login(&extra_api),
                ..Default::default()
            }],
            ..old.clone()
        };
        assert!(!format!("{refreshed:?}").contains("token"), "{refreshed:?}");
        let written = toml::to_string(&refreshed).unwrap();
        assert!(!written.contains("token"), "{written}");

        check_replaced_weak_pins(&old, &refreshed, &[])
            .await
            .unwrap();
        for mock in mocks.iter().skip(1).step_by(3) {
            mock.assert_async().await;
        }
        for mock in mocks.iter().skip(2).step_by(3) {
            mock.assert_async().await;
        }
    }

    /// A forge on a custom API installs from the API URL, so its entry names
    /// that URL as both download URLs. The replaced weak pin is then checked
    /// against the API attachment, which the install and `lock_urls_of` hash,
    /// even when the browser URL serves other bytes with no sign-in page.
    #[tokio::test]
    async fn a_replaced_weak_pin_on_a_custom_api_is_checked_against_the_api_attachment() {
        let mut server = mockito::Server::new_async().await;
        let mut browser = Vec::new();
        let mut api = Vec::new();
        for name in ["custom-main", "custom-extra"] {
            let path = format!("/download/{name}.tar.gz");
            browser.push(
                server
                    .mock("HEAD", path.as_str())
                    .with_header("content-type", "application/octet-stream")
                    .expect(0)
                    .create_async()
                    .await,
            );
            browser.push(
                server
                    .mock("GET", path.as_str())
                    .with_body("other bytes")
                    .expect(0)
                    .create_async()
                    .await,
            );
            api.push(
                server
                    .mock("GET", format!("/api/v1/assets/{name}").as_str())
                    .match_header("authorization", "token secret")
                    .with_body("artifact")
                    .create_async()
                    .await,
            );
        }
        let urls = |name: &str| {
            (
                format!("{}/download/{name}.tar.gz", server.url()),
                format!("{}/api/v1/assets/{name}", server.url()),
            )
        };
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("authorization", "token secret".parse().unwrap());
        let headers = Some(DownloadHeaders::Client(headers));
        let install_url = |url: &str| Some(crate::lockfile::LoginUrl::new(url));
        let (main_url, main_api) = urls("custom-main");
        let (extra_url, extra_api) = urls("custom-extra");
        let md5 = format!("md5:{ARTIFACT_MD5}");
        let sha256 = format!("sha256:{ARTIFACT_SHA256}");
        let old = PlatformInfo {
            url: Some(main_url.clone()),
            url_api: Some(main_api.clone()),
            checksum: Some(md5.clone()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: extra_url.clone(),
                url_api: Some(extra_api.clone()),
                checksum: Some(md5.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let refreshed = PlatformInfo {
            checksum: Some(sha256.clone()),
            download_url: install_url(&main_api),
            download_url_api: install_url(&main_api),
            download_headers: headers.clone(),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: extra_url,
                url_api: Some(extra_api.clone()),
                checksum: Some(sha256),
                download_url: install_url(&extra_api),
                download_url_api: install_url(&extra_api),
                download_headers: headers,
                ..Default::default()
            }],
            ..old.clone()
        };

        check_replaced_weak_pins(&old, &refreshed, &[])
            .await
            .unwrap();
        for mock in browser.iter().chain(&api) {
            mock.assert_async().await;
        }
    }

    /// An old entry of a forge on a custom API, recorded with a login, has its
    /// md5 pins checked against the API attachment the install downloads, not
    /// the browser URL, with the login and headers the install sends there.
    /// The logins the old entry recorded are not sent.
    #[tokio::test]
    async fn a_weak_pin_recorded_with_a_login_on_a_custom_api_is_checked_against_the_api() {
        let mut server = mockito::Server::new_async().await;
        let mut mocks = Vec::new();
        for name in ["login-main", "login-extra"] {
            let path = format!("/download/{name}.tar.gz");
            mocks.push(
                server
                    .mock("HEAD", path.as_str())
                    .with_header("content-type", "application/octet-stream")
                    .expect(0)
                    .create_async()
                    .await,
            );
            mocks.push(
                server
                    .mock("GET", path.as_str())
                    .with_body("other bytes")
                    .expect(0)
                    .create_async()
                    .await,
            );
        }
        // "ci:old", the login the old entry recorded.
        mocks.push(
            server
                .mock("GET", mockito::Matcher::Regex("^/api/v1/assets/".into()))
                .match_header("authorization", "Basic Y2k6b2xk")
                .with_status(401)
                .expect(0)
                .create_async()
                .await,
        );
        // The main artifact's API URL holds the configured login, "ci:token".
        mocks.push(
            server
                .mock("GET", "/api/v1/assets/login-main")
                .match_header("authorization", "Basic Y2k6dG9rZW4=")
                .with_body("artifact")
                .expect(1)
                .create_async()
                .await,
        );
        // The extra one is downloaded with the configured token header.
        mocks.push(
            server
                .mock("GET", "/api/v1/assets/login-extra")
                .match_header("authorization", "token secret")
                .with_body("artifact")
                .expect(1)
                .create_async()
                .await,
        );
        let urls = |name: &str| {
            (
                format!("{}/download/{name}.tar.gz", server.url()),
                format!("{}/api/v1/assets/{name}", server.url()),
            )
        };
        let old_login = |url: &str| url.replace("http://", "http://ci:old@");
        let install_url = |url: &str| Some(crate::lockfile::LoginUrl::new(url));
        let (main_url, main_api) = urls("login-main");
        let (extra_url, extra_api) = urls("login-extra");
        let main_install = main_api.replace("http://", "http://ci:token@");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("authorization", "token secret".parse().unwrap());
        let md5 = format!("md5:{ARTIFACT_MD5}");
        // The main artifact's old login is in its API URL, the extra one's in
        // its browser URL.
        let old = PlatformInfo {
            url: Some(main_url.clone()),
            url_api: Some(old_login(&main_api)),
            checksum: Some(md5.clone()),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: old_login(&extra_url),
                url_api: Some(extra_api.clone()),
                checksum: Some(md5.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut new = PlatformInfo {
            url: Some(main_url),
            url_api: Some(main_api.clone()),
            download_url: install_url(&main_install),
            download_url_api: install_url(&main_install),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: extra_url,
                url_api: Some(extra_api.clone()),
                download_url: install_url(&extra_api),
                download_url_api: install_url(&extra_api),
                download_headers: Some(DownloadHeaders::Client(headers)),
                ..Default::default()
            }],
            ..Default::default()
        };

        let verified = upgrade_weak_pins_recorded_with_a_login(&old, &mut new)
            .await
            .unwrap();
        assert_eq!(verified.len(), 2);
        let sha256 = format!("sha256:{ARTIFACT_SHA256}");
        assert_eq!(new.checksum.as_deref(), Some(sha256.as_str()));
        assert_eq!(
            new.additional_artifacts[0].checksum.as_deref(),
            Some(sha256.as_str())
        );
        for mock in &mocks {
            mock.assert_async().await;
        }
    }

    /// An artifact the install downloads from its API URL alone is checked
    /// there, with the install's headers. Anything else is checked at the URLs
    /// the old entry recorded, and the headers reach only the server they were
    /// resolved for.
    #[test]
    fn old_pins_are_checked_where_the_install_downloads() {
        let install =
            crate::lockfile::LoginUrl::new("https://ci:token@forge.example/api/v1/assets/1");
        let browser = crate::lockfile::LoginUrl::new("https://forge.example/download/tool.tar.gz");
        assert_eq!(
            api_only_download_url(Some(&install), Some(&install)),
            Some(install.as_str())
        );
        assert_eq!(api_only_download_url(Some(&browser), Some(&install)), None);
        assert_eq!(api_only_download_url(None, Some(&install)), None);

        let headers = Some(DownloadHeaders::Client(reqwest::header::HeaderMap::new()));
        let old_url = "https://ci:old@forge.example/download/tool.tar.gz";
        let old_api = Some("https://forge.example/api/v1/assets/1");
        let api = install.as_str();
        assert_eq!(
            old_pin_download(
                (old_url, old_api),
                Some(api),
                (None, None),
                headers.as_ref()
            ),
            (api.to_owned(), Some(api.to_owned()), headers.as_ref())
        );
        let same_server = (Some(browser.as_str()), old_api);
        assert_eq!(
            old_pin_download((old_url, old_api), None, same_server, headers.as_ref()),
            (
                old_url.to_owned(),
                old_api.map(str::to_owned),
                headers.as_ref()
            )
        );
        let moved = (None, Some("https://api.forge.example/assets/1"));
        assert_eq!(
            old_pin_download((old_url, old_api), None, moved, headers.as_ref()),
            (old_url.to_owned(), old_api.map(str::to_owned), None)
        );
    }

    /// An additional artifact sends the headers its backend resolved it with,
    /// as the main one does, both when its md5 pin recorded with a login is
    /// replaced and when an artifact with no checksum is hashed.
    #[tokio::test]
    async fn additional_artifacts_send_their_download_headers() {
        let mut server = mockito::Server::new_async().await;
        let upgraded = server
            .mock("GET", "/tool.tar.gz.sig")
            .match_header("private-token", "forge-token")
            .with_body("artifact")
            .create_async()
            .await;
        let completed = server
            .mock("GET", "/tool.tar.gz.pem")
            .match_header("private-token", "forge-token")
            .with_body("artifact")
            .create_async()
            .await;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("private-token", "forge-token".parse().unwrap());
        let sig = format!("{}/tool.tar.gz.sig", server.url());
        let pem = format!("{}/tool.tar.gz.pem", server.url());
        let old = PlatformInfo {
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: sig.replace("http://", "http://ci:token@"),
                checksum: Some(format!("md5:{ARTIFACT_MD5}")),
                ..Default::default()
            }],
            ..Default::default()
        };
        let artifact = |url: &str| crate::lockfile::ArtifactInfo {
            url: url.into(),
            download_headers: Some(DownloadHeaders::Client(headers.clone())),
            ..Default::default()
        };
        let mut new = PlatformInfo {
            additional_artifacts: vec![artifact(&sig), artifact(&pem)],
            ..Default::default()
        };
        upgrade_weak_pins_recorded_with_a_login(&old, &mut new)
            .await
            .unwrap();
        complete_artifact_checksums(&mut new).await.unwrap();
        upgraded.assert_async().await;
        completed.assert_async().await;
        let sha256 = format!("sha256:{ARTIFACT_SHA256}");
        for artifact in &new.additional_artifacts {
            assert_eq!(artifact.checksum.as_deref(), Some(sha256.as_str()));
        }
        // The headers are never persisted.
        let written = toml::to_string(&new).unwrap();
        assert!(!written.contains("forge-token"), "{written}");
    }

    #[test]
    fn legacy_bits_follow_an_artifact_whose_login_was_left_out() {
        let entry = |url: &str| PlatformInfo {
            url: Some(url.into()),
            checksum: Some("sha256:old".into()),
            provenance_verified: Some(true),
            additional_artifacts: vec![crate::lockfile::ArtifactInfo {
                url: format!("{url}.sig"),
                checksum: Some("sha256:old-sig".into()),
                provenance_verified: Some(true),
                ..Default::default()
            }],
            ..Default::default()
        };
        let old = entry("https://ci:token@mirror.example/v20.0.0/node-linux-x64.tar.gz");
        let mut new = PlatformInfo {
            provenance_verified: None,
            ..entry("https://mirror.example/v20.0.0/node-linux-x64.tar.gz")
        };
        new.additional_artifacts[0].provenance_verified = None;
        keep_pins_recorded_with_a_login(&old, &mut new);
        preserve_legacy_metadata(&old, &mut new);
        assert_eq!(new.provenance_verified, Some(true));
        assert_eq!(new.additional_artifacts[0].provenance_verified, Some(true));
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
                .lockfile_options(&tv.request, &PlatformTarget::new(platform.clone()))
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
        assert!(is_current(&old, &[(ba.clone(), tv.clone())], &platforms, true).unwrap());
        let generated = generate(
            &old,
            &[(ba, tv)],
            &platforms,
            false,
            PlatformSelection::Default,
            2,
            &[],
        )
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
        assert!(ensure_no_downgrade(&old, &PlatformInfo::default(), "github:o/r").is_err());
        let mut new = old.clone();
        new.provenance_verified = None;
        assert!(ensure_no_downgrade(&old, &new, "github:o/r").is_ok());
    }

    #[test]
    fn aqua_slsa_downgrade_points_at_the_signer_options() {
        let old = PlatformInfo {
            provenance: Some(ProvenanceType::Slsa { url: None }),
            ..Default::default()
        };
        let new = PlatformInfo::default();
        let aqua = ensure_no_downgrade(&old, &new, "aqua:o/r")
            .unwrap_err()
            .to_string();
        assert!(aqua.contains("slsa_signer_identity"), "{aqua}");
        let github = ensure_no_downgrade(&old, &new, "github:o/r")
            .unwrap_err()
            .to_string();
        assert!(!github.contains("slsa_signer_identity"), "{github}");
    }

    #[test]
    fn packslip_replaces_legacy_provenance_with_its_signer_chain() {
        let old = PlatformInfo {
            provenance: Some(ProvenanceType::GithubAttestations),
            signer: Some(
                "sigstore-oidc:https://github.com/o/r/.github/workflows/release.yml".into(),
            ),
            ..Default::default()
        };
        let mut new = old.clone();
        new.provenance = None;

        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_ok());
        assert!(ensure_no_downgrade(&old, &new, "github:o/r").is_err());

        for legacy in [ProvenanceType::Cosign, ProvenanceType::Minisign] {
            let old = PlatformInfo {
                provenance: Some(legacy),
                ..Default::default()
            };
            let mut new = PlatformInfo {
                signer: Some(
                    "sigstore-oidc:https://github.com/o/r/.github/workflows/release.yml".into(),
                ),
                ..Default::default()
            };
            assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_ok());
            new.signer = None;
            assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_err());
        }

        new.signer = None;
        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_err());

        new.signer =
            Some("sigstore-oidc:https://github.com/o/r/.github/workflows/other.yml".into());
        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_err());

        new.signer = old.signer.clone();
        let old = PlatformInfo {
            provenance: Some(ProvenanceType::Slsa { url: None }),
            ..old
        };
        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_err());
    }

    #[test]
    fn packslip_signer_follows_the_repository_id_across_a_rename() {
        let signer = |repo: &str| {
            Some(format!(
                "sigstore-oidc:https://github.com/{repo}/.github/workflows/release.yml"
            ))
        };
        let old = PlatformInfo {
            signer: signer("old/tool"),
            repository_id: Some("42".into()),
            repository_owner_id: Some("7".into()),
            ..Default::default()
        };
        let renamed = PlatformInfo {
            signer: signer("new/tool"),
            ..old.clone()
        };
        let backend = "packslip:github.com/old/tool";
        assert!(ensure_no_downgrade(&old, &renamed, backend).is_ok());
        assert!(ensure_no_downgrade(&old, &old, backend).is_ok());

        // A recreated name keeps the signer string but not the repository.
        let squatted = PlatformInfo {
            repository_id: Some("43".into()),
            ..old.clone()
        };
        assert!(ensure_no_downgrade(&old, &squatted, backend).is_err());
        // A transfer to another owner is the same repository, like a rename,
        // whatever owner ID an older mise recorded.
        let transferred = PlatformInfo {
            signer: signer("acme/tool"),
            repository_owner_id: Some("8".into()),
            ..old.clone()
        };
        assert!(ensure_no_downgrade(&old, &transferred, backend).is_ok());
        let ownerless_transfer = PlatformInfo {
            repository_owner_id: None,
            ..transferred
        };
        assert!(ensure_no_downgrade(&old, &ownerless_transfer, backend).is_ok());
        // A resolution under explicit signer options records no IDs: the same
        // signer keeps the old entry's, and another signer cannot drop them.
        let unchecked = PlatformInfo {
            repository_id: None,
            repository_owner_id: None,
            ..old.clone()
        };
        assert!(ensure_no_downgrade(&old, &unchecked, backend).is_ok());
        let mut carried = unchecked.clone();
        carry_forge_ids(&old, &mut carried);
        assert_eq!(carried, old);
        let unchecked_renamed = PlatformInfo {
            signer: signer("new/tool"),
            ..unchecked.clone()
        };
        assert!(ensure_no_downgrade(&old, &unchecked_renamed, backend).is_err());
        let mut not_carried = unchecked_renamed.clone();
        carry_forge_ids(&old, &mut not_carried);
        assert_eq!(not_carried.repository_id, None);
        // A new entry records no owner ID; the one an older mise recorded for
        // the same repository stays, and only for it.
        let ownerless = PlatformInfo {
            repository_owner_id: None,
            ..old.clone()
        };
        assert!(ensure_no_downgrade(&old, &ownerless, backend).is_ok());
        let mut kept = ownerless.clone();
        carry_forge_ids(&old, &mut kept);
        assert_eq!(kept, old);
        let mut other_repository = PlatformInfo {
            repository_id: Some("43".into()),
            ..ownerless
        };
        carry_forge_ids(&old, &mut other_repository);
        assert_eq!(other_repository.repository_owner_id, None);
        // Another workflow of the same repository is another signer.
        let other_workflow = PlatformInfo {
            signer: Some(
                "sigstore-oidc:https://github.com/new/tool/.github/workflows/other.yml".into(),
            ),
            ..old.clone()
        };
        assert!(ensure_no_downgrade(&old, &other_workflow, backend).is_err());

        // An entry locked before mise recorded forge IDs keeps the exact
        // signer comparison, and gains the IDs from the next resolution.
        let legacy = PlatformInfo {
            repository_id: None,
            repository_owner_id: None,
            ..old.clone()
        };
        assert!(ensure_no_downgrade(&legacy, &old, backend).is_ok());
        assert!(ensure_no_downgrade(&legacy, &renamed, backend).is_err());
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
        assert!(ensure_no_downgrade(&old, &PlatformInfo::default(), "github:o/r").is_err());

        let mut new = PlatformInfo {
            signer: Some(
                "sigstore-oidc:https://github.com/o/r/.github/workflows/release.yml".into(),
            ),
            ..Default::default()
        };
        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_ok());

        let old = PlatformInfo {
            additional_artifacts: vec![ArtifactInfo {
                provenance: Some(ProvenanceType::Slsa { url: None }),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_err());

        new.signer = None;
        assert!(ensure_no_downgrade(&old, &new, "packslip:github.com/o/r").is_err());
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
        assert!(ensure_no_downgrade(&old, &new, "github:o/r").is_ok());
        new.additional_artifacts[1].url = "https://example.com/verified-v2".into();
        assert!(ensure_no_downgrade(&old, &new, "github:o/r").is_ok());
        new.additional_artifacts[1].provenance = None;
        assert!(ensure_no_downgrade(&old, &new, "github:o/r").is_err());
        new.additional_artifacts.pop();
        assert!(ensure_no_downgrade(&old, &new, "github:o/r").is_err());
    }

    #[tokio::test]
    async fn artifact_checksum_hashes_local_file_urls() {
        let tmp = tempfile::tempdir().unwrap();
        let artifact = tmp.path().join("probe");
        std::fs::write(&artifact, b"artifact").unwrap();
        let url = url::Url::from_file_path(&artifact).unwrap().to_string();
        let mut info = PlatformInfo {
            url: Some(url.clone()),
            ..Default::default()
        };
        complete_artifact_checksums(&mut info).await.unwrap();
        let expected = format!(
            "sha256:{}",
            crate::hash::file_hash_sha256(&artifact, None).unwrap()
        );
        assert_eq!(info.checksum.as_deref(), Some(expected.as_str()));
        // The recorded URL stays as written.
        assert_eq!(info.url.as_deref(), Some(url.as_str()));

        let mut checksum = None;
        download_for_verification(&url, &mut checksum)
            .await
            .unwrap();
        assert_eq!(checksum.as_deref(), Some(expected.as_str()));
    }

    #[tokio::test]
    async fn artifact_checksum_sends_plugin_headers() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/artifact.zip")
            .match_header("authorization", "Bearer secret")
            .with_body("artifact")
            .create_async()
            .await;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        let mut info = PlatformInfo {
            url: Some(format!("{}/artifact.zip", server.url())),
            download_headers: Some(DownloadHeaders::Plugin(headers)),
            ..Default::default()
        };
        complete_artifact_checksums(&mut info).await.unwrap();
        mock.assert_async().await;
        let checksum = info.checksum.clone().unwrap();
        assert!(checksum.starts_with("sha256:"));
        // The headers are never persisted.
        let written = toml::to_string(&info).unwrap();
        assert!(!written.contains("secret"), "{written}");
    }

    #[tokio::test]
    async fn artifact_checksum_withholds_headers_from_a_reachable_browser_url() {
        let mut server = mockito::Server::new_async().await;
        let head = server
            .mock("HEAD", "/browser")
            .with_header("content-type", "application/octet-stream")
            .create_async()
            .await;
        let browser = server
            .mock("GET", "/browser")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_body("artifact")
            .create_async()
            .await;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        let browser_url = format!("{}/browser", server.url());
        let api_url = format!("{}/api/asset", server.url());
        let headers = DownloadHeaders::Client(headers);
        let checksum = artifact_checksum(&browser_url, Some(&api_url), Some(&headers), &[])
            .await
            .unwrap();
        head.assert_async().await;
        browser.assert_async().await;
        assert!(checksum.starts_with("sha256:"));
    }

    #[tokio::test]
    async fn artifact_checksum_sends_headers_to_the_api_url_when_the_browser_url_is_html() {
        let mut server = mockito::Server::new_async().await;
        let head = server
            .mock("HEAD", "/browser")
            .with_header("content-type", "text/html; charset=utf-8")
            .create_async()
            .await;
        let api = server
            .mock("GET", "/api/asset")
            .match_header("authorization", "Bearer secret")
            .with_body("artifact")
            .create_async()
            .await;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        let browser_url = format!("{}/browser", server.url());
        let api_url = format!("{}/api/asset", server.url());
        let headers = DownloadHeaders::Client(headers);
        let checksum = artifact_checksum(&browser_url, Some(&api_url), Some(&headers), &[])
            .await
            .unwrap();
        head.assert_async().await;
        api.assert_async().await;
        assert!(checksum.starts_with("sha256:"));
    }

    #[test]
    fn headers_for_old_urls_keeps_headers_only_for_the_same_server_and_scheme() {
        let headers = DownloadHeaders::Client(reqwest::header::HeaderMap::new());
        let h = Some(&headers);
        let new = Some("https://forge.example/api/v4/asset");
        assert!(
            headers_for_old_urls("https://forge.example/old/asset", None, None, new, h).is_some()
        );
        assert!(
            headers_for_old_urls("https://old-forge.example/asset", None, None, new, h).is_none()
        );
        assert!(
            headers_for_old_urls("http://forge.example/old/asset", None, None, new, h).is_none()
        );
        assert!(
            headers_for_old_urls(
                "https://forge.example/browser",
                Some("https://api.other.example/asset"),
                Some("https://forge.example/browser"),
                None,
                h
            )
            .is_none()
        );
        assert!(headers_for_old_urls("https://forge.example/a", None, None, None, h).is_none());
    }
}
