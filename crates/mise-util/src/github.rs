use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::tokens;
use crate::{dirs, env};
use eyre::{Result, WrapErr};
use heck::ToKebabCase;
use mise_settings::Settings;
use reqwest::IntoUrl;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock as Lazy, Mutex};
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use xx::regex;

pub mod oauth;
pub mod sigstore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    // pub name: Option<String>,
    // pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    #[serde(default)]
    pub published_at: Option<String>,
    pub assets: Vec<GithubAsset>,
}

impl GithubRelease {
    /// The time this release became public. GitHub's `created_at` is the date
    /// of the tagged commit, which may be much older than the publication date.
    pub fn released_at(&self) -> &str {
        self.published_at.as_deref().unwrap_or(&self.created_at)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTag {
    pub name: String,
    pub commit: Option<GithubTagCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTagCommit {
    pub sha: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommit {
    pub commit: GithubCommitInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommitInfo {
    pub committer: GithubCommitPerson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommitPerson {
    pub date: String,
}

/// Tag with date information
#[derive(Debug, Clone)]
pub struct GithubTagWithDate {
    pub name: String,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    // pub size: u64,
    pub browser_download_url: String,
    pub url: String,
    /// SHA256 digest provided by GitHub API (format: "sha256:hash")
    /// Will be null for releases created before this feature was added
    #[serde(default)]
    pub digest: Option<String>,
    /// When the asset was last uploaded; later than the release's
    /// `published_at` when the maintainer replaced it after publishing.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// mise-versions supplied this asset, so the asset ID in `url` must be
    /// confirmed before use. Set by mise when it pins mirrored data (a value
    /// in the mirror's own JSON is overwritten), and kept in release caches so
    /// a later run still knows.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_versions_host: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    full_name: String,
}

type CacheGroup<T> = HashMap<String, CacheManager<T>>;

static RELEASES_CACHE: Lazy<RwLock<CacheGroup<Vec<GithubRelease>>>> = Lazy::new(Default::default);

static RELEASE_CACHE: Lazy<RwLock<CacheGroup<GithubRelease>>> = Lazy::new(Default::default);

static TAGS_CACHE: Lazy<RwLock<CacheGroup<Vec<String>>>> = Lazy::new(Default::default);

static REPOSITORY_CACHE: Lazy<RwLock<CacheGroup<RepositoryIdentity>>> = Lazy::new(Default::default);

pub static API_URL: &str = "https://api.github.com";

pub static API_PATH: &str = "/api/v3";

/// Without `MISE_LIST_ALL_VERSIONS`, mise normally fetches only the first page of
/// releases to save API quota. The read path filters out prereleases/drafts by
/// default, so a repo whose most recent releases are all prereleases (e.g. nightly
/// builds) would yield zero candidates. `list_releases_` therefore keeps paginating
/// until at least one stable release is seen, bounded to this many pages. (#10343)
const MAX_RELEASE_FALLBACK_PAGES: usize = 3;

async fn get_tags_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<String>>> {
    TAGS_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}-tags.msgpack.z")))
                .with_fresh_duration(crate::network::fetch_remote_versions_cache(&Settings::get()))
                .build()
        });
    TAGS_CACHE.read().await
}

async fn get_releases_cache(key: &str) -> RwLockReadGuard<'_, CacheGroup<Vec<GithubRelease>>> {
    RELEASES_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}-all-releases.msgpack.z")))
                .with_fresh_duration(crate::network::fetch_remote_versions_cache(&Settings::get()))
                .build()
        });
    RELEASES_CACHE.read().await
}

async fn get_release_cache<'a>(key: &str) -> RwLockReadGuard<'a, CacheGroup<GithubRelease>> {
    RELEASE_CACHE
        .write()
        .await
        .entry(key.to_string())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}.msgpack.z")))
                .with_fresh_duration(crate::network::fetch_remote_versions_cache(&Settings::get()))
                .build()
        });
    RELEASE_CACHE.read().await
}

pub async fn list_releases(repo: &str) -> Result<Vec<GithubRelease>> {
    Ok(list_releases_including_prereleases(repo)
        .await?
        .into_iter()
        .filter(|r| !r.prerelease)
        .collect())
}

pub async fn list_releases_from_url(api_url: &str, repo: &str) -> Result<Vec<GithubRelease>> {
    // `false`: this variant's callers (ubi, spm, and the github backend's
    // security-feature probe) keep asset-less releases, so their pagination
    // must stop where it always did.
    Ok(
        list_releases_including_prereleases_from_url(api_url, repo, false)
            .await?
            .into_iter()
            .filter(|r| !r.prerelease)
            .collect(),
    )
}

/// Like [`list_releases`] but includes releases flagged `prerelease: true`.
/// Drafts are always filtered out. Callers opting in to pre-releases (e.g. the
/// `github:` backend with `prerelease = true`) use this variant; the cache is
/// shared with [`list_releases`] so there's no extra API cost.
pub async fn list_releases_including_prereleases(repo: &str) -> Result<Vec<GithubRelease>> {
    // The version goes into the hash: appended as text, "owner/foo" would
    // collide with the old key of a repository named "owner/foo-2".
    let key = format!(
        "{}-{}",
        repo.to_kebab_case(),
        crate::hash::hash_to_str(&(repo, RELEASE_LIST_CACHE_VERSION, mirror_source(repo)))
    );
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(remember_mirrored_assets(
        cache
            .get_or_try_init_async(async || list_releases_(API_URL, repo, false).await)
            .await?
            .to_vec(),
    ))
}

/// Re-register assets mise-versions supplied, for releases that may have come
/// out of an on-disk cache written by an earlier run.
fn remember_mirrored_assets<R: std::borrow::Borrow<GithubRelease>, T: AsRef<[R]>>(
    releases: T,
) -> T {
    for release in releases.as_ref() {
        for asset in &release.borrow().assets {
            if asset.from_versions_host {
                crate::versions_host::remember_mirrored_asset_api_url(&asset.url);
            }
        }
    }
    releases
}

/// `require_assets` reaches the fetch loop from the caller's own filter: the
/// `github:` backend drops releases with nothing uploaded, so for it a stable
/// release with no assets is not a place to stop paginating. See
/// `has_stopping_stable_release`. It is part of the cache key because the two
/// answers are different lists — the `true` list is a superset, and handing the
/// shorter one to a caller that filters is the bug this argument exists to fix.
pub async fn list_releases_including_prereleases_from_url(
    api_url: &str,
    repo: &str,
    require_assets: bool,
) -> Result<Vec<GithubRelease>> {
    let key = releases_cache_key(api_url, repo, require_assets);
    let cache = get_releases_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(remember_mirrored_assets(
        cache
            .get_or_try_init_async(async || list_releases_(api_url, repo, require_assets).await)
            .await?
            .to_vec(),
    ))
}

/// Cache key for one release listing.
///
/// `require_assets` cannot be a suffix on the readable part. Appending
/// `-with-assets` makes `("owner/foo", true)` and `("owner/foo-with-assets",
/// false)` the same string, and this cache is global and persisted, so one
/// repository would be served the other's releases. The hash of the tuple is
/// what separates them; the kebab-cased prefix is kept only so the file on disk
/// is still recognisable.
fn releases_cache_key(api_url: &str, repo: &str, require_assets: bool) -> String {
    format!(
        "{}-{}",
        format!("{api_url}-{repo}").to_kebab_case(),
        crate::hash::hash_to_str(&(
            api_url,
            repo,
            require_assets,
            RELEASE_LIST_CACHE_VERSION,
            mirror_source(repo)
        ))
    )
}

/// Bumped when cached release lists can no longer be trusted as written:
/// "2" since assets record `from_versions_host`, so a list cached before that
/// could pass mirrored asset IDs off as GitHub's.
const RELEASE_LIST_CACHE_VERSION: &str = "2";

/// Part of every release cache key: whether mise-versions may answer for
/// `repo`. Turning `use_versions_host` off, or adding a `url_replacements`
/// rule for GitHub, then starts from a fresh cache instead of serving mirror
/// data cached before it.
fn mirror_source(repo: &str) -> bool {
    // Not `prefer_offline`: offline runs should keep reading what was cached.
    Settings::get().use_versions_host && !crate::versions_host::github_is_url_replaced(Some(repo))
}

/// Whether the bounded prerelease fallback has found what it went looking for:
/// a stable release the caller will actually keep.
///
/// Without `require_assets` this is the original test — any published,
/// non-prerelease release. With it, a stable release that carries no assets no
/// longer counts, because the `github:` backend removes those from the version
/// list; stopping on one would leave an installable stable release sitting on a
/// page that is never fetched. Callers that keep every release pass `false` and
/// stop exactly where they always have.
fn has_stopping_stable_release(releases: &[GithubRelease], require_assets: bool) -> bool {
    releases
        .iter()
        .any(|r| !r.prerelease && !r.draft && (!require_assets || !r.assets.is_empty()))
}

async fn list_releases_(
    api_url: &str,
    repo: &str,
    require_assets: bool,
) -> Result<Vec<GithubRelease>> {
    if is_public_github_api_base(api_url)
        && let Some(releases) = list_releases_from_versions_host(repo, require_assets).await
    {
        return Ok(releases);
    }

    let mut url = format!("{api_url}/repos/{repo}/releases?per_page=100");
    let headers = get_headers(&url)?;
    let (mut releases, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubRelease>, _>(&url, &headers)
        .await?;

    // Fetch additional pages when MISE_LIST_ALL_VERSIONS is set, or (bounded) while
    // every release seen so far is a prerelease/draft so a stable release is still
    // discovered on a repo dominated by nightlies. (#10343)
    // pages_fetched counts the initial page already fetched above, so the cap
    // applies to the total number of pages rather than to extra requests.
    let mut pages_fetched = 1;
    while let Some(next) = next_page(&headers) {
        if !*env::MISE_LIST_ALL_VERSIONS
            && (has_stopping_stable_release(&releases, require_assets)
                || pages_fetched >= MAX_RELEASE_FALLBACK_PAGES)
        {
            break;
        }
        url = crate::http::resolve_pagination_url(&url, &next)?;
        headers = get_headers(&url)?;
        let (more, h) = crate::http::HTTP_FETCH
            .json_headers_with_headers::<Vec<GithubRelease>, _>(&url, &headers)
            .await?;
        releases.extend(more);
        headers = h;
        pages_fetched += 1;
    }
    releases.retain(|r| !r.draft);

    Ok(releases)
}

/// [`list_releases_`] through mise-versions. `None` sends the caller to
/// GitHub for the whole list rather than for the remaining pages, since two
/// sources may not agree on where a page ends.
async fn list_releases_from_versions_host(
    repo: &str,
    require_assets: bool,
) -> Option<Vec<GithubRelease>> {
    let releases =
        paginate_mirrored_releases(require_assets, *env::MISE_LIST_ALL_VERSIONS, |page| async move {
            match crate::versions_host::github_releases(repo, page).await {
                Ok(list) => list,
                Err(err) => {
                    warn!(
                        "mise-versions endpoint=github_releases repo={repo} page={page} outcome=failed fallback=true error={err:#}"
                    );
                    None
                }
            }
        })
        .await?;
    trace!("got GitHub releases for {repo} from mise-versions");
    Some(releases)
}

/// Page through mirrored release lists with the same stopping rules as the
/// direct loop in [`list_releases_`].
async fn paginate_mirrored_releases<F, Fut>(
    require_assets: bool,
    list_all: bool,
    mut fetch_page: F,
) -> Option<Vec<GithubRelease>>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Option<crate::versions_host::GithubReleasesPage>>,
{
    let mut releases = Vec::new();
    let mut page = 1;
    let mut pages_fetched = 0;
    loop {
        let list = fetch_page(page).await?;
        releases.extend(list.releases);
        pages_fetched += 1;
        let more = list.next_page.is_some() || list.truncated;
        if !more
            || !list_all
                && (has_stopping_stable_release(&releases, require_assets)
                    || pages_fetched >= MAX_RELEASE_FALLBACK_PAGES)
        {
            break;
        }
        // `None` past the pages mise-versions serves: GitHub has to answer.
        // The page count bounds the loop even if a page number repeats.
        if pages_fetched >= crate::versions_host::GITHUB_RELEASES_MAX_PAGES {
            return None;
        }
        page = list.next_page?;
    }
    Some(releases)
}

/// Whether any of `attested` (repositories that attestations vouch for, as
/// `owner/repo`) is `owner/repo`, directly or through a rename or transfer.
pub async fn attested_by_repository(owner: &str, repo: &str, attested: &[String]) -> bool {
    let requested = format!("{owner}/{repo}");
    if attested.iter().any(|a| a.eq_ignore_ascii_case(&requested)) {
        return true;
    }
    // A rename or transfer: attestations name the repository as it was when
    // they were made. Ask GitHub where both names lead now. It keeps
    // redirecting an old name until someone else takes it, so matching here
    // is GitHub's word that they are the same repository.
    let Ok(canonical) = canonical_repo(&requested).await else {
        return false;
    };
    let mut others: Vec<String> = attested.iter().map(|a| a.to_ascii_lowercase()).collect();
    others.sort();
    others.dedup();
    // An attestation set is a handful of entries; don't let a long one turn
    // into a stream of requests.
    for other in others.iter().take(5) {
        if canonical_repo(other)
            .await
            .is_ok_and(|c| c.eq_ignore_ascii_case(&canonical))
        {
            return true;
        }
    }
    false
}

pub async fn list_tags(repo: &str) -> Result<Vec<String>> {
    let key = repo.to_kebab_case();
    let cache = get_tags_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            list_tags_(API_URL, repo, *env::MISE_LIST_ALL_VERSIONS).await
        })
        .await?
        .to_vec())
}

pub async fn list_tags_from_url(api_url: &str, repo: &str) -> Result<Vec<String>> {
    let key = format!("{api_url}-{repo}").to_kebab_case();
    let cache = get_tags_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            list_tags_(api_url, repo, *env::MISE_LIST_ALL_VERSIONS).await
        })
        .await?
        .to_vec())
}

/// `list_all` is `MISE_LIST_ALL_VERSIONS`, taken as an argument rather than read here so
/// tests can exercise the pagination loop: the env var is a process-wide `Lazy` that other
/// modules' tests have already forced by the time this one runs.
async fn list_tags_(api_url: &str, repo: &str, list_all: bool) -> Result<Vec<String>> {
    let mut url = format!("{api_url}/repos/{repo}/tags?per_page=100");
    let headers = get_headers(&url)?;
    let (mut tags, mut headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
        .await?;

    if list_all {
        while let Some(next) = next_page(&headers) {
            url = crate::http::resolve_pagination_url(&url, &next)?;
            headers = get_headers(&url)?;
            let (more, h) = crate::http::HTTP_FETCH
                .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
                .await?;
            tags.extend(more);
            headers = h;
        }
    }

    Ok(tags.into_iter().map(|t| t.name).collect())
}

/// List tags with their commit dates. This is slower than `list_tags` as it requires
/// fetching commit info for each tag. Use only when MISE_LIST_ALL_VERSIONS is set.
pub async fn list_tags_with_dates(repo: &str) -> Result<Vec<GithubTagWithDate>> {
    list_tags_with_dates_(API_URL, repo).await
}

async fn list_tags_with_dates_(api_url: &str, repo: &str) -> Result<Vec<GithubTagWithDate>> {
    let mut url = format!("{api_url}/repos/{repo}/tags?per_page=100");
    let headers = get_headers(&url)?;
    let (mut tags, mut response_headers) = crate::http::HTTP_FETCH
        .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &headers)
        .await?;

    // Fetch all pages when MISE_LIST_ALL_VERSIONS is set
    while let Some(next) = next_page(&response_headers) {
        url = crate::http::resolve_pagination_url(&url, &next)?;
        response_headers = get_headers(&url)?;
        let (more, h) = crate::http::HTTP_FETCH
            .json_headers_with_headers::<Vec<GithubTag>, _>(&url, &response_headers)
            .await?;
        tags.extend(more);
        response_headers = h;
    }

    // Fetch commit dates in parallel using the parallel utility
    let results = crate::parallel::parallel(tags, |tag| async move {
        let date = if let Some(commit) = tag.commit {
            let headers = get_headers(&commit.url)?;
            match crate::http::HTTP_FETCH
                .json_with_headers::<GithubCommit, _>(&commit.url, &headers)
                .await
            {
                Ok(commit_info) => Some(commit_info.commit.committer.date),
                Err(e) => {
                    warn!("Failed to fetch commit date for tag {}: {}", tag.name, e);
                    None
                }
            }
        } else {
            None
        };
        Ok((tag.name, date))
    })
    .await?;

    Ok(results
        .into_iter()
        .map(|(name, date)| GithubTagWithDate { name, date })
        .collect())
}

pub async fn get_release(repo: &str, tag: &str) -> Result<GithubRelease> {
    get_release_with_versions_host(repo, tag, true).await
}

/// What GitHub says a repository name stands for now: the repository's
/// immutable IDs and its current name. A renamed or transferred repository's
/// old name answers with the new name and the same `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryIdentity {
    /// The repository's numeric ID, which no rename or transfer changes.
    pub id: String,
    /// `owner/repo` as the repository is called now.
    pub full_name: String,
    /// The numeric ID of the repository's owner.
    pub owner_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubRepositoryIds {
    id: u64,
    full_name: String,
    owner: Option<GithubRepositoryOwner>,
}

#[derive(Debug, Deserialize)]
struct GithubRepositoryOwner {
    id: u64,
}

/// The identity GitHub gives `owner/repo` now, following a rename's
/// redirect. Cached like a release listing.
pub async fn repository_identity(repo: &str) -> Result<RepositoryIdentity> {
    let key = format!(
        "{}-repository-{}",
        repo.to_kebab_case(),
        crate::hash::hash_to_str(&repo)
    );
    REPOSITORY_CACHE
        .write()
        .await
        .entry(key.clone())
        .or_insert_with(|| {
            CacheManagerBuilder::new(cache_dir().join(format!("{key}.msgpack.z")))
                .with_fresh_duration(crate::network::fetch_remote_versions_cache(&Settings::get()))
                .build()
        });
    let caches = REPOSITORY_CACHE.read().await;
    let cache = caches.get(&key).unwrap();
    Ok(cache
        .get_or_try_init_async(async || {
            let url = format!("{API_URL}/repos/{repo}");
            let headers = get_headers(&url)?;
            let repository: GithubRepositoryIds = crate::http::HTTP_FETCH
                .json_with_headers(url, &headers)
                .await?;
            Ok(RepositoryIdentity {
                id: repository.id.to_string(),
                full_name: repository.full_name,
                owner_id: repository.owner.map(|o| o.id.to_string()),
            })
        })
        .await?
        .clone())
}

/// Resolve a repository name through GitHub so callers can follow repository
/// transfers without weakening identity checks to the former owner/name.
pub async fn canonical_repo(repo: &str) -> Result<String> {
    let url = format!("{API_URL}/repos/{repo}");
    let headers = get_headers(&url)?;
    let repository: GithubRepository = crate::http::HTTP_FETCH
        .json_with_headers(url, &headers)
        .await?;
    Ok(repository.full_name)
}

pub async fn get_release_with_versions_host(
    repo: &str,
    tag: &str,
    use_versions_host: bool,
) -> Result<GithubRelease> {
    let key = release_cache_key(API_URL, repo, tag, use_versions_host);
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    let release = cache
        .get_or_try_init_async_if(
            async || get_release_with_options(API_URL, repo, tag, use_versions_host).await,
            should_cache_release,
        )
        .await?;
    remember_mirrored_assets([&release]);
    Ok(release)
}

pub async fn get_release_for_url_with_versions_host(
    api_url: &str,
    repo: &str,
    tag: &str,
    use_versions_host: bool,
) -> Result<GithubRelease> {
    let key = release_cache_key(api_url, repo, tag, use_versions_host);
    let cache = get_release_cache(&key).await;
    let cache = cache.get(&key).unwrap();
    let release = cache
        .get_or_try_init_async_if(
            async || get_release_with_options(api_url, repo, tag, use_versions_host).await,
            should_cache_release,
        )
        .await?;
    remember_mirrored_assets([&release]);
    Ok(release)
}

fn release_cache_key(api_url: &str, repo: &str, tag: &str, use_versions_host: bool) -> String {
    // "hosted-2": entries from before assets recorded `from_versions_host`
    // would pass mirrored asset IDs off as GitHub's, so they aren't reused.
    let source = if use_versions_host
        && mirror_source(repo)
        && !crate::versions_host::github_release_is_url_replaced(repo, tag)
    {
        "hosted-2"
    } else {
        "direct"
    };
    format!("{api_url}-{repo}-{tag}-{source}").to_kebab_case()
}

#[doc(hidden)]
pub fn release_cache_key_for_test(repo: &str, tag: &str) -> String {
    release_cache_key(API_URL, repo, tag, true)
}

fn should_cache_release(release: &GithubRelease) -> bool {
    !release.assets.is_empty()
}

/// Find the latest build revision for a version in a GitHub repo.
///
/// Build revisions use the pattern `{version}-{N}` where N is an incrementing integer.
/// For example, given version "3.3.11", this will prefer tag "3.3.11-2" over "3.3.11-1"
/// over "3.3.11". Returns the release with the highest build revision and whether
/// a numeric build revision tag was found.
///
/// This is used by precompiled binary repos (e.g., jdx/ruby) where binaries may be
/// rebuilt with different checksums while keeping the same upstream version.
///
/// Note: this relies on `list_releases` which may only return the first page of results
/// when `MISE_LIST_ALL_VERSIONS` is not set. For repos with many releases, older versions
/// may not be found, falling back to the exact version tag via `get_release`.
#[cfg_attr(windows, allow(dead_code))]
pub async fn get_release_with_build_revision_status(
    repo: &str,
    version: &str,
    use_versions_host: bool,
) -> Result<(GithubRelease, bool)> {
    let releases = list_releases(repo).await?;
    match pick_best_numeric_build_revision(releases.clone(), version) {
        Some(release) => Ok((release, true)),
        None => match pick_best_build_revision(releases, version) {
            Some(release) => Ok((release, false)),
            None => Ok((
                get_release_with_versions_host(repo, version, use_versions_host).await?,
                false,
            )),
        },
    }
}

/// Select the highest numeric build revision for a given version.
///
/// Given releases with tags like "3.3.11", "3.3.11-1", "3.3.11-2", picks the
/// highest numeric `-N` suffix and ignores the base version.
#[cfg_attr(windows, allow(dead_code))]
fn pick_best_numeric_build_revision(
    releases: Vec<GithubRelease>,
    version: &str,
) -> Option<GithubRelease> {
    let prefix = format!("{version}-");
    releases
        .into_iter()
        .filter_map(|r| {
            let revision = r
                .tag_name
                .strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<u32>().ok())?;
            Some((revision, r))
        })
        .max_by_key(|(revision, _)| *revision)
        .map(|(_, release)| release)
}

/// Select the release with the highest build revision for a given version.
///
/// Given releases with tags like "3.3.11", "3.3.11-1", "3.3.11-2", picks the one
/// with the highest numeric `-N` suffix. The base version (no suffix) is treated as
/// revision 0.
#[cfg_attr(windows, allow(dead_code))]
fn pick_best_build_revision(releases: Vec<GithubRelease>, version: &str) -> Option<GithubRelease> {
    let prefix = format!("{version}-");
    releases
        .into_iter()
        .filter(|r| {
            r.tag_name == version
                || r.tag_name
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| suffix.parse::<u32>().is_ok())
        })
        .max_by_key(|r| {
            r.tag_name
                .strip_prefix(&prefix)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0)
        })
}

async fn get_release_with_options(
    api_url: &str,
    repo: &str,
    tag: &str,
    use_versions_host: bool,
) -> Result<GithubRelease> {
    if use_versions_host
        && is_public_github_api_base(api_url)
        && let Ok(Some(release)) = crate::versions_host::github_release(repo, tag).await
    {
        trace!("got GitHub release {repo}@{tag} from mise-versions");
        return Ok(release);
    }

    let url = if tag == "latest" {
        format!("{api_url}/repos/{repo}/releases/latest")
    } else {
        // As one path segment: a tag may hold `#` or `/`, which would
        // otherwise start a fragment or reach a different path. GitHub accepts
        // the encoded form, and `versions_host` already sends it that way.
        format!(
            "{api_url}/repos/{repo}/releases/tags/{}",
            urlencoding::encode(tag)
        )
    };
    let headers = get_headers(&url)?;
    crate::http::HTTP_FETCH
        .json_with_headers(url, &headers)
        .await
}

fn is_public_github_api_base(api_url: &str) -> bool {
    api_url.trim_end_matches('/') == API_URL
}

fn next_page(headers: &HeaderMap) -> Option<String> {
    let link = headers
        .get("link")
        .map(|l| l.to_str().unwrap_or_default().to_string())
        .unwrap_or_default();
    regex!(r#"<([^>]+)>; rel="next""#)
        .captures(&link)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}

fn cache_dir() -> PathBuf {
    dirs::CACHE.join("github")
}

/// The source from which a GitHub token was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar(&'static str),
    TokensFile,
    GhCli,
    CredentialCommand,
    GithubOauth,
    GitCredential,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSource::EnvVar(name) => write!(f, "{name}"),
            TokenSource::TokensFile => write!(f, "github_tokens.toml"),
            TokenSource::GhCli => write!(f, "gh CLI (hosts.yml)"),
            TokenSource::CredentialCommand => write!(f, "credential_command"),
            TokenSource::GithubOauth => write!(f, "GitHub OAuth"),
            TokenSource::GitCredential => write!(f, "git credential fill"),
        }
    }
}

/// Map API hostnames to the hostnames where GitHub tokens are commonly stored.
fn canonical_token_host(host: &str) -> &str {
    match host {
        "api.github.com" => "github.com",
        // Repository file contents, authenticated by the same github.com token.
        "raw.githubusercontent.com" => "github.com",
        h if is_ghe_com_api_host(h) => h.strip_prefix("api.").unwrap_or(h),
        other => other,
    }
}

/// Repository file contents, e.g. `raw.githubusercontent.com/<owner>/<repo>/<ref>/<path>`.
///
/// A private repository's files are a 404 without a token and a 200 with one, so
/// this host needs the same bearer token `api.github.com` gets. It is not an API
/// host, so it gets no `x-github-api-version` header, and it is deliberately
/// distinct from the release-asset hosts below: those URLs are pre-signed, and
/// sending an Authorization header alongside the signature makes the storage
/// backend reject the request. `resolve_token` enforces that separately by
/// refusing to resolve a token for an asset host at all.
pub fn is_github_raw_content_url(url: &url::Url) -> bool {
    // https only. A token must never ride a cleartext request, and nothing
    // legitimately fetches raw content over http: the scheme check costs
    // nothing and closes the downgrade.
    url.scheme() == "https" && url.host_str() == Some("raw.githubusercontent.com")
}

fn is_github_release_asset_host(host: &str) -> bool {
    matches!(
        host,
        "objects.githubusercontent.com"
            | "objects-origin.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}

fn is_ghe_com_api_host(host: &str) -> bool {
    host.starts_with("api.") && host.ends_with(".ghe.com")
}

fn is_ghes_api_path(path: &str) -> bool {
    path == API_PATH
        || path
            .strip_prefix(API_PATH)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn token_lookup_hosts(host: &str) -> Vec<&str> {
    let canonical = canonical_token_host(host);
    if canonical == host {
        vec![host]
    } else {
        vec![canonical, host]
    }
}

/// Returns true for GitHub REST API URLs.
///
/// Auth and API-version headers must be scoped to these URLs only. Browser URLs
/// such as github.com release downloads and content/CDN URLs under
/// githubusercontent.com are not REST API URLs and can reject or mishandle those
/// headers.
pub fn is_github_api_url(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    host == "api.github.com"
        || is_ghe_com_api_host(host)
        || (host != "github.com"
            && !host.ends_with(".githubusercontent.com")
            && !host.ends_with(".ghe.com")
            && is_ghes_api_path(url.path()))
}

/// Pick which URL to use for a GitHub download.
///
/// Public repositories serve release assets, archives, and raw content at their browser-facing
/// URLs, so those URLs are used when reachable. Private repositories return 404 — or a 200 HTML
/// login page — there even with a valid token; in that case the file is fetched from its GitHub
/// API endpoint instead. `get_headers`/`host_auth_headers` add the bearer token and the media type
/// required by release assets or repository content. Shared by GitHub-backed installers so they
/// resolve private downloads consistently.
pub async fn pick_reachable_asset_url(browser_url: &str, api_url: &str) -> String {
    if browser_url == api_url {
        return browser_url.to_string();
    }
    match crate::http::HTTP.head(browser_url).await {
        Ok(resp) => {
            let is_html = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                // HTTP media types are case-insensitive, and the header may carry params
                // (e.g. `text/html; charset=utf-8`), so lowercase before matching.
                .is_some_and(|ct| ct.to_ascii_lowercase().contains("text/html"));
            if is_html {
                debug!(
                    "browser URL returned HTML (likely an auth page), \
                     using the API asset endpoint"
                );
                checked_api_asset_url(browser_url, api_url).await
            } else {
                browser_url.to_string()
            }
        }
        Err(e) => {
            debug!("HEAD on browser URL failed ({e}), using the API asset endpoint");
            checked_api_asset_url(browser_url, api_url).await
        }
    }
}

/// `api_url` if the GitHub release asset it names is the file `browser_url`
/// downloads; otherwise `browser_url`, which then fails on its own.
///
/// mise-versions can pair a correct browser URL with any asset ID in the
/// repository, and the ID is only used here, when the browser URL can't be.
/// One metadata request settles it: GitHub reports which tag and file the ID
/// is. Only IDs mise-versions supplied are checked, so GitHub's own release
/// data (private repos, where this fallback is routine) costs nothing extra.
async fn checked_api_asset_url(browser_url: &str, api_url: &str) -> String {
    if !crate::versions_host::is_mirrored_asset_api_url(api_url) {
        return api_url.to_string();
    }
    let asset = async {
        let mut headers = get_headers(api_url)?;
        headers.insert(
            "accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        crate::http::HTTP_FETCH
            .json_with_headers::<GithubAsset, _>(api_url, &headers)
            .await
    };
    match asset.await {
        Ok(asset) if same_release_download(browser_url, &asset.browser_download_url) => {
            api_url.to_string()
        }
        Ok(asset) => {
            warn!(
                "GitHub release asset {api_url} is {}, not {browser_url}; not using it",
                asset.browser_download_url
            );
            browser_url.to_string()
        }
        Err(err) => {
            warn!("could not confirm GitHub release asset {api_url} is {browser_url}: {err:#}");
            browser_url.to_string()
        }
    }
}

/// Whether two github.com release download URLs are the same tag and file.
/// The repository may differ: GitHub reports a renamed repository's new name.
fn same_release_download(a: &str, b: &str) -> bool {
    fn tag_and_file(url: &str) -> Option<String> {
        let url = url::Url::parse(url).ok()?;
        let (_, rest) = url.path().split_once("/releases/download/")?;
        urlencoding::decode(rest).ok().map(|rest| rest.into_owned())
    }
    tag_and_file(a).is_some_and(|a| tag_and_file(b).is_some_and(|b| a == b))
}

/// Split a `github.com/{owner}/{repo}/releases/download/{tag}/{asset}` browser
/// URL into `(owner/repo, tag, asset name)`. `None` for any other URL.
///
/// A tag may contain `/`, and GitHub leaves those literal in the download URL
/// (`.../download/@biomejs/biome@2.5.2/biome-linux-x64`) while percent-encoding
/// the rest. An asset name never contains one, so the last segment is the asset
/// and everything before it is the tag.
pub fn release_asset_from_url(url: &str) -> Option<(String, String, String)> {
    let url = url::Url::parse(url).ok()?;
    if url.host_str()? != "github.com" {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let [owner, repo, "releases", "download", tail @ ..] = segments.as_slice() else {
        return None;
    };
    let (asset, tag) = tail.split_last()?;
    if tag.is_empty() || asset.is_empty() {
        return None;
    }
    let tag = tag
        .iter()
        .map(|segment| urlencoding::decode(segment).map(|s| s.into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?
        .join("/");
    let asset = urlencoding::decode(asset).ok()?.into_owned();
    Some((format!("{owner}/{repo}"), tag, asset))
}

/// Explain a checksum mismatch on a GitHub release asset whose upstream bytes
/// changed after the expected checksum was recorded.
///
/// Only runs once verification has already failed, so it asks GitHub directly
/// and skips the release cache and mise-versions: either may still hold the
/// digest of the replaced upload. `None` when `download_url` is not a
/// github.com release download, the lookup fails, or GitHub's current digest
/// does not match the downloaded file.
pub async fn checksum_mismatch_note(
    download_url: &str,
    file: &Path,
    from_lockfile: bool,
) -> Option<String> {
    let (repo, tag, asset_name) = release_asset_from_url(download_url)?;
    let release = match get_release_with_options(API_URL, &repo, &tag, false).await {
        Ok(release) => release,
        Err(err) => {
            debug!("failed to check GitHub release {repo}@{tag} after checksum mismatch: {err:#}");
            return None;
        }
    };
    let actual = match crate::hash::file_hash_sha256(file, None) {
        Ok(actual) => actual,
        Err(err) => {
            debug!(
                "failed to hash {} after checksum mismatch: {err:#}",
                file.display()
            );
            return None;
        }
    };
    replaced_asset_note(&repo, &release, &asset_name, &actual, from_lockfile)
}

/// Append [`checksum_mismatch_note`] to a failed verification when it failed
/// on a checksum mismatch; size, I/O, and malformed-checksum errors pass through.
/// `from_lockfile` says whether mise.lock held the expected checksum, rather
/// than release metadata fetched for this install.
///
/// Install failures are rendered with `{:#}`, which drops color-eyre sections,
/// so the hint goes into the message itself.
pub async fn with_checksum_mismatch_note(
    err: eyre::Report,
    download_url: &str,
    file: &Path,
    from_lockfile: bool,
) -> eyre::Report {
    if !err
        .chain()
        .any(|cause| cause.is::<crate::hash::ChecksumMismatch>())
    {
        return err;
    }
    match checksum_mismatch_note(download_url, file, from_lockfile).await {
        Some(note) => eyre::eyre!("{err:#}\nhint: {note}"),
        None => err,
    }
}

fn replaced_asset_note(
    repo: &str,
    release: &GithubRelease,
    asset_name: &str,
    actual_sha256: &str,
    from_lockfile: bool,
) -> Option<String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)?;
    let digest = asset.digest.as_deref()?;
    if !digest
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.eq_ignore_ascii_case(actual_sha256))
    {
        return None;
    }
    let tag = &release.tag_name;
    let timing = match (&asset.updated_at, &release.published_at) {
        (Some(updated_at), Some(published_at)) => {
            format!(" (asset updated {updated_at}, release published {published_at})")
        }
        (Some(updated_at), None) => format!(" (asset updated {updated_at})"),
        _ => String::new(),
    };
    let remedy = if from_lockfile {
        "If you trust the new upload, update the checksum in mise.lock."
    } else {
        "The expected checksum came from cached release metadata, which refreshes within \
         about an hour; `mise cache clear` drops mise's local copy."
    };
    Some(format!(
        "GitHub's current digest for {asset_name} in {repo} {tag} matches this download{timing}, \
         so the expected checksum is out of date: the maintainer likely re-uploaded the asset. \
         {remedy}"
    ))
}

/// The API endpoint that serves the release asset a browser-facing URL names.
///
/// A private repository answers `github.com/.../releases/download/...` with 404
/// even for a caller holding a valid token, so the asset has to be fetched from
/// `api.github.com/repos/{repo}/releases/assets/{id}` instead — and only the
/// release metadata knows that id. `None` when the URL is not a GitHub release
/// download, the release cannot be read, or it carries no asset by that name.
///
/// Pass `use_versions_host: false` when the browser URL has already failed: the
/// repository is most likely private, so mise-versions cannot hold the release
/// and asking would only tell a public host the owner, repository, and tag.
pub async fn release_asset_api_url(browser_url: &str, use_versions_host: bool) -> Option<String> {
    let (repo, tag, asset_name) = release_asset_from_url(browser_url)?;
    let release = match get_release_with_versions_host(&repo, &tag, use_versions_host).await {
        Ok(release) => release,
        Err(err) => {
            debug!("failed to resolve GitHub release asset {repo}@{tag}/{asset_name}: {err:#}");
            return None;
        }
    };
    match release.assets.iter().find(|asset| asset.name == asset_name) {
        Some(asset) => Some(asset.url.clone()),
        None => {
            debug!("GitHub release {repo}@{tag} did not include asset {asset_name}");
            None
        }
    }
}

/// Standard GitHub token env vars, in precedence order (applies to every host).
const GITHUB_TOKEN_ENV_VARS: &[&str] = &["MISE_GITHUB_TOKEN", "GITHUB_API_TOKEN", "GITHUB_TOKEN"];

static TOKEN_SOURCES: Lazy<Mutex<HashMap<String, (String, TokenSource)>>> =
    Lazy::new(Default::default);

/// Remembers the source of the token used to build a request header.
///
/// The 401 error path must not call [`resolve_token`] again: OAuth resolution can
/// synchronously refresh a token, which would block inside the async HTTP send path.
pub fn remember_token_source(host: &str, token: &str, source: TokenSource) {
    TOKEN_SOURCES
        .lock()
        .unwrap()
        .insert(host.to_string(), (token.to_string(), source));
}

/// Returns the recorded source only when `token` is the token sent for `host`.
///
/// Matching both values prevents a netrc or caller-provided Authorization header
/// from being attributed to an unrelated GitHub credential.
pub fn token_source_for_token(host: &str, token: &str) -> Option<TokenSource> {
    TOKEN_SOURCES
        .lock()
        .unwrap()
        .get(host)
        .filter(|(recorded, _)| recorded == token)
        .map(|(_, source)| source.clone())
}

/// Resolve the GitHub token for the given hostname, returning the token and its source.
///
/// Priority:
/// 1. `MISE_GITHUB_ENTERPRISE_TOKEN` env var (non-github.com only)
/// 2. `MISE_GITHUB_TOKEN` / `GITHUB_API_TOKEN` / `GITHUB_TOKEN` env vars
/// 3. `credential_command` (if set)
/// 4. native GitHub OAuth device-flow token (if configured)
/// 5. `github_tokens.toml` (per-host)
/// 6. gh CLI token (from `hosts.yml`)
/// 7. `git credential fill` (if enabled)
pub fn resolve_token(host: &str) -> Option<(String, TokenSource)> {
    resolve_token_inner(host, true)
}

/// Git already runs its configured helpers; do not recursively invoke them.
pub fn resolve_token_for_git(host: &str) -> Option<(String, TokenSource)> {
    resolve_token_inner(host, false)
}

fn resolve_token_inner(host: &str, use_git_credentials: bool) -> Option<(String, TokenSource)> {
    let settings = Settings::get();

    if is_github_release_asset_host(host) {
        return None;
    }

    if crate::testing::in_tests()
        && let Some(token) = test_support::lookup_tokens_file_override(&token_lookup_hosts(host))
    {
        return Some((token, TokenSource::TokensFile));
    }

    // Classify through the canonical host so every github.com-backed service is
    // covered by one rule. raw.githubusercontent.com is the case that matters:
    // treated as an enterprise host it would be handed
    // MISE_GITHUB_ENTERPRISE_TOKEN below, sending a credential for a private
    // GHES instance to a public GitHub service.
    let is_ghcom = canonical_token_host(host) == "github.com";
    let lookup_hosts = token_lookup_hosts(host);

    // 1. Enterprise token (non-github.com only)
    if !is_ghcom && let Some(token) = env::scoped_var("MISE_GITHUB_ENTERPRISE_TOKEN") {
        return Some((token, TokenSource::EnvVar("MISE_GITHUB_ENTERPRISE_TOKEN")));
    }

    // 2. Standard env vars (checked individually for correct precedence and source reporting)
    for var_name in GITHUB_TOKEN_ENV_VARS {
        if let Some(token) = env::scoped_var(var_name) {
            return Some((token, TokenSource::EnvVar(var_name)));
        }
    }

    // 3. credential_command — call once with the canonical host so
    // `github.com` and `api.github.com` (same instance) share a cache
    // entry, while `github.com` vs a GHE host stay separate. Walking
    // `lookup_hosts` here would spawn the helper twice on a single
    // `resolve_token("api.github.com")` whenever the first call returned
    // `None`, which manifests as extra password-manager prompts.
    let credential_command = &settings.github.credential_command;
    if use_git_credentials
        && !credential_command.is_empty()
        && let Some(canonical) = lookup_hosts.first()
        && let Some(token) =
            tokens::get_credential_command_token("github", credential_command, canonical)
    {
        return Some((token, TokenSource::CredentialCommand));
    }

    // 4. native GitHub OAuth device-flow token. Asked about the canonical host
    // for the same reason as the credential command above: the resolver matches
    // the configured OAuth endpoint, which knows `github.com` and
    // `api.github.com` but not `raw.githubusercontent.com`, so passing the raw
    // host would silently return no token for a user authenticated by device
    // flow rather than by GITHUB_TOKEN.
    if let Some(token) = oauth::resolve_token(canonical_token_host(host)) {
        return Some((token, TokenSource::GithubOauth));
    }

    // 5. github_tokens.toml
    for lookup_host in &lookup_hosts {
        if let Some(token) = MISE_GITHUB_TOKENS.get(*lookup_host) {
            return Some((token.clone(), TokenSource::TokensFile));
        }
    }

    // 6. gh CLI hosts.yml
    if settings.github.gh_cli_tokens {
        for lookup_host in &lookup_hosts {
            if let Some(token) = GH_HOSTS.get(*lookup_host) {
                return Some((token.clone(), TokenSource::GhCli));
            }
        }
    }

    // 7. git credential fill
    if use_git_credentials && settings.github.use_git_credentials {
        for lookup_host in &lookup_hosts {
            if let Some(token) = tokens::get_git_credential_token("github", lookup_host) {
                return Some((token, TokenSource::GitCredential));
            }
        }
    }

    None
}

/// Resolve the GitHub token from a full API base URL (e.g., "https://api.github.com").
/// Extracts the hostname and delegates to [`resolve_token`].
pub fn resolve_token_for_api_url(api_url: &str) -> Option<String> {
    let parsed = url::Url::parse(api_url).ok();
    let host = parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or("api.github.com");
    resolve_token(host).map(|(t, _)| t)
}

pub fn get_headers<U: IntoUrl>(url: U) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let url = url
        .into_url()
        .wrap_err("invalid request URL for GitHub auth headers")?;

    if is_github_api_url(&url) {
        let host = url.host_str().unwrap_or("github.com");
        if let Some((token, source)) = resolve_token(host) {
            remember_token_source(host, &token, source);
            headers.insert(
                reqwest::header::AUTHORIZATION,
                tokens::bearer_header("GitHub", &token)?,
            );
            headers.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
        } else {
            TOKEN_SOURCES.lock().unwrap().remove(host);
        }
    }

    // Not an API URL, so the block above skipped it, but a private repository's
    // raw file still needs the token.
    if !is_github_api_url(&url)
        && is_github_raw_content_url(&url)
        && let Some((token, source)) = resolve_token("raw.githubusercontent.com")
    {
        remember_token_source("raw.githubusercontent.com", &token, source);
        headers.insert(
            reqwest::header::AUTHORIZATION,
            tokens::bearer_header("GitHub", &token)?,
        );
    }

    if is_github_api_url(&url) && url.path().contains("/releases/assets/") {
        headers.insert(
            "accept",
            HeaderValue::from_static("application/octet-stream"),
        );
    } else if is_github_api_url(&url) && url.path().contains("/contents/") {
        // https://docs.github.com/en/rest/repos/contents#custom-media-types-for-repository-contents
        headers.insert(
            "accept",
            HeaderValue::from_static("application/vnd.github.raw"),
        );
    }

    Ok(headers)
}

// ── github_tokens.toml ──────────────────────────────────────────────

/// Tokens from $MISE_CONFIG_DIR/github_tokens.toml.
/// Maps hostname (e.g. "github.com") to token string.
static MISE_GITHUB_TOKENS: Lazy<HashMap<String, String>> =
    Lazy::new(|| read_mise_github_tokens().unwrap_or_default());

#[cfg(test)]
fn parse_github_tokens(contents: &str) -> Option<HashMap<String, String>> {
    tokens::parse_tokens_toml(contents)
}

fn read_mise_github_tokens() -> Option<HashMap<String, String>> {
    tokens::read_tokens_toml("github_tokens.toml", "github_tokens.toml")
}

// ── gh CLI hosts.yml ────────────────────────────────────────────────

/// Tokens read from the gh CLI hosts config (~/.config/gh/hosts.yml).
/// Maps hostname (e.g. "github.com") to oauth_token.
static GH_HOSTS: Lazy<HashMap<String, String>> = Lazy::new(|| read_gh_hosts().unwrap_or_default());

/// Resolve the path to gh CLI's hosts.yml, following go-gh's own `ConfigDir()`:
/// 1. `$GH_CONFIG_DIR/hosts.yml`
/// 2. `$XDG_CONFIG_HOME/gh/hosts.yml` — only when that variable is actually set, which is gh's
///    condition; `env::XDG_CONFIG_HOME` defaults to `~/.config` and so cannot express it
/// 3. `%APPDATA%\GitHub CLI\hosts.yml` on Windows — gh checks `AppData` explicitly rather than
///    going through XDG, so this is the default location there and mise never looked at it.
///    Like gh, this branch is taken only when the variable is actually set.
/// 4. `~/.config/gh/hosts.yml`, gh's own last branch, used as the fallback
///
/// The macOS candidate is kept for compatibility but does not correspond to a gh branch: gh
/// uses `~/.config/gh` on macOS too.
fn gh_hosts_path() -> Option<PathBuf> {
    // Explicit GH_CONFIG_DIR takes priority. Empty means unset, as in go-gh's
    // `os.Getenv(ghConfigDir) != ""`; `var_os` rather than `var` so a non-UTF-8 directory is
    // honoured instead of silently skipped.
    if let Some(dir) = std::env::var_os("GH_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("hosts.yml"));
    }

    // When XDG_CONFIG_HOME is set it is both gh's next branch and the right thing to name in a
    // trace if nothing is found; otherwise the branch gh would fall to on this platform.
    // `var_path` treats an empty value as unset, matching go-gh's
    // `os.Getenv(xdgConfigHome) != ""`.
    let xdg_path = env::var_path("XDG_CONFIG_HOME").map(|dir| dir.join("gh/hosts.yml"));
    let fallback = xdg_path.clone().unwrap_or_else(gh_default_hosts_path);

    let candidates = xdg_path
        .into_iter()
        .chain(gh_native_hosts_paths())
        .collect();
    Some(tokens::first_existing_file(candidates, fallback))
}

/// `%APPDATA%\GitHub CLI\hosts.yml`, or `None` when `APPDATA` is unset or empty.
///
/// go-gh guards that branch with `os.Getenv(appData) != ""` and otherwise falls through to
/// `~/.config/gh`, so the variable being absent is meaningful — synthesizing `~/AppData/Roaming`
/// here would send mise to a directory gh would never have used.
#[cfg(windows)]
fn gh_appdata_hosts_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(|v| PathBuf::from(v).join("GitHub CLI/hosts.yml"))
}

/// Where gh lands when neither `GH_CONFIG_DIR` nor `XDG_CONFIG_HOME` is set.
#[cfg(windows)]
fn gh_default_hosts_path() -> PathBuf {
    gh_appdata_hosts_path().unwrap_or_else(|| dirs::HOME.join(".config/gh/hosts.yml"))
}

#[cfg(not(windows))]
fn gh_default_hosts_path() -> PathBuf {
    dirs::HOME.join(".config/gh/hosts.yml")
}

/// Platform-native locations gh may have written to, probed after the XDG one.
#[cfg(target_os = "macos")]
fn gh_native_hosts_paths() -> Vec<PathBuf> {
    vec![dirs::HOME.join("Library/Application Support/gh/hosts.yml")]
}

#[cfg(windows)]
fn gh_native_hosts_paths() -> Vec<PathBuf> {
    gh_appdata_hosts_path().into_iter().collect()
}

#[cfg(all(not(target_os = "macos"), not(windows)))]
fn gh_native_hosts_paths() -> Vec<PathBuf> {
    Vec::new()
}

fn read_gh_hosts() -> Option<HashMap<String, String>> {
    let hosts_path = gh_hosts_path()?;
    let contents = match std::fs::read_to_string(&hosts_path) {
        Ok(c) => c,
        Err(e) => {
            trace!("gh hosts.yml not readable at {}: {e}", hosts_path.display());
            return None;
        }
    };
    let hosts: HashMap<String, GhHostEntry> = match serde_yaml::from_str(&contents) {
        Ok(h) => h,
        Err(e) => {
            debug!(
                "failed to parse gh hosts.yml at {}: {e}",
                hosts_path.display()
            );
            return None;
        }
    };
    Some(
        hosts
            .into_iter()
            .filter_map(|(host, entry)| entry.oauth_token.map(|token| (host, token)))
            .collect(),
    )
}

#[derive(Deserialize)]
struct GhHostEntry {
    oauth_token: Option<String>,
}

/// Serializes env-var mutations across every test module that touches GitHub token
/// environment variables. `github::tests` and `github::sigstore::tests` both mutate the same
/// four tokens (`MISE_GITHUB_TOKEN`, `GITHUB_API_TOKEN`, `GITHUB_TOKEN`,
/// `MISE_GITHUB_ENTERPRISE_TOKEN`); sharing a single lock prevents parallel test runs from
/// racing.
#[doc(hidden)]
pub static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[doc(hidden)]
pub mod test_support {
    //! Test-only hooks that let sibling modules seed non-env-var token sources without
    //! spinning up global configuration infrastructure. Compiled into every build, but
    //! `resolve_token` only consults them while [`crate::testing::in_tests`] is true, which
    //! never holds in a release binary.

    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Overrides the `github_tokens.toml` source in [`super::resolve_token`].
    /// Keyed by the same lookup hosts `resolve_token` walks — e.g. `"github.com"`.
    /// Hold [`super::TEST_ENV_LOCK`] while mutating; always clear before returning.
    pub static TOKENS_FILE_OVERRIDE: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

    pub fn lookup_tokens_file_override(lookup_hosts: &[&str]) -> Option<String> {
        let guard = TOKENS_FILE_OVERRIDE.read().ok()?;
        let map = guard.as_ref()?;
        for host in lookup_hosts {
            if let Some(token) = map.get(*host) {
                return Some(token.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
