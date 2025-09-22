use std::cmp::Ordering;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use mlua::{AsChunk, FromLuaMulti, IntoLua, Lua, Table};
use once_cell::sync::OnceCell;

use crate::config::Config;
use crate::context::Context;
use crate::error::Result;
use crate::metadata::Metadata;
use crate::runtime::Runtime;
use crate::sdk_info::SdkInfo;
use crate::{config, error, lua_mod, VfoxError};

#[derive(Debug)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
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
        Ok(Self {
            name: dir.file_name().unwrap().to_string_lossy().to_string(),
            dir: dir.to_path_buf(),
            lua,
            metadata: OnceCell::new(),
        })
    }

    pub fn from_name(name: &str) -> Result<Self> {
        let dir = Config::get().plugin_dir.join(name);
        Self::from_dir(&dir)
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
            set_paths(
                &self.lua,
                &[
                    self.dir.join("?.lua"), //xx
                    self.dir.join("hooks/?.lua"),
                    self.dir.join("lib/?.lua"),
                ],
            )?;

            lua_mod::archiver(&self.lua)?;
            lua_mod::cmd(&self.lua)?;
            lua_mod::file(&self.lua)?;
            lua_mod::html(&self.lua)?;
            lua_mod::http(&self.lua)?;
            lua_mod::json(&self.lua)?;
            lua_mod::strings(&self.lua)?;
            lua_mod::env(&self.lua)?;

            let metadata = self.load_metadata()?;
            self.set_global("PLUGIN", metadata.clone())?;
            self.set_global("RUNTIME", Runtime::get(self.dir.clone()))?;
            self.set_global("OS_TYPE", config::os())?;
            self.set_global("ARCH_TYPE", config::arch())?;

            let mut metadata: Metadata = metadata.try_into()?;

            metadata.hooks = lua_mod::hooks(&self.lua, &self.dir)?;

            Ok(metadata)
        })
    }

    fn set_global<V>(&self, name: &str, value: V) -> Result<()>
    where
        V: IntoLua,
    {
        self.lua.globals().set(name, value)?;
        Ok(())
    }

    fn load_metadata(&self) -> Result<Table> {
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
