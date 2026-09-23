use std::path::Path;

use futures_util::{StreamExt, future, stream};
use itertools::Itertools;

use crate::backend::backend_type::BackendType;
use crate::backend::{cargo, dotnet, gem, npm_registry};
use crate::cache::CacheManagerBuilder;
use crate::config::Settings;
use crate::plugins::PluginType;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::registry::{REGISTRY, RegistryTool, tool_enabled};
use crate::toolset::install_state;
use crate::{dirs, timeout};

const BACKEND_CATALOG_CONCURRENCY: usize = 8;
/// Built-in backends whose package registry has a search API.
const PACKAGE_REGISTRY_BACKENDS: &[BackendType] = &[
    BackendType::Cargo,
    BackendType::Dotnet,
    BackendType::Gem,
    BackendType::Npm,
];
const SEARCH_LIMIT: usize = 20;

#[derive(Debug, Clone)]
pub(crate) enum ToolCatalogSource {
    Registry(&'static RegistryTool),
    Backend,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolCatalogEntry {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source: ToolCatalogSource,
}

impl ToolCatalogEntry {
    pub(crate) fn canonical_id(&self) -> &str {
        match &self.source {
            ToolCatalogSource::Registry(tool) => tool.short,
            ToolCatalogSource::Backend => &self.id,
        }
    }

    pub(crate) fn selector_description(&self) -> &str {
        match &self.source {
            ToolCatalogSource::Registry(tool) => tool
                .description
                .or_else(|| tool.backends().first().copied())
                .unwrap_or_default(),
            ToolCatalogSource::Backend => self.description.as_deref().unwrap_or_default(),
        }
    }

    pub(crate) fn selectable(&self) -> bool {
        match &self.source {
            ToolCatalogSource::Registry(tool) => !tool.backends().is_empty(),
            ToolCatalogSource::Backend => true,
        }
    }
}

pub(crate) async fn search(query: &str) -> Vec<ToolCatalogEntry> {
    let settings = Settings::get();
    let enable_tools = settings.enable_tools();
    let disable_tools = settings.disable_tools();
    let mut entries = REGISTRY
        .iter()
        .filter(|(short, _)| {
            tool_enabled(enable_tools.as_ref(), &disable_tools, &short.to_string())
        })
        .map(|(short, tool)| ToolCatalogEntry {
            id: short.to_string(),
            name: short.to_string(),
            description: tool.description.map(str::to_string),
            source: ToolCatalogSource::Registry(tool),
        })
        .collect_vec();

    let Some(plugins) = install_state::try_list_plugins() else {
        return entries;
    };
    let backend_catalogs = plugins
        .iter()
        .filter(|(plugin_name, plugin_type)| {
            **plugin_type == PluginType::VfoxBackend
                && !settings.disable_backends.contains(*plugin_name)
        })
        .filter_map(|(plugin_name, _)| {
            if !backend_query_targets_plugin(plugin_name, query) {
                return None;
            }
            let plugin_path = dirs::PLUGINS.join(plugin_name);
            let has_list = plugin_path.join("hooks/backend_list_tools.lua").exists();
            let search_query = backend_search_query(plugin_name, query);
            let has_search = search_query.is_some()
                && plugin_path.join("hooks/backend_search_tools.lua").exists();
            (has_list || has_search).then_some(async move {
                let list_tools = async {
                    if has_list {
                        cached_backend_list_tools(plugin_name, &plugin_path).await
                    } else {
                        vec![]
                    }
                };
                let search_tools = async {
                    if has_search {
                        cached_backend_search_tools(
                            plugin_name,
                            &plugin_path,
                            search_query.unwrap(),
                        )
                        .await
                    } else {
                        vec![]
                    }
                };
                let (mut tools, search_tools) = future::join(list_tools, search_tools).await;
                tools.extend(search_tools);
                (plugin_name, tools)
            })
        });
    let backend_catalogs = stream::iter(backend_catalogs)
        .buffered(BACKEND_CATALOG_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (plugin_name, tools) in backend_catalogs {
        entries.extend(backend_entries(plugin_name, tools));
    }

    entries
        .into_iter()
        .unique_by(|entry| entry.id.clone())
        .collect()
}

/// Searches a built-in backend's package registry for a `backend:query`
/// search, e.g. `npm:prettier`. Unprefixed queries return nothing, so plain
/// searches and shell completion stay offline.
pub(crate) async fn search_package_registry(query: &str) -> Vec<ToolCatalogEntry> {
    let Some((backend, query)) = query.split_once(':') else {
        return vec![];
    };
    let settings = Settings::get();
    let backend_type = BackendType::guess(backend);
    if query.is_empty()
        || !PACKAGE_REGISTRY_BACKENDS.contains(&backend_type)
        || settings.offline()
        || settings.disable_backends.iter().any(|b| b == backend)
        || (backend_type.is_experimental() && !settings.experimental)
    {
        return vec![];
    }
    let cache = CacheManagerBuilder::new(
        dirs::CACHE
            .join("package-registry-search")
            .join(format!("{backend}.msgpack.z")),
    )
    .with_cache_key(query.to_string())
    .with_fresh_duration(settings.fetch_remote_versions_cache())
    .build();
    let result = cache
        .get_or_try_init_async(|| async {
            timeout::run_with_timeout_async(
                || async {
                    match backend_type {
                        BackendType::Cargo => cargo::search_tools(query, SEARCH_LIMIT).await,
                        BackendType::Dotnet => dotnet::search_tools(query, SEARCH_LIMIT).await,
                        BackendType::Gem => gem::search_tools(query, SEARCH_LIMIT).await,
                        BackendType::Npm => npm_registry::search_tools(query, SEARCH_LIMIT).await,
                        _ => Ok(vec![]),
                    }
                },
                settings.fetch_remote_versions_timeout(),
            )
            .await
        })
        .await;
    let tools = match result {
        Ok(tools) => tools.clone(),
        Err(err) => {
            warn!("failed to search {backend} packages for {query}: {err:#}");
            return vec![];
        }
    };
    backend_entries(backend, tools).collect()
}

fn backend_entries(
    backend: &str,
    tools: Vec<vfox::BackendTool>,
) -> impl Iterator<Item = ToolCatalogEntry> + '_ {
    let settings = Settings::get();
    let enable_tools = settings.enable_tools();
    let disable_tools = settings.disable_tools();
    tools.into_iter().filter_map(move |tool| {
        let name = tool.name.trim();
        if !valid_tool_name(name) {
            debug!("ignoring invalid tool name from backend {backend}: {name:?}");
            return None;
        }
        let id = format!("{backend}:{name}");
        if !tool_enabled(enable_tools.as_ref(), &disable_tools, &id) {
            return None;
        }
        Some(ToolCatalogEntry {
            id,
            name: name.to_string(),
            // Registry descriptions can span several lines, which would break
            // table rows and completion output.
            description: tool
                .description
                .map(|description| description.split_whitespace().join(" "))
                .filter(|description| !description.is_empty()),
            source: ToolCatalogSource::Backend,
        })
    })
}

fn backend_query_targets_plugin(plugin_name: &str, query: &str) -> bool {
    query
        .split_once(':')
        .is_none_or(|(prefix, _)| prefix == plugin_name)
}

fn backend_search_query<'a>(plugin_name: &str, query: &'a str) -> Option<&'a str> {
    if query.is_empty() {
        None
    } else if let Some((prefix, query)) = query.split_once(':') {
        (prefix == plugin_name && !query.is_empty()).then_some(query)
    } else {
        Some(query)
    }
}

async fn cached_backend_list_tools(
    plugin_name: &str,
    plugin_path: &Path,
) -> Vec<vfox::BackendTool> {
    let cache = CacheManagerBuilder::new(
        dirs::CACHE
            .join(plugin_name)
            .join("backend_list_tools.msgpack.z"),
    )
    .with_cache_key(plugin_name.to_string())
    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
    .with_fresh_file(plugin_path.to_path_buf())
    .with_fresh_file(plugin_path.join("hooks/backend_list_tools.lua"))
    .build();
    let plugin = VfoxPlugin::new(plugin_name.to_string(), plugin_path.to_path_buf());
    match cache
        .get_or_try_init_async(|| async {
            timeout::run_with_timeout_async(
                || async { Ok(plugin.backend_list_tools().await?.unwrap_or_default()) },
                Settings::get().fetch_remote_versions_timeout(),
            )
            .await
        })
        .await
    {
        Ok(tools) => tools.clone(),
        Err(err) => match cache.get_cached() {
            Ok(tools) => {
                debug!(
                    "failed to refresh tool catalog from backend plugin {plugin_name}, using stale cache: {err:#}"
                );
                tools
            }
            Err(_) => {
                debug!("failed to list tools from backend plugin {plugin_name}: {err:#}");
                vec![]
            }
        },
    }
}

async fn cached_backend_search_tools(
    plugin_name: &str,
    plugin_path: &Path,
    query: &str,
) -> Vec<vfox::BackendTool> {
    let cache = CacheManagerBuilder::new(
        dirs::CACHE
            .join(plugin_name)
            .join("backend_search_tools.msgpack.z"),
    )
    .with_cache_key(plugin_name.to_string())
    .with_cache_key(query.to_string())
    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
    .with_fresh_file(plugin_path.to_path_buf())
    .with_fresh_file(plugin_path.join("hooks/backend_search_tools.lua"))
    .build();
    let plugin = VfoxPlugin::new(plugin_name.to_string(), plugin_path.to_path_buf());
    match cache
        .get_or_try_init_async(|| async {
            timeout::run_with_timeout_async(
                || async {
                    Ok(plugin
                        .backend_search_tools(query.to_string())
                        .await?
                        .unwrap_or_default())
                },
                Settings::get().fetch_remote_versions_timeout(),
            )
            .await
        })
        .await
    {
        Ok(tools) => tools.clone(),
        Err(err) => match cache.get_cached() {
            Ok(tools) => {
                debug!(
                    "failed to search tool catalog from backend plugin {plugin_name}, using stale cache: {err:#}"
                );
                tools
            }
            Err(_) => {
                debug!("failed to search tools from backend plugin {plugin_name}: {err:#}");
                vec![]
            }
        },
    }
}

fn valid_tool_name(name: &str) -> bool {
    let valid_at = !name.contains('@')
        || name
            .strip_prefix('@')
            .is_some_and(|scoped| !scoped.contains('@'));
    !name.is_empty()
        && valid_at
        && !name.chars().any(char::is_whitespace)
        && !name.contains([':', '[', ']'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_tool_name() {
        assert!(valid_tool_name("prettier"));
        assert!(valid_tool_name("@scope/tool"));
        assert!(!valid_tool_name(""));
        assert!(!valid_tool_name("other:tool"));
        assert!(!valid_tool_name("two tools"));
        assert!(!valid_tool_name("tool[option=true]"));
        assert!(!valid_tool_name("tool@version"));
        assert!(!valid_tool_name("@scope/tool@version"));
    }

    #[test]
    fn test_backend_search_query() {
        assert_eq!(backend_search_query("npm", "react"), Some("react"));
        assert_eq!(backend_search_query("npm", "npm:react"), Some("react"));
        assert_eq!(backend_search_query("npm", "npm:"), None);
        assert_eq!(backend_search_query("npm", "cargo:react"), None);
        assert_eq!(backend_search_query("npm", ""), None);
    }

    #[tokio::test]
    async fn test_search_package_registry_skips_without_network() {
        // None of these reach a package registry, so they must return
        // immediately with no results.
        for query in ["", "prettier", "npm:", "pipx:black", "github:jdx/mise"] {
            assert!(search_package_registry(query).await.is_empty(), "{query}");
        }
    }

    #[test]
    fn test_backend_entries_normalizes_descriptions() {
        let tools = vec![
            vfox::BackendTool {
                name: "rubocop".into(),
                description: Some("A linter.\n  It formats too.".into()),
            },
            vfox::BackendTool {
                name: "blank".into(),
                description: Some(" \n".into()),
            },
            vfox::BackendTool {
                name: "bad name".into(),
                description: None,
            },
        ];
        let entries = backend_entries("gem", tools).collect_vec();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "gem:rubocop");
        assert_eq!(
            entries[0].description.as_deref(),
            Some("A linter. It formats too.")
        );
        assert_eq!(entries[1].description, None);
    }

    #[test]
    fn test_backend_query_targets_plugin() {
        assert!(backend_query_targets_plugin("npm", ""));
        assert!(backend_query_targets_plugin("npm", "react"));
        assert!(backend_query_targets_plugin("npm", "npm:"));
        assert!(backend_query_targets_plugin("npm", "npm:react"));
        assert!(!backend_query_targets_plugin("npm", "cargo:react"));
    }
}
