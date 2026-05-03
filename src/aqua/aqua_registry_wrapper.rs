use crate::backend::aqua::{arch, os};
use crate::config::Settings;
use crate::http::HTTP;
use crate::{dirs, duration::WEEKLY, file, hash};
use aqua_registry::{
    AquaRegistry, AquaRegistryConfig, AquaRegistryError, CompiledRegistry, NoOpCacheStore,
    RegistryFetcher,
};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use eyre::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
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

    pub async fn package_with_version(&self, id: &str, versions: &[&str]) -> Result<AquaPackage> {
        let pkg = self.package(id).await?;
        Ok(pkg.with_version(versions, os(), arch()))
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
        if let Some(registry) = self.compiled_registry().await? {
            match registry.package(package_id) {
                Ok(package) => {
                    log::trace!("reading aqua package for {package_id} from compiled registry");
                    return Ok(package);
                }
                Err(AquaRegistryError::PackageNotFound(_)) => {}
                Err(err) => return Err(err),
            }
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
                    .map_err(|err| err.to_string())
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
        let compiled_dir = self.config.cache_dir.join("compiled").join(source_hash);

        if let Ok(registry) = CompiledRegistry::load(&compiled_dir) {
            return Ok(Some(registry));
        }

        info!("compiling aqua registry from {registry_url}");
        let registry = CompiledRegistry::compile_from_yaml(&source, &compiled_dir)?;
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

fn write_registry_source(path: &std::path::Path, source: &str) -> aqua_registry::Result<()> {
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

#[derive(Debug, Deserialize)]
struct GithubContentResponse {
    content: String,
    encoding: String,
}

async fn download_github_registry_source(
    owner: &str,
    repo: &str,
    file_name: &str,
) -> aqua_registry::Result<String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{file_name}");
    let response = HTTP
        .json::<GithubContentResponse, _>(&url)
        .await
        .map_err(|err| {
            AquaRegistryError::RegistryNotAvailable(format!(
                "failed to download aqua registry source {url}: {err}"
            ))
        })?;

    if response.encoding != "base64" {
        return Err(AquaRegistryError::RegistryNotAvailable(format!(
            "unsupported GitHub content encoding for aqua registry source {url}: {}",
            response.encoding
        )));
    }

    let content = response.content.lines().collect::<String>();
    let bytes = BASE64_STANDARD.decode(content).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "failed to decode aqua registry source {url}: {err}"
        ))
    })?;
    String::from_utf8(bytes).map_err(|err| {
        AquaRegistryError::RegistryNotAvailable(format!(
            "aqua registry source {url} is not valid UTF-8: {err}"
        ))
    })
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
    AquaChecksum, AquaChecksumType, AquaMinisignType, AquaPackage, AquaPackageType,
};
