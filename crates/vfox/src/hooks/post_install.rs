use crate::Plugin;
use crate::error::Result;
use crate::sdk_info::SdkInfo;
use indexmap::IndexMap;
use mlua::{IntoLua, Lua, LuaSerdeExt, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

impl Plugin {
    pub async fn post_install(&self, ctx: PostInstallContext) -> Result<()> {
        debug!("[vfox:{}] post_install", self.name);
        self.exec_async(chunk! {
            require "hooks/post_install"
            PLUGIN:PostInstall($ctx)
        })
        .await
    }
}

pub struct PostInstallContext {
    pub root_path: PathBuf,
    pub runtime_version: String,
    pub sdk_info: BTreeMap<String, SdkInfo>,
    pub options: IndexMap<String, toml::Value>,
}

impl IntoLua for PostInstallContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("rootPath", self.root_path.to_string_lossy().to_string())?;
        table.set("runtimeVersion", self.runtime_version)?;
        table.set("sdkInfo", self.sdk_info)?;
        table.set("options", lua.to_value(&self.options)?)?;
        Ok(Value::Table(table))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Plugin, embedded_plugins};
    use mlua::{Lua, ObjectLike, Table};
    use tokio::test;

    use super::*;

    #[test]
    async fn dummy() {
        // A temp dir, not a relative path: the dummy plugin's PostInstall writes a VERSION file
        // and a bin/ directory under rootPath, which would otherwise land in the crate directory.
        //
        // The apostrophe is deliberate. PostInstall builds a shell command to create the
        // directory, so a path like this is what breaks naive quoting.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("O'Brien");
        let p = Plugin::test("dummy");
        let ctx = PostInstallContext {
            root_path: root.clone(),
            runtime_version: "runtime_version".to_string(),
            sdk_info: BTreeMap::new(),
            options: Default::default(),
        };
        p.post_install(ctx).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            "runtime_version"
        );
        assert!(root.join("bin").join("dummy").exists());
    }

    #[test]
    async fn embedded_chromedriver() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("O'Brien");
        std::fs::create_dir_all(&root).unwrap();
        let binary_name = if cfg!(windows) {
            "chromedriver.exe"
        } else {
            "chromedriver"
        };
        std::fs::write(root.join(binary_name), "chromedriver").unwrap();

        let embedded = embedded_plugins::get_embedded_plugin("chromedriver").unwrap();
        let plugin = Plugin::from_embedded("chromedriver", embedded).unwrap();
        let sdk_info = SdkInfo::new(
            "chromedriver".to_string(),
            "153.0.8009.0".to_string(),
            root.clone(),
        );
        let ctx = PostInstallContext {
            root_path: root.clone(),
            runtime_version: "153.0.8009.0".to_string(),
            sdk_info: BTreeMap::from([("chromedriver".to_string(), sdk_info)]),
            options: Default::default(),
        };

        plugin.post_install(ctx).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("bin").join(binary_name)).unwrap(),
            "chromedriver"
        );
    }

    #[test]
    async fn poetry_quotes_script_paths_and_removes_them_without_a_shell() {
        let poetry_hook = embedded_plugins::get_embedded_plugin("poetry")
            .unwrap()
            .hooks
            .iter()
            .find(|(name, _)| *name == "post_install")
            .unwrap()
            .1;

        for root_path in [
            "/tmp/poetry install",
            "/tmp/poetry; harmless-marker",
            "/tmp/poetry O'Brien",
        ] {
            let lua = Lua::new();
            lua.load(
                r#"
                    PLUGIN = {}
                    RUNTIME = { osType = "linux" }
                    commands = {}
                    removed_path = nil
                    io.open = function()
                        return { write = function() end, close = function() end }
                    end
                    os.execute = function(command)
                        table.insert(commands, command)
                        return 0
                    end
                    os.remove = function(path)
                        removed_path = path
                    end
                "#,
            )
            .exec()
            .unwrap();
            lua.load(poetry_hook).exec().unwrap();

            let ctx = lua.create_table().unwrap();
            ctx.set("rootPath", root_path).unwrap();
            let poetry = lua.create_table().unwrap();
            poetry.set("version", "2.0.0").unwrap();
            let sdk_info = lua.create_table().unwrap();
            sdk_info.set("poetry", poetry).unwrap();
            ctx.set("sdkInfo", sdk_info).unwrap();

            let plugin: Table = lua.globals().get("PLUGIN").unwrap();
            plugin.call_method::<()>("PostInstall", ctx).unwrap();

            let script_path = format!("{root_path}/install_poetry.sh");
            let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
            let commands: Table = lua.globals().get("commands").unwrap();
            assert_eq!(
                commands.get::<String>(1).unwrap(),
                format!(
                    "chmod +x {} && {}",
                    quote(&script_path),
                    quote(&script_path)
                )
            );
            assert_eq!(
                lua.globals().get::<String>("removed_path").unwrap(),
                script_path
            );
        }
    }
}
