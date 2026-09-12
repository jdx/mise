use std::path::Path;

use heck::ToKebabCase;
use itertools::Itertools;

use crate::cache::CacheManagerBuilder;
use crate::config::Settings;
use crate::plugins::PluginType;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::registry::{REGISTRY, RegistryTool, tool_enabled};
use crate::toolset::install_state;
use crate::{dirs, timeout};

#[derive(Debug, Clone)]
pub(crate) enum ToolCatalogSource {
    Registry(&'static RegistryTool),
    VfoxBackend,
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
            ToolCatalogSource::VfoxBackend => &self.id,
        }
    }

    pub(crate) fn selector_description(&self) -> &str {
        match &self.source {
            ToolCatalogSource::Registry(tool) => tool
                .description
                .or_else(|| tool.backends().first().copied())
                .unwrap_or_default(),
            ToolCatalogSource::VfoxBackend => self.description.as_deref().unwrap_or_default(),
        }
    }
}

pub(crate) async fn list() -> Vec<ToolCatalogEntry> {
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
    for (plugin_name, plugin_type) in plugins.iter() {
        if *plugin_type != PluginType::VfoxBackend
            || settings.disable_backends.contains(plugin_name)
        {
            continue;
        }
        let plugin_path = dirs::PLUGINS.join(plugin_name.to_kebab_case());
        if !plugin_path.exists() || !plugin_path.join("hooks/backend_list_tools.lua").exists() {
            continue;
        }
        let tools = cached_backend_tools(plugin_name, &plugin_path).await;
        entries.extend(tools.into_iter().filter_map(|tool| {
            let name = tool.name.trim();
            if !valid_tool_name(name) {
                debug!("ignoring invalid tool name from backend plugin {plugin_name}: {name:?}");
                return None;
            }
            let id = format!("{plugin_name}:{name}");
            if !tool_enabled(enable_tools.as_ref(), &disable_tools, &id) {
                return None;
            }
            Some(ToolCatalogEntry {
                id,
                name: name.to_string(),
                description: tool
                    .description
                    .filter(|description| !description.is_empty()),
                source: ToolCatalogSource::VfoxBackend,
            })
        }));
    }

    entries
        .into_iter()
        .unique_by(|entry| entry.id.clone())
        .collect()
}

async fn cached_backend_tools(plugin_name: &str, plugin_path: &Path) -> Vec<vfox::BackendTool> {
    let cache = CacheManagerBuilder::new(
        dirs::CACHE
            .join(plugin_name.to_kebab_case())
            .join("backend_tools.msgpack.z"),
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
}
