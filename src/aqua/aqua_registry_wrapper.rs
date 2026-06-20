use crate::config::Settings;
use crate::http::HTTP;
use crate::{dirs, duration};
use aqua_registry::{AquaRegistryError, CompiledRegistry, ParsedRegistry, RegistryCache};
use eyre::Result;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock as Lazy};
use tokio::sync::{Mutex, OnceCell};
use url::Url;

static AQUA_REGISTRY_PATH: Lazy<PathBuf> = Lazy::new(|| dirs::CACHE.join("aqua-registry"));
static AQUA_DEFAULT_REGISTRY_URL: &str = "https://github.com/aquaproj/aqua-registry";
pub(crate) const DEFAULT_AQUA_REGISTRY_CACHE_TTL: duration::Duration = duration::WEEKLY;

pub static AQUA_REGISTRY: Lazy<AquaRegistry> = Lazy::new(AquaRegistry::from_settings);

#[derive(Debug)]
pub struct AquaRegistry {
    registries: Vec<RegistrySource>,
}

impl AquaRegistry {
    fn from_settings() -> Self {
        let path = AQUA_REGISTRY_PATH.clone();
        let settings = Settings::get();
        let registry_urls = configured_registry_urls(&settings);

        Self::new(
            path,
            registry_urls,
            settings.aqua.baked_registry,
            settings.prefer_offline(),
            settings.aqua_registry_cache_ttl(),
        )
    }

    fn new(
        cache_dir: PathBuf,
        registry_urls: Vec<String>,
        use_baked_registry: bool,
        prefer_offline: bool,
        source_cache_ttl: duration::Duration,
    ) -> Self {
        let cache = RegistryCache::new(cache_dir);
        let mut registries = registry_urls
            .into_iter()
            .map(|registry_url| {
                RegistrySource::Downloaded(DownloadedRegistry::new(
                    registry_url,
                    cache.clone(),
                    prefer_offline,
                    source_cache_ttl,
                ))
            })
            .collect::<Vec<_>>();
        if use_baked_registry {
            registries.push(RegistrySource::Baked);
        }
        Self { registries }
    }
}

#[derive(Debug)]
enum RegistrySource {
    Downloaded(DownloadedRegistry),
    Baked,
}

impl RegistrySource {
    async fn package(&self, package_id: &str) -> aqua_registry::Result<Option<AquaPackage>> {
        match self {
            Self::Downloaded(registry) => match registry.package(package_id).await {
                Ok(package) => Ok(Some(package)),
                Err(AquaRegistryError::PackageNotFound(_)) => Ok(None),
                Err(err) => Err(err),
            },
            Self::Baked => super::standard_registry::package(package_id).transpose(),
        }
    }

    fn description(&self) -> &str {
        match self {
            Self::Downloaded(registry) => registry.registry_url.as_str(),
            Self::Baked => "baked-in aqua registry",
        }
    }
}

fn configured_registry_urls(settings: &Settings) -> Vec<String> {
    if let Some(registries) = settings.aqua.registries.clone() {
        registries
    } else if settings.aqua.baked_registry {
        Vec::new()
    } else {
        vec![AQUA_DEFAULT_REGISTRY_URL.into()]
    }
}

#[derive(Debug)]
struct DownloadedRegistry {
    registry_url: String,
    prefer_offline: bool,
    source_cache_ttl: duration::Duration,
    cache: RegistryCache,
    registry: OnceCell<std::result::Result<Arc<ActiveRegistry>, String>>,
}

impl DownloadedRegistry {
    fn new(
        registry_url: String,
        cache: RegistryCache,
        prefer_offline: bool,
        source_cache_ttl: duration::Duration,
    ) -> Self {
        Self {
            registry_url,
            prefer_offline,
            source_cache_ttl,
            cache,
            registry: OnceCell::new(),
        }
    }
}

#[derive(Debug)]
enum ActiveRegistry {
    Compiled(CompiledRegistry),
    Parsed(Arc<ParsedRegistry>),
}

impl ActiveRegistry {
    fn package(&self, package_id: &str) -> aqua_registry::Result<AquaPackage> {
        match self {
            Self::Compiled(registry) => registry.package(package_id),
            Self::Parsed(registry) => registry.package(package_id),
        }
    }
}

impl AquaRegistry {
    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        static CACHE: Lazy<Mutex<HashMap<String, AquaPackage>>> =
            Lazy::new(|| Mutex::new(HashMap::new()));

        if let Some(pkg) = CACHE.lock().await.get(id) {
            return Ok(pkg.clone());
        }

        let mut pkg = self.fetch_package(id).await?;
        pkg.setup_version_filter()?;
        CACHE.lock().await.insert(id.to_string(), pkg.clone());
        Ok(pkg)
    }

    async fn fetch_package(&self, package_id: &str) -> aqua_registry::Result<AquaPackage> {
        for registry in &self.registries {
            if let Some(package) = registry.package(package_id).await? {
                log::trace!(
                    "reading aqua package for {package_id} from {}",
                    registry.description()
                );
                return Ok(package);
            }
        }

        Err(AquaRegistryError::RegistryNotAvailable(format!(
            "no aqua-registry found for {package_id}"
        )))
    }
}

impl DownloadedRegistry {
    async fn package(&self, package_id: &str) -> aqua_registry::Result<AquaPackage> {
        let registry = self.registry().await?;
        registry.package(package_id)
    }

    async fn registry(&self) -> aqua_registry::Result<Arc<ActiveRegistry>> {
        let registry = self
            .registry
            .get_or_init(|| async { self.load_registry().await.map_err(|err| err.to_string()) })
            .await;
        registry
            .clone()
            .map_err(AquaRegistryError::RegistryNotAvailable)
    }

    async fn load_registry(&self) -> aqua_registry::Result<Arc<ActiveRegistry>> {
        let registry_url = self.registry_url.as_str();
        let source = self.registry_source(registry_url).await?;
        let source_hash = RegistryCache::source_hash(&source);

        if let Some(registry) = self
            .load_compiled_registry(registry_url, &source_hash)
            .await
        {
            spawn_stale_compiled_prune(
                self.cache.clone(),
                registry_url.to_string(),
                source_hash.clone(),
            );
            return Ok(Arc::new(ActiveRegistry::Compiled(registry)));
        }

        let registry = parse_registry_source(registry_url.to_string(), source).await?;
        spawn_compiled_registry_cache_writer(
            registry_url.to_string(),
            self.cache.clone(),
            source_hash,
            Arc::clone(&registry),
        );
        Ok(Arc::new(ActiveRegistry::Parsed(registry)))
    }

    async fn load_compiled_registry(
        &self,
        registry_url: &str,
        source_hash: &str,
    ) -> Option<CompiledRegistry> {
        let cache = self.cache.clone();
        let registry_url = registry_url.to_string();
        let cache_registry_url = registry_url.clone();
        let cache_source_hash = source_hash.to_string();
        match tokio::task::spawn_blocking(move || {
            cache.load_compiled(&cache_registry_url, &cache_source_hash)
        })
        .await
        {
            Ok(Ok(registry)) => Some(registry),
            Ok(Err(err)) => {
                log::debug!("compiled aqua registry cache miss for {registry_url}: {err}");
                None
            }
            Err(err) => {
                warn!("failed to load compiled aqua registry cache for {registry_url}: {err}");
                None
            }
        }
    }

    async fn registry_source(&self, registry_url: &str) -> aqua_registry::Result<String> {
        if Url::parse(registry_url).is_ok_and(|url| url.scheme() == "file") {
            return download_registry_source(registry_url).await;
        }

        if let Some(source) = self
            .cache
            .read_fresh_source(registry_url, self.source_cache_ttl)?
        {
            return Ok(source);
        }

        if self.prefer_offline {
            trace!("using cached aqua registry source due to prefer-offline mode");
            return self
                .cache
                .read_source(registry_url)
                .map_err(|err| {
                    AquaRegistryError::RegistryNotAvailable(format!(
                        "failed to read cached aqua registry source {} while prefer-offline mode is enabled: {err}",
                        self.cache.source_path(registry_url).display()
                    ))
                })?
                .ok_or_else(|| {
                    AquaRegistryError::RegistryNotAvailable(format!(
                        "failed to read cached aqua registry source {} while prefer-offline mode is enabled: cache file does not exist",
                        self.cache.source_path(registry_url).display()
                    ))
                });
        }

        let source = download_registry_source(registry_url).await?;
        self.cache.write_source(registry_url, &source)?;
        Ok(source)
    }
}

async fn download_registry_source(registry_url: &str) -> aqua_registry::Result<String> {
    let mut errors = Vec::new();
    let github_repo = github_repo_slug(registry_url);

    for file_name in ["registry.yaml", "registry.yml"] {
        let source = if let Some((owner, repo)) = github_repo.as_ref() {
            let url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{file_name}");
            let mut headers = HeaderMap::new();
            headers.insert(
                ACCEPT,
                HeaderValue::from_static("application/vnd.github.raw"),
            );
            HTTP.get_text_request(url.as_str())
                .headers(&headers)
                .send()
                .await
                .map_err(|err| {
                    AquaRegistryError::RegistryNotAvailable(format!(
                        "failed to download aqua registry source {url}: {err}"
                    ))
                })
        } else {
            match registry_file_url(registry_url, file_name) {
                Ok(url) => download_registry_url(url.as_str()).await,
                Err(err) => Err(err),
            }
        };

        match source {
            Ok(source) => return Ok(source),
            Err(err) => errors.push(err.to_string()),
        }
    }

    if github_repo.is_none() {
        match download_registry_url(registry_url).await {
            Ok(source) => return Ok(source),
            Err(err) => errors.push(err.to_string()),
        }
    }

    Err(AquaRegistryError::RegistryNotAvailable(format!(
        "failed to download aqua registry from {registry_url}: {}",
        errors.join("; ")
    )))
}

async fn download_registry_url(url: &str) -> aqua_registry::Result<String> {
    let parsed = Url::parse(url).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!("invalid aqua registry URL {url}: {err}"))
    })?;

    if parsed.scheme() == "file" {
        let path = parsed.to_file_path().map_err(|_| {
            AquaRegistryError::RegistryNotAvailable(format!("invalid aqua registry URL {url}"))
        })?;
        let path_display = path.display().to_string();
        return tokio::task::spawn_blocking(move || {
            std::fs::read_to_string(&path).map_err(|err| {
                AquaRegistryError::RegistryNotAvailable(format!(
                    "failed to read aqua registry source {path_display}: {err}"
                ))
            })
        })
        .await
        .map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to read aqua registry source on blocking worker: {err}"
            ))
        })?;
    }

    HTTP.get_text(parsed).await.map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to download aqua registry source {url}: {err}"
        ))
    })
}

fn registry_file_url(registry_url: &str, file_name: &str) -> aqua_registry::Result<Url> {
    let mut url = Url::parse(registry_url).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "invalid aqua registry URL {registry_url}: {err}"
        ))
    })?;
    let path = url.path().trim_end_matches('/');
    url.set_path(&format!("{path}/{file_name}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

async fn parse_registry_source(
    registry_url: String,
    source: String,
) -> aqua_registry::Result<Arc<ParsedRegistry>> {
    tokio::task::spawn_blocking(move || {
        info!("parsing aqua registry from {registry_url}");
        measure!("aqua_registry::parse_yaml", {
            ParsedRegistry::parse_yaml(&source).map(Arc::new)
        })
    })
    .await
    .map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to parse aqua registry on blocking worker: {err}"
        ))
    })?
}

fn spawn_stale_compiled_prune(cache: RegistryCache, registry_url: String, source_hash: String) {
    tokio::task::spawn_blocking(move || {
        cache.prune_stale_compiled(&registry_url, &source_hash);
    });
}

fn spawn_compiled_registry_cache_writer(
    registry_url: String,
    cache: RegistryCache,
    source_hash: String,
    registry: Arc<ParsedRegistry>,
) {
    tokio::task::spawn_blocking(move || {
        if cache.load_compiled(&registry_url, &source_hash).is_ok() {
            cache.prune_stale_compiled(&registry_url, &source_hash);
            return;
        }

        info!("writing compiled aqua registry cache for {registry_url}");
        if let Err(err) = measure!("aqua_registry::write_compiled_cache", {
            cache
                .write_compiled(&registry_url, &source_hash, registry.as_ref())
                .map(|_| ())
        }) {
            warn!("failed to write compiled aqua registry cache for {registry_url}: {err}");
        }
    });
}

fn github_repo_slug(registry_url: &str) -> Option<(String, String)> {
    let url = Url::parse(registry_url).ok()?;
    if url.scheme() != "https"
        || url.host_str()? != "github.com"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }

    let mut segments = url.path_segments()?;
    let owner = segments.next()?;
    let repo = segments.next()?.trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() || segments.any(|segment| !segment.is_empty()) {
        return None;
    }

    Some((owner.to_string(), repo.to_string()))
}

struct AquaSuggestionsCache {
    name_to_ids: HashMap<&'static str, Vec<&'static str>>,
    names: Vec<&'static str>,
}

static AQUA_SUGGESTIONS_CACHE: Lazy<AquaSuggestionsCache> = Lazy::new(|| {
    let ids = super::standard_registry::package_ids();
    let mut name_to_ids: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
    for id in ids {
        if let Some((_, name)) = id.rsplit_once('/') {
            name_to_ids.entry(name).or_default().push(id);
        }
    }
    let names = name_to_ids.keys().copied().collect();
    AquaSuggestionsCache { name_to_ids, names }
});

/// Search aqua packages by tool name, returning "owner/name" IDs
/// where the name part is similar to the query.
pub fn aqua_suggest(query: &str) -> Vec<String> {
    let cache = &*AQUA_SUGGESTIONS_CACHE;

    // Use a higher threshold (0.8) to avoid noisy suggestions
    let similar_names = xx::suggest::similar_n_with_threshold(query, &cache.names, 5, 0.8);

    // Map back to full IDs
    let mut results = Vec::new();
    for matched_name in &similar_names {
        if let Some(full_ids) = cache.name_to_ids.get(matched_name.as_str()) {
            for full_id in full_ids {
                results.push(full_id.to_string());
                if results.len() >= 5 {
                    return results;
                }
            }
        }
    }
    results
}

// Re-export types and static for compatibility
pub use aqua_registry::{
    AquaChecksum, AquaChecksumType, AquaCosign, AquaGithubArtifactAttestations, AquaMinisign,
    AquaMinisignType, AquaPackage, AquaPackageType,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn github_slug_only_handles_https_repo_urls() {
        assert_eq!(
            github_repo_slug("https://github.com/aquaproj/aqua-registry"),
            Some(("aquaproj".to_string(), "aqua-registry".to_string()))
        );
        assert_eq!(
            github_repo_slug("https://github.com/aquaproj/aqua-registry.git"),
            Some(("aquaproj".to_string(), "aqua-registry".to_string()))
        );
        assert_eq!(
            github_repo_slug("https://github.com/aquaproj/aqua-registry/"),
            Some(("aquaproj".to_string(), "aqua-registry".to_string()))
        );
        assert_eq!(
            github_repo_slug("http://github.com/aqua/aqua-registry"),
            None
        );
        assert_eq!(
            github_repo_slug("https://api.github.com/repos/aquaproj/aqua-registry"),
            None
        );
        assert_eq!(
            github_repo_slug("git@github.com:aquaproj/aqua-registry.git"),
            None
        );
        assert_eq!(
            github_repo_slug("https://github.com/aquaproj/aqua-registry?ref=main"),
            None
        );
    }

    #[test]
    fn registry_file_url_appends_registry_file_name() {
        assert_eq!(
            registry_file_url("https://example.com/aqua-registry/", "registry.yml")
                .unwrap()
                .as_str(),
            "https://example.com/aqua-registry/registry.yml"
        );
        assert_eq!(
            registry_file_url(
                "https://example.com/aqua-registry?ref=main",
                "registry.yaml"
            )
            .unwrap()
            .as_str(),
            "https://example.com/aqua-registry/registry.yaml"
        );
    }

    #[test]
    fn compiled_registry_cache_is_scoped_by_registry_url() {
        let cache = RegistryCache::new("/cache");
        let first = cache.compiled_dir("https://example.com/one", "source");
        let second = cache.compiled_dir("https://example.com/two", "source");

        assert_ne!(first.parent(), second.parent());
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("source")
        );
    }

    #[test]
    fn registries_setting_becomes_registry_urls() {
        let mut settings = Settings::default();
        settings.aqua.baked_registry = true;
        settings.aqua.registries = Some(vec![
            "https://example.com/first".to_string(),
            "https://example.com/second".to_string(),
        ]);

        assert_eq!(
            configured_registry_urls(&settings),
            vec![
                "https://example.com/first".to_string(),
                "https://example.com/second".to_string()
            ]
        );
    }

    #[test]
    fn baked_registry_disabled_without_config_uses_downloaded_official_registry() {
        let mut settings = Settings::default();
        settings.aqua.baked_registry = false;

        assert_eq!(
            configured_registry_urls(&settings),
            vec![AQUA_DEFAULT_REGISTRY_URL.to_string()]
        );
    }

    #[test]
    fn explicit_empty_registries_override_default_registry() {
        let mut settings = Settings::default();
        settings.aqua.baked_registry = false;
        settings.aqua.registries = Some(Vec::new());

        assert_eq!(configured_registry_urls(&settings), Vec::<String>::new());
    }

    #[tokio::test]
    async fn registry_load_failure_does_not_check_later_registries() {
        let temp = tempfile::tempdir().unwrap();
        let missing_registry = temp.path().join("missing-registry");
        let err = test_registry(
            temp.path().to_path_buf(),
            vec![file_registry_url(&missing_registry)],
            true,
        )
        .fetch_package("01mf02/jaq")
        .await
        .unwrap_err();

        assert!(matches!(err, AquaRegistryError::RegistryNotAvailable(_)));
    }

    #[tokio::test]
    async fn baked_registry_handles_downloaded_registry_package_miss() {
        let temp = tempfile::tempdir().unwrap();
        let registry_dir = temp.path().join("custom-registry");
        std::fs::create_dir(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.yml"),
            "packages:\n  - name: example/custom\n    repo_owner: example\n    repo_name: custom\n",
        )
        .unwrap();

        let package = test_registry(
            temp.path().to_path_buf(),
            vec![file_registry_url(&registry_dir)],
            true,
        )
        .fetch_package("01mf02/jaq")
        .await
        .unwrap();

        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
    }

    #[tokio::test]
    async fn downloaded_registry_miss_fails_when_baked_registry_disabled() {
        let temp = tempfile::tempdir().unwrap();
        let missing_registry = temp.path().join("missing-registry");

        let err = test_registry(
            temp.path().to_path_buf(),
            vec![file_registry_url(&missing_registry)],
            false,
        )
        .fetch_package("01mf02/jaq")
        .await
        .unwrap_err();

        assert!(matches!(err, AquaRegistryError::RegistryNotAvailable(_)));
    }

    #[tokio::test]
    async fn parses_bundled_registry_from_local_source() {
        let temp = tempfile::tempdir().unwrap();
        let registry_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/aqua-registry");
        let fetcher = test_registry(
            temp.path().to_path_buf(),
            vec![file_registry_url(&registry_dir)],
            false,
        );

        let registry = first_downloaded_registry(&fetcher)
            .load_registry()
            .await
            .unwrap();
        let package = registry.package("01mf02/jaq").unwrap();

        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
    }

    #[tokio::test]
    async fn same_source_hash_uses_existing_compiled_cache() {
        let temp = tempfile::tempdir().unwrap();
        let registry_dir = temp.path().join("custom-registry");
        std::fs::create_dir(&registry_dir).unwrap();
        let source = "packages:\n  - name: example/custom\n    url: https://example.com/custom\n";
        std::fs::write(registry_dir.join("registry.yml"), source).unwrap();
        let registry_url = file_registry_url(&registry_dir);
        let source_hash = RegistryCache::source_hash(source);
        let cache = RegistryCache::new(temp.path());
        let parsed = ParsedRegistry::parse_yaml(source).unwrap();
        cache
            .write_compiled(&registry_url, &source_hash, &parsed)
            .unwrap();

        let fetcher = test_registry(temp.path().to_path_buf(), vec![registry_url], false);
        let registry = first_downloaded_registry(&fetcher)
            .load_registry()
            .await
            .unwrap();

        assert!(matches!(registry.as_ref(), ActiveRegistry::Compiled(_)));
    }

    #[tokio::test]
    async fn local_registry_source_bypasses_download_cache() {
        let temp = tempfile::tempdir().unwrap();
        let registry_dir = temp.path().join("custom-registry");
        std::fs::create_dir(&registry_dir).unwrap();
        let registry_path = registry_dir.join("registry.yaml");
        std::fs::write(
            &registry_path,
            "packages:\n  - name: example/first\n    url: https://example.com/first\n",
        )
        .unwrap();

        let fetcher = test_registry(
            temp.path().join("cache"),
            vec![format!("file://{}", registry_dir.display())],
            false,
        );
        let registry = first_downloaded_registry(&fetcher);
        let first = registry
            .registry_source(registry.registry_url.as_str())
            .await
            .unwrap();

        std::fs::write(
            registry_path,
            "packages:\n  - name: example/second\n    url: https://example.com/second\n",
        )
        .unwrap();
        let second = registry
            .registry_source(registry.registry_url.as_str())
            .await
            .unwrap();

        assert!(first.contains("example/first"));
        assert!(second.contains("example/second"));
    }

    #[tokio::test]
    async fn direct_file_registry_source_is_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("registry.yaml");
        std::fs::write(
            &registry_path,
            "packages:\n  - name: example/direct\n    url: https://example.com/direct\n",
        )
        .unwrap();

        let fetcher = test_registry(
            temp.path().join("cache"),
            vec![file_registry_url(&registry_path)],
            false,
        );
        let registry = first_downloaded_registry(&fetcher);
        let source = registry
            .registry_source(registry.registry_url.as_str())
            .await
            .unwrap();

        assert!(source.contains("example/direct"));
    }

    #[tokio::test]
    async fn prefer_offline_missing_source_has_clear_error() {
        let temp = tempfile::tempdir().unwrap();
        let fetcher = AquaRegistry::new(
            temp.path().to_path_buf(),
            vec!["https://example.com/aqua-registry".to_string()],
            false,
            true,
            DEFAULT_AQUA_REGISTRY_CACHE_TTL,
        );

        let err = first_downloaded_registry(&fetcher)
            .registry_source("https://example.com/aqua-registry")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("prefer-offline mode is enabled"));
    }

    #[tokio::test]
    async fn registries_are_checked_in_order() {
        let temp = tempfile::tempdir().unwrap();
        let first_registry = write_registry(
            temp.path(),
            "first-registry",
            "packages:\n  - name: example/shared\n    repo_owner: example\n    repo_name: first\n",
        );
        let second_registry = write_registry(
            temp.path(),
            "second-registry",
            "packages:\n  - name: example/shared\n    repo_owner: example\n    repo_name: second\n",
        );

        let package = test_registry(
            temp.path().join("cache"),
            vec![
                file_registry_url(&first_registry),
                file_registry_url(&second_registry),
            ],
            false,
        )
        .fetch_package("example/shared")
        .await
        .unwrap();

        assert_eq!(package.repo_name, "first");
    }

    #[tokio::test]
    async fn registry_miss_checks_next_registry() {
        let temp = tempfile::tempdir().unwrap();
        let first_registry = write_registry(
            temp.path(),
            "first-registry",
            "packages:\n  - name: example/first\n    repo_owner: example\n    repo_name: first\n",
        );
        let second_registry = write_registry(
            temp.path(),
            "second-registry",
            "packages:\n  - name: example/second\n    repo_owner: example\n    repo_name: second\n",
        );

        let package = test_registry(
            temp.path().join("cache"),
            vec![
                file_registry_url(&first_registry),
                file_registry_url(&second_registry),
            ],
            false,
        )
        .fetch_package("example/second")
        .await
        .unwrap();

        assert_eq!(package.repo_name, "second");
    }

    #[tokio::test]
    async fn registry_aliases_are_registry_local() {
        let temp = tempfile::tempdir().unwrap();
        let first_registry = write_registry(
            temp.path(),
            "first-registry",
            r#"
packages:
  - name: example/canonical
    repo_owner: example
    repo_name: first
    aliases:
      - name: example/shared
"#,
        );
        let second_registry = write_registry(
            temp.path(),
            "second-registry",
            "packages:\n  - name: example/shared\n    repo_owner: example\n    repo_name: second\n",
        );

        let package = test_registry(
            temp.path().join("cache"),
            vec![
                file_registry_url(&first_registry),
                file_registry_url(&second_registry),
            ],
            true,
        )
        .fetch_package("example/shared")
        .await
        .unwrap();

        assert_eq!(package.name.as_deref(), Some("example/canonical"));
        assert_eq!(package.repo_name, "first");
    }

    fn test_registry(
        cache_dir: PathBuf,
        registry_urls: Vec<String>,
        use_baked_registry: bool,
    ) -> AquaRegistry {
        AquaRegistry::new(
            cache_dir,
            registry_urls,
            use_baked_registry,
            false,
            DEFAULT_AQUA_REGISTRY_CACHE_TTL,
        )
    }

    fn first_downloaded_registry(fetcher: &AquaRegistry) -> &DownloadedRegistry {
        match fetcher.registries.first().unwrap() {
            RegistrySource::Downloaded(registry) => registry,
            RegistrySource::Baked => panic!("expected downloaded registry"),
        }
    }

    fn write_registry(root: &Path, name: &str, source: &str) -> PathBuf {
        let registry_dir = root.join(name);
        std::fs::create_dir(&registry_dir).unwrap();
        std::fs::write(registry_dir.join("registry.yml"), source).unwrap();
        registry_dir
    }

    fn file_registry_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }
}
