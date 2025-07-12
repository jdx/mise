use crate::error::Result;
use crate::sdk_info::SdkInfo;
use crate::Plugin;
use mlua::{IntoLua, Lua, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

impl Plugin {
    pub async fn post_install(&self, ctx: PostInstallContext) -> Result<()> {
        debug!("[vfox:{}] post_install", &self.name);
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
}

impl IntoLua for PostInstallContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("rootPath", self.root_path.to_string_lossy().to_string())?;
        table.set("runtimeVersion", self.runtime_version)?;
        table.set("sdkInfo", self.sdk_info)?;
        Ok(Value::Table(table))
    }
}

#[cfg(test)]
mod tests {
    use crate::Plugin;
    use tokio::test;

    use super::*;

    #[test]
    async fn dummy() {
        let p = Plugin::test("dummy");
        let ctx = PostInstallContext {
            root_path: PathBuf::from("root_path"),
            runtime_version: "runtime_version".to_string(),
            sdk_info: BTreeMap::new(),
        };
        p.post_install(ctx).await.unwrap();
    }
}
