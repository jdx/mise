use crate::config::Settings;
use crate::http::HTTP;
use crate::{dirs, duration::WEEKLY, file, hash};
use aqua_registry::{
    AquaRegistry, AquaRegistryConfig, AquaRegistryError, CompiledRegistry, NoOpCacheStore,
    RegistryFetcher,
};
use eyre::Result;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock as Lazy};
use tokio::sync::{Mutex, OnceCell};
use url::Url;

static AQUA_REGISTRY_PATH: Lazy<PathBuf> = Lazy::new(|| dirs::CACHE.join("aqua-registry"));
static AQUA_DEFAULT_REGISTRY_URL: &str = "https://github.com/aquaproj/aqua-registry";

pub static AQUA_REGISTRY: Lazy<MiseAquaRegistry> = Lazy::new(|| {
    MiseAquaRegistry::standard().unwrap_or_else(|err| {
        warn!("failed to initialize aqua registry: {err:?}");
        MiseAquaRegistry::default()
    })
});

/// Wrapper around the aqua-registry crate that provides mise-specific functionality
#[derive(Debug)]
pub struct MiseAquaRegistry {
    inner: AquaRegistry<MiseRegistryFetcher>,
    #[allow(dead_code)]
    path: PathBuf,
}

impl Default for MiseAquaRegistry {
    fn default() -> Self {
        let config = AquaRegistryConfig::default();
        let inner = aqua_registry(config.clone());
        Self {
            inner,
            path: config.cache_dir,
        }
    }
}

impl MiseAquaRegistry {
    pub fn standard() -> Result<Self> {
        let path = AQUA_REGISTRY_PATH.clone();
        let settings = Settings::get();
        let registry_url =
            settings
                .aqua
                .registry_url
                .as_deref()
                .or(if settings.aqua.baked_registry {
                    None
                } else {
                    Some(AQUA_DEFAULT_REGISTRY_URL)
                });

        let config = AquaRegistryConfig {
            cache_dir: path.clone(),
            registry_url: registry_url.map(|s| s.to_string()),
            use_baked_registry: settings.aqua.baked_registry,
            prefer_offline: settings.prefer_offline(),
        };

        let inner = aqua_registry(config);

        Ok(Self { inner, path })
    }

    pub async fn package(&self, id: &str) -> Result<AquaPackage> {
        static CACHE: Lazy<Mutex<HashMap<String, AquaPackage>>> =
            Lazy::new(|| Mutex::new(HashMap::new()));

        if let Some(pkg) = CACHE.lock().await.get(id) {
            return Ok(pkg.clone());
        }

        let pkg = self.inner.package(id).await?;
        CACHE.lock().await.insert(id.to_string(), pkg.clone());
        Ok(pkg)
    }
}

#[derive(Debug, Clone)]
struct MiseRegistryFetcher {
    config: AquaRegistryConfig,
    compiled_registry: Arc<OnceCell<std::result::Result<Option<CompiledRegistry>, String>>>,
}

fn aqua_registry(config: AquaRegistryConfig) -> AquaRegistry<MiseRegistryFetcher> {
    AquaRegistry::with_fetcher_and_cache(
        config.clone(),
        MiseRegistryFetcher {
            config,
            compiled_registry: Arc::new(OnceCell::new()),
        },
        NoOpCacheStore,
    )
}

impl RegistryFetcher for MiseRegistryFetcher {
    async fn fetch_package(&self, package_id: &str) -> aqua_registry::Result<AquaPackage> {
        match self.compiled_registry().await {
            Ok(Some(registry)) => match registry.package(package_id) {
                Ok(package) => {
                    log::trace!("reading aqua package for {package_id} from compiled registry");
                    return Ok(package);
                }
                Err(AquaRegistryError::PackageNotFound(_)) => {}
                Err(err) => return Err(err),
            },
            Ok(None) => {}
            Err(err) if self.config.use_baked_registry => {
                log::trace!(
                    "falling back to baked-in aqua registry after custom registry load failed: {err}"
                );
            }
            Err(err) => return Err(err),
        }

        if self.config.use_baked_registry
            && let Some(package) = super::standard_registry::package(package_id)
        {
            log::trace!("reading baked-in aqua package for {package_id}");
            return package;
        }

        Err(AquaRegistryError::RegistryNotAvailable(format!(
            "no aqua-registry found for {package_id}"
        )))
    }
}

impl MiseRegistryFetcher {
    async fn compiled_registry(&self) -> aqua_registry::Result<Option<CompiledRegistry>> {
        let registry = self
            .compiled_registry
            .get_or_init(|| async {
                self.load_compiled_registry()
                    .await
                    .map_err(|err| {
                        if self.config.use_baked_registry {
                            if let Some(registry_url) = self.config.registry_url.as_deref() {
                                warn!(
                                    "failed to load aqua registry from {registry_url}: {err}; falling back to baked-in aqua registry"
                                );
                            }
                        }
                        err.to_string()
                    })
            })
            .await;
        registry
            .clone()
            .map_err(AquaRegistryError::RegistryNotAvailable)
    }

    async fn load_compiled_registry(&self) -> aqua_registry::Result<Option<CompiledRegistry>> {
        let Some(registry_url) = self.config.registry_url.as_deref() else {
            return Ok(None);
        };

        let source = self.registry_source(registry_url).await?;
        let source_hash = hash::hash_blake3_to_str(&source);
        let compiled_dir =
            compiled_registry_cache_dir(&self.config.cache_dir, registry_url, &source_hash);

        if let Ok(registry) = CompiledRegistry::load(&compiled_dir) {
            prune_stale_compiled_registries(&compiled_dir);
            return Ok(Some(registry));
        }

        info!("compiling aqua registry from {registry_url}");
        let registry = CompiledRegistry::compile_from_yaml(&source, &compiled_dir)?;
        prune_stale_compiled_registries(&compiled_dir);
        Ok(Some(registry))
    }

    async fn registry_source(&self, registry_url: &str) -> aqua_registry::Result<String> {
        let source_path = registry_source_cache_path(&self.config.cache_dir, registry_url);

        if source_is_fresh(&source_path) {
            return Ok(std::fs::read_to_string(&source_path)?);
        }

        if self.config.prefer_offline {
            trace!("using cached aqua registry source due to prefer-offline mode");
            return std::fs::read_to_string(&source_path).map_err(Into::into);
        }

        let source = download_registry_source(registry_url).await?;
        write_registry_source(&source_path, &source)?;
        Ok(source)
    }
}

fn source_is_fresh(path: &std::path::Path) -> bool {
    path.exists() && file::modified_duration(path).is_ok_and(|duration| duration < WEEKLY)
}

fn registry_source_cache_path(cache_dir: &std::path::Path, registry_url: &str) -> PathBuf {
    cache_dir
        .join("sources")
        .join(format!("{}.yaml", hash::hash_to_str(&registry_url)))
}

fn compiled_registry_cache_dir(cache_dir: &Path, registry_url: &str, source_hash: &str) -> PathBuf {
    cache_dir
        .join("compiled")
        .join(hash::hash_to_str(&registry_url))
        .join(source_hash)
}

fn prune_stale_compiled_registries(current_dir: &Path) {
    let Some(parent) = current_dir.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path == current_dir {
            continue;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && let Err(err) = std::fs::remove_dir_all(&path)
        {
            debug!(
                "failed to prune stale compiled aqua registry cache {}: {err}",
                path.display()
            );
        }
    }
}

fn write_registry_source(path: &Path, source: &str) -> aqua_registry::Result<()> {
    if let Ok(existing) = std::fs::read_to_string(path)
        && existing == source
    {
        file::touch_file(path).map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to touch cached aqua registry source {}: {err}",
                path.display()
            ))
        })?;
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, source)?;
    Ok(())
}

async fn download_registry_source(registry_url: &str) -> aqua_registry::Result<String> {
    let mut errors = Vec::new();
    for file_name in ["registry.yaml", "registry.yml"] {
        match download_registry_source_file(registry_url, file_name).await {
            Ok(source) => return Ok(source),
            Err(err) => errors.push(err.to_string()),
        }
    }

    Err(AquaRegistryError::RegistryNotAvailable(format!(
        "failed to download aqua registry from {registry_url}: {}",
        errors.join("; ")
    )))
}

async fn download_registry_source_file(
    registry_url: &str,
    file_name: &str,
) -> aqua_registry::Result<String> {
    if let Some(path) = local_registry_source_path(registry_url, file_name) {
        return std::fs::read_to_string(&path).map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to read aqua registry source {}: {err}",
                path.display()
            ))
        });
    }

    if let Some((owner, repo)) = github_repo_slug(registry_url) {
        return download_github_registry_source(&owner, &repo, file_name).await;
    }

    let url = registry_file_url(registry_url, file_name)?;
    HTTP.get_text(url.as_str()).await.map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to download aqua registry source {url}: {err}"
        ))
    })
}

fn local_registry_source_path(registry_url: &str, file_name: &str) -> Option<PathBuf> {
    if let Ok(url) = Url::parse(registry_url)
        && url.scheme() == "file"
    {
        return url.to_file_path().ok().map(|path| path.join(file_name));
    }

    if registry_url.contains("://") || registry_url.starts_with("git@") {
        return None;
    }

    Some(PathBuf::from(registry_url).join(file_name))
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

fn github_repo_slug(registry_url: &str) -> Option<(String, String)> {
    if let Some(rest) = registry_url.strip_prefix("git@github.com:") {
        let (owner, repo) = rest.split_once('/')?;
        return Some((owner.to_string(), repo.trim_end_matches(".git").to_string()));
    }

    let url = Url::parse(registry_url).ok()?;
    match url.host_str()? {
        "github.com" => {
            let mut segments = url.path_segments()?;
            let owner = segments.next()?.to_string();
            let repo = segments.next()?.trim_end_matches(".git").to_string();
            if owner.is_empty() || repo.is_empty() {
                None
            } else {
                Some((owner, repo))
            }
        }
        "api.github.com" => {
            let mut segments = url.path_segments()?;
            if segments.next()? != "repos" {
                return None;
            }
            let owner = segments.next()?.to_string();
            let repo = segments.next()?.trim_end_matches(".git").to_string();
            if owner.is_empty() || repo.is_empty() {
                None
            } else {
                Some((owner, repo))
            }
        }
        _ => None,
    }
}

async fn download_github_registry_source(
    owner: &str,
    repo: &str,
    file_name: &str,
) -> aqua_registry::Result<String> {
    let url = github_registry_file_url(owner, repo, file_name);
    HTTP.get_text_with_headers(url.as_str(), &github_raw_contents_headers())
        .await
        .map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to download aqua registry source {url}: {err}"
            ))
        })
}

fn github_registry_file_url(owner: &str, repo: &str, file_name: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{repo}/contents/{file_name}")
}

fn github_raw_contents_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github.raw"),
    );
    headers
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
    use std::path::PathBuf;

    #[test]
    fn github_slug_handles_common_registry_urls() {
        assert_eq!(
            github_repo_slug("https://github.com/aquaproj/aqua-registry"),
            Some(("aquaproj".to_string(), "aqua-registry".to_string()))
        );
        assert_eq!(
            github_repo_slug("https://api.github.com/repos/aquaproj/aqua-registry"),
            Some(("aquaproj".to_string(), "aqua-registry".to_string()))
        );
        assert_eq!(
            github_repo_slug("git@github.com:aquaproj/aqua-registry.git"),
            Some(("aquaproj".to_string(), "aqua-registry".to_string()))
        );
    }

    #[test]
    fn github_registry_download_uses_raw_contents_api_without_json_size_limit() {
        assert_eq!(
            github_registry_file_url("aquaproj", "aqua-registry", "registry.yaml"),
            "https://api.github.com/repos/aquaproj/aqua-registry/contents/registry.yaml"
        );
        assert_eq!(
            github_raw_contents_headers()
                .get(ACCEPT)
                .and_then(|value| value.to_str().ok()),
            Some("application/vnd.github.raw")
        );
    }

    #[test]
    fn compiled_registry_cache_is_scoped_by_registry_url() {
        let cache_dir = Path::new("/cache");
        let first = compiled_registry_cache_dir(cache_dir, "https://example.com/one", "source");
        let second = compiled_registry_cache_dir(cache_dir, "https://example.com/two", "source");

        assert_ne!(first.parent(), second.parent());
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("source")
        );
    }

    #[tokio::test]
    async fn baked_registry_fallback_survives_custom_registry_load_failure() {
        let temp = tempfile::tempdir().unwrap();
        let missing_registry = temp.path().join("missing-registry");
        let fetcher = test_fetcher(
            temp.path().to_path_buf(),
            Some(missing_registry.display().to_string()),
            true,
        );

        let package = fetcher.fetch_package("01mf02/jaq").await.unwrap();

        assert_eq!(package.repo_owner, "01mf02");
        assert_eq!(package.repo_name, "jaq");
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

        let package = test_fetcher(
            temp.path().to_path_buf(),
            Some(registry_dir.display().to_string()),
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

        let err = test_fetcher(
            temp.path().to_path_buf(),
            Some(missing_registry.display().to_string()),
            false,
        )
        .fetch_package("01mf02/jaq")
        .await
        .unwrap_err();

        assert!(matches!(err, AquaRegistryError::RegistryNotAvailable(_)));
    }

    fn test_fetcher(
        cache_dir: PathBuf,
        registry_url: Option<String>,
        use_baked_registry: bool,
    ) -> MiseRegistryFetcher {
        MiseRegistryFetcher {
            config: AquaRegistryConfig {
                cache_dir,
                registry_url,
                use_baked_registry,
                prefer_offline: false,
            },
            compiled_registry: Arc::new(OnceCell::new()),
        }
    }
}
