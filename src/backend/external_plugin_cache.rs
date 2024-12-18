use crate::backend::asdf::AsdfBackend;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::config::Config;
use crate::dirs;
use crate::env_diff::EnvMap;
use crate::hash::hash_to_str;
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{ToolRequest, ToolVersion};
use eyre::{eyre, WrapErr};
use std::collections::HashMap;
use std::env;
use std::sync::RwLock;

#[derive(Debug, Default)]
pub struct ExternalPluginCache {
    list_bin_paths: RwLock<HashMap<ToolRequest, CacheManager<Vec<String>>>>,
    exec_env: RwLock<HashMap<ToolRequest, CacheManager<EnvMap>>>,
}

impl ExternalPluginCache {
    pub fn list_bin_paths<F>(
        &self,
        plugin: &AsdfBackend,
        tv: &ToolVersion,
        fetch: F,
    ) -> eyre::Result<Vec<String>>
    where
        F: FnOnce() -> eyre::Result<Vec<String>>,
    {
        let mut w = self.list_bin_paths.write().unwrap();
        let cm = w.entry(tv.request.clone()).or_insert_with(|| {
            let list_bin_paths_filename = match &plugin.toml.list_bin_paths.cache_key {
                Some(key) => {
                    let config = Config::get();
                    let key = render_cache_key(&config, tv, key);
                    let filename = format!("{}.msgpack.z", key);
                    tv.cache_path().join("list_bin_paths").join(filename)
                }
                None => tv.cache_path().join("list_bin_paths.msgpack.z"),
            };
            CacheManagerBuilder::new(list_bin_paths_filename)
                .with_fresh_file(plugin.plugin_path.clone())
                .with_fresh_file(tv.install_path())
                .build()
        });
        cm.get_or_try_init(fetch).cloned()
    }

    pub fn exec_env<F>(
        &self,
        config: &Config,
        plugin: &AsdfBackend,
        tv: &ToolVersion,
        fetch: F,
    ) -> eyre::Result<EnvMap>
    where
        F: FnOnce() -> eyre::Result<EnvMap>,
    {
        let mut w = self.exec_env.write().unwrap();
        let cm = w.entry(tv.request.clone()).or_insert_with(|| {
            let exec_env_filename = match &plugin.toml.exec_env.cache_key {
                Some(key) => {
                    let key = render_cache_key(config, tv, key);
                    let filename = format!("{}.msgpack.z", key);
                    tv.cache_path().join("exec_env").join(filename)
                }
                None => tv.cache_path().join("exec_env.msgpack.z"),
            };
            CacheManagerBuilder::new(exec_env_filename)
                .with_fresh_file(dirs::DATA.to_path_buf())
                .with_fresh_file(plugin.plugin_path.clone())
                .with_fresh_file(tv.install_path())
                .build()
        });
        cm.get_or_try_init(fetch).cloned()
    }
}

fn render_cache_key(config: &Config, tv: &ToolVersion, cache_key: &[String]) -> String {
    let elements = cache_key
        .iter()
        .map(|tmpl| {
            let s = parse_template(config, tv, tmpl).unwrap();
            let s = s.trim().to_string();
            trace!("cache key element: {} -> {}", tmpl.trim(), s);
            let mut s = hash_to_str(&s);
            s.truncate(10);
            s
        })
        .collect::<Vec<String>>();
    elements.join("-")
}

fn parse_template(config: &Config, tv: &ToolVersion, tmpl: &str) -> eyre::Result<String> {
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("project_root", &config.project_root);
    ctx.insert("opts", &tv.request.options().opts);
    get_tera(
        config
            .project_root
            .as_ref()
            .or(env::current_dir().as_ref().ok())
            .map(|p| p.as_path()),
    )
    .render_str(tmpl, &ctx)
    .wrap_err_with(|| eyre!("failed to parse template: {tmpl}"))
}
