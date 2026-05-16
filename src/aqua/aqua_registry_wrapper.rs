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
    registry_url: Option<String>,
    use_baked_registry: bool,
    prefer_offline: bool,
    source_cache_ttl: duration::Duration,
    cache: RegistryCache,
    registry: Arc<OnceCell<std::result::Result<Option<Arc<ActiveRegistry>>, String>>>,
}

impl AquaRegistry {
    fn from_settings() -> Self {
        let path = AQUA_REGISTRY_PATH.clone();
        let settings = Settings::get();
        let registry_url =
            settings.aqua.registry_url.clone().or_else(|| {
                (!settings.aqua.baked_registry).then(|| AQUA_DEFAULT_REGISTRY_URL.into())
            });

        Self::new(
            path,
            registry_url,
            settings.aqua.baked_registry,
            settings.prefer_offline(),
            settings.aqua_registry_cache_ttl(),
        )
    }

    fn new(
        cache_dir: PathBuf,
        registry_url: Option<String>,
        use_baked_registry: bool,
        prefer_offline: bool,
        source_cache_ttl: duration::Duration,
    ) -> Self {
        Self {
            registry_url,
            use_baked_registry,
            prefer_offline,
            source_cache_ttl,
            cache: RegistryCache::new(cache_dir),
            registry: Arc::new(OnceCell::new()),
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
        match self.registry().await {
            Ok(Some(registry)) => match registry.package(package_id) {
                Ok(package) => {
                    log::trace!("reading aqua package for {package_id} from custom registry");
                    return Ok(package);
                }
                Err(AquaRegistryError::PackageNotFound(_)) => {}
                Err(err) => return Err(err),
            },
            Ok(None) => {}
            Err(err) => return Err(err),
        }

        if self.use_baked_registry
            && let Some(package) = super::standard_registry::package(package_id)
        {
            log::trace!("reading baked-in aqua package for {package_id}");
            return package;
        }

        Err(AquaRegistryError::RegistryNotAvailable(format!(
            "no aqua-registry found for {package_id}"
        )))
    }

    async fn registry(&self) -> aqua_registry::Result<Option<Arc<ActiveRegistry>>> {
        let registry = self
            .registry
            .get_or_init(|| async { self.load_registry().await.map_err(|err| err.to_string()) })
            .await;
        registry
            .clone()
            .map_err(AquaRegistryError::RegistryNotAvailable)
    }

    async fn load_registry(&self) -> aqua_registry::Result<Option<Arc<ActiveRegistry>>> {
        let Some(registry_url) = self.registry_url.as_deref() else {
            return Ok(None);
        };

        let source = self.registry_source(registry_url).await?;
        let source_hash = RegistryCache::source_hash(&source);

        if let Ok(registry) = self.cache.load_compiled(registry_url, &source_hash) {
            self.cache.prune_stale_compiled(registry_url, &source_hash);
            return Ok(Some(Arc::new(ActiveRegistry::Compiled(registry))));
        }

        info!("parsing aqua registry from {registry_url}");
        let registry = Arc::new(measure!("aqua_registry::parse_yaml", {
            ParsedRegistry::parse_yaml(&source)
        })?);
        let registry_url = registry_url.to_string();
        let cache = self.cache.clone();
        let registry_for_cache = Arc::clone(&registry);
        tokio::task::spawn_blocking(move || {
            if cache.load_compiled(&registry_url, &source_hash).is_ok() {
                cache.prune_stale_compiled(&registry_url, &source_hash);
                return;
            }

            info!("writing compiled aqua registry cache for {registry_url}");
            if let Err(err) = measure!("aqua_registry::write_compiled_cache", {
                cache
                    .write_compiled(&registry_url, &source_hash, registry_for_cache.as_ref())
                    .map(|_| ())
            }) {
                warn!("failed to write compiled aqua registry cache for {registry_url}: {err}");
            }
        });
        Ok(Some(Arc::new(ActiveRegistry::Parsed(registry))))
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
            HTTP.get_text_with_headers(url.as_str(), &headers)
                .await
                .map_err(|err| {
                    AquaRegistryError::RegistryNotAvailable(format!(
                        "failed to download aqua registry source {url}: {err}"
                    ))
                })
        } else {
            download_registry_url(&format!("{registry_url}/{file_name}")).await
        };

        match source {
            Ok(source) => return Ok(source),
            Err(err) => errors.push(err.to_string()),
        }
    }

    match download_registry_url(registry_url).await {
        Ok(source) => return Ok(source),
        Err(err) => errors.push(err.to_string()),
    }

    Err(AquaRegistryError::RegistryNotAvailable(format!(
        "failed to download aqua registry from {registry_url}: {}",
        errors.join("; ")
    )))
}

async fn download_registry_url(url: &str) -> aqua_registry::Result<String> {
    if let Ok(parsed) = Url::parse(url)
        && parsed.scheme() == "file"
    {
        let path = parsed.to_file_path().map_err(|_| {
            AquaRegistryError::RegistryNotAvailable(format!("invalid aqua registry URL {url}"))
        })?;
        return std::fs::read_to_string(&path).map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to read aqua registry source {}: {err}",
                path.display()
            ))
        });
    }

    HTTP.get_text(url).await.map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to download aqua registry source {url}: {err}"
        ))
    })
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
    if owner.is_empty() || repo.is_empty() || segments.next().is_some() {
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
    AquaChecksum, AquaChecksumType, AquaCosign, AquaMinisignType, AquaPackage, AquaPackageType,
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

    #[tokio::test]
    async fn custom_registry_load_failure_does_not_fall_back_to_baked_registry() {
        let temp = tempfile::tempdir().unwrap();
        let missing_registry = temp.path().join("missing-registry");
        let err = test_registry(
            temp.path().to_path_buf(),
            Some(file_registry_url(&missing_registry)),
            true,
        )
        .fetch_package("01mf02/jaq")
        .await
        .unwrap_err();

        assert!(matches!(err, AquaRegistryError::RegistryNotAvailable(_)));
    }

    #[tokio::test]
    async fn baked_registry_fallback_handles_custom_registry_package_miss() {
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
            Some(file_registry_url(&registry_dir)),
            true,
        )
        .fetch_package("01mf02/jaq")
        .await
        .unwrap();

        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
    }

    #[tokio::test]
    async fn custom_registry_does_not_fall_back_when_baked_registry_disabled() {
        let temp = tempfile::tempdir().unwrap();
        let missing_registry = temp.path().join("missing-registry");

        let err = test_registry(
            temp.path().to_path_buf(),
            Some(file_registry_url(&missing_registry)),
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
            Some(file_registry_url(&registry_dir)),
            false,
        );

        let registry = fetcher.load_registry().await.unwrap().unwrap();
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

        let registry = test_registry(temp.path().to_path_buf(), Some(registry_url), false)
            .load_registry()
            .await
            .unwrap()
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
            Some(format!("file://{}", registry_dir.display())),
            false,
        );
        let first = fetcher
            .registry_source(fetcher.registry_url.as_deref().unwrap())
            .await
            .unwrap();

        std::fs::write(
            registry_path,
            "packages:\n  - name: example/second\n    url: https://example.com/second\n",
        )
        .unwrap();
        let second = fetcher
            .registry_source(fetcher.registry_url.as_deref().unwrap())
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
            Some(file_registry_url(&registry_path)),
            false,
        );
        let source = fetcher
            .registry_source(fetcher.registry_url.as_deref().unwrap())
            .await
            .unwrap();

        assert!(source.contains("example/direct"));
    }

    #[tokio::test]
    async fn prefer_offline_missing_source_has_clear_error() {
        let temp = tempfile::tempdir().unwrap();
        let mut fetcher = test_registry(
            temp.path().to_path_buf(),
            Some("https://example.com/aqua-registry".to_string()),
            false,
        );
        fetcher.prefer_offline = true;

        let err = fetcher
            .registry_source(fetcher.registry_url.as_deref().unwrap())
            .await
            .unwrap_err();

        assert!(err.to_string().contains("prefer-offline mode is enabled"));
    }

    fn test_registry(
        cache_dir: PathBuf,
        registry_url: Option<String>,
        use_baked_registry: bool,
    ) -> AquaRegistry {
        AquaRegistry::new(
            cache_dir,
            registry_url,
            use_baked_registry,
            false,
            DEFAULT_AQUA_REGISTRY_CACHE_TTL,
        )
    }

    fn file_registry_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }
}
