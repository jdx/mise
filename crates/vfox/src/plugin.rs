use std::cmp::Ordering;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use mlua::{AsChunk, FromLuaMulti, IntoLua, Lua, Table, Value};
use once_cell::sync::OnceCell;

use crate::config::Config;
use crate::context::Context;
use crate::embedded_plugins::{self, EmbeddedPlugin};
use crate::error::Result;
use crate::metadata::Metadata;
use crate::runtime::Runtime;
use crate::sdk_info::SdkInfo;
use crate::{VfoxError, config, error, lua_mod};

#[derive(Debug)]
pub enum PluginSource {
    Filesystem(PathBuf),
    Embedded(&'static EmbeddedPlugin),
}

#[derive(Debug)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
    source: PluginSource,
    lua: Lua,
    metadata: OnceCell<Metadata>,
}

impl Plugin {
    pub fn from_dir(dir: &Path) -> Result<Self> {
        if !dir.exists() {
            error!("Plugin directory not found: {:?}", dir);
        }
        let lua = Lua::new();
        lua.set_named_registry_value("plugin_dir", dir.to_path_buf())?;
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        lua.set_named_registry_value("plugin_name", name.clone())?;
        Ok(Self {
            name,
            dir: dir.to_path_buf(),
            source: PluginSource::Filesystem(dir.to_path_buf()),
            lua,
            metadata: OnceCell::new(),
        })
    }

    pub fn from_embedded(name: &str, embedded: &'static EmbeddedPlugin) -> Result<Self> {
        let lua = Lua::new();
        // Use a dummy path for embedded plugins
        let dummy_dir = PathBuf::from(format!("embedded:{}", name));
        lua.set_named_registry_value("plugin_dir", dummy_dir.clone())?;
        lua.set_named_registry_value("embedded_plugin", true)?;
        lua.set_named_registry_value("plugin_name", name.to_string())?;
        Ok(Self {
            name: name.to_string(),
            dir: dummy_dir,
            source: PluginSource::Embedded(embedded),
            lua,
            metadata: OnceCell::new(),
        })
    }

    pub fn from_name(name: &str) -> Result<Self> {
        // Check filesystem first - allows user to override embedded plugins
        let dir = Config::get().plugin_dir.join(name);
        if dir.exists() {
            return Self::from_dir(&dir);
        }
        // Fall back to embedded plugin if available
        if let Some(embedded) = embedded_plugins::get_embedded_plugin(name) {
            return Self::from_embedded(name, embedded);
        }
        Self::from_dir(&dir)
    }

    pub fn from_name_or_dir(name: &str, dir: &Path) -> Result<Self> {
        // Check filesystem first - allows user to override embedded plugins
        if dir.exists() {
            return Self::from_dir(dir);
        }
        // Fall back to embedded plugin if available
        if let Some(embedded) = embedded_plugins::get_embedded_plugin(name) {
            return Self::from_embedded(name, embedded);
        }
        Self::from_dir(dir)
    }

    pub fn is_embedded(&self) -> bool {
        matches!(self.source, PluginSource::Embedded(_))
    }

    /// Store an environment map in the Lua registry for use by cmd.exec().
    /// This allows env module hooks to run commands that find mise-managed tools on PATH.
    pub fn set_cmd_env(&self, env: &indexmap::IndexMap<String, String>) -> Result<()> {
        let table = self.lua.create_table()?;
        for (k, v) in env {
            table.set(k.as_str(), v.as_str())?;
        }
        self.lua.set_named_registry_value("mise_env", table)?;
        Ok(())
    }

    pub fn list() -> Result<Vec<String>> {
        let config = Config::get();
        if !config.plugin_dir.exists() {
            return Ok(vec![]);
        }
        let plugins = xx::file::ls(&config.plugin_dir)?;
        let plugins = plugins
            .iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|f| f.to_str())
                    .map(|s| s.to_string())
            })
            .collect();
        Ok(plugins)
    }

    pub fn get_metadata(&self) -> Result<Metadata> {
        Ok(self.load()?.clone())
    }

    pub fn sdk_info(&self, version: String, install_dir: PathBuf) -> Result<SdkInfo> {
        Ok(SdkInfo::new(
            self.get_metadata()?.name.clone(),
            version,
            install_dir,
        ))
    }

    #[cfg(test)]
    pub(crate) fn test(name: &str) -> Self {
        let dir = PathBuf::from("plugins").join(name);
        Self::from_dir(&dir).unwrap()
    }

    pub(crate) fn context(&self, version: Option<String>) -> Result<Context> {
        let ctx = Context {
            args: vec![],
            version,
            // version: "1.0.0".to_string(),
            // runtime_version: "xxx".to_string(),
        };
        Ok(ctx)
    }

    pub(crate) async fn exec_async(&self, chunk: impl AsChunk) -> Result<()> {
        self.load()?;
        let chunk = self.lua.load(chunk);
        chunk.exec_async().await?;
        Ok(())
    }

    pub(crate) async fn eval_async<R>(&self, chunk: impl AsChunk) -> Result<R>
    where
        R: FromLuaMulti,
    {
        self.load()?;
        let chunk = self.lua.load(chunk);
        let result = chunk.eval_async().await?;
        Ok(result)
    }

    // Backend plugin methods
    fn load(&self) -> Result<&Metadata> {
        self.metadata.get_or_try_init(|| {
            debug!("[vfox] Getting metadata for {self}");

            // For filesystem plugins, set Lua package paths
            if let PluginSource::Filesystem(dir) = &self.source {
                set_paths(
                    &self.lua,
                    &[
                        dir.join("?.lua"),
                        dir.join("hooks/?.lua"),
                        dir.join("lib/?.lua"),
                    ],
                )?;
            }

            // Load standard Lua modules (http, json, etc.) FIRST
            // These must be available before loading embedded lib files
            lua_mod::archiver(&self.lua)?;
            lua_mod::cmd(&self.lua)?;
            lua_mod::file(&self.lua)?;
            lua_mod::html(&self.lua)?;
            lua_mod::http(&self.lua)?;
            lua_mod::json(&self.lua)?;
            lua_mod::semver(&self.lua)?;
            lua_mod::strings(&self.lua)?;
            lua_mod::env(&self.lua)?;
            lua_mod::log(&self.lua)?;

            // For embedded plugins, load lib modules AFTER standard modules
            // (lib files may require http, json, etc.)
            if let PluginSource::Embedded(embedded) = &self.source {
                self.load_embedded_libs(embedded)?;
            }

            let metadata = self.load_metadata()?;
            self.set_global("PLUGIN", metadata.clone())?;
            self.set_global("RUNTIME", Runtime::get(self.dir.clone()))?;
            self.set_global("OS_TYPE", config::os())?;
            self.set_global("ARCH_TYPE", config::arch())?;

            let mut metadata: Metadata = metadata.try_into()?;

            metadata.hooks = match &self.source {
                PluginSource::Filesystem(dir) => lua_mod::hooks(&self.lua, dir)?,
                PluginSource::Embedded(embedded) => lua_mod::hooks_embedded(&self.lua, embedded)?,
            };

            Ok(metadata)
        })
    }

    fn load_embedded_libs(&self, embedded: &EmbeddedPlugin) -> Result<()> {
        let package: Table = self.lua.globals().get("package")?;
        let preload: Table = package.get("preload")?;

        // Register lib modules in package.preload so require() works regardless of load order
        // This allows lib files to require each other without alphabetical ordering issues
        for (name, code) in embedded.lib {
            let lua = self.lua.clone();
            let code = *code;
            let loader = lua.create_function(move |lua, _: ()| {
                let module: Value = lua.load(code).eval()?;
                Ok(module)
            })?;
            preload.set(*name, loader)?;
        }

        Ok(())
    }

    fn set_global<V>(&self, name: &str, value: V) -> Result<()>
    where
        V: IntoLua,
    {
        self.lua.globals().set(name, value)?;
        Ok(())
    }

    fn load_metadata(&self) -> Result<Table> {
        match &self.source {
            PluginSource::Filesystem(_) => {
                let metadata = self
                    .lua
                    .load(
                        r#"
                        require "metadata"
                        return PLUGIN
                    "#,
                    )
                    .eval()?;
                Ok(metadata)
            }
            PluginSource::Embedded(embedded) => {
                // Load metadata from embedded string
                self.lua.load(embedded.metadata).exec()?;
                let metadata = self.lua.globals().get("PLUGIN")?;
                Ok(metadata)
            }
        }
    }
}

fn get_package(lua: &Lua) -> Result<Table> {
    let package = lua.globals().get::<Table>("package")?;
    Ok(package)
}

fn set_paths(lua: &Lua, paths: &[PathBuf]) -> Result<()> {
    let paths = paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<String>>()
        .join(";");

    get_package(lua)?.set("path", paths)?;

    Ok(())
}

impl Display for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl PartialEq<Self> for Plugin {
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir
    }
}

impl Eq for Plugin {}

impl PartialOrd for Plugin {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Plugin {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}
