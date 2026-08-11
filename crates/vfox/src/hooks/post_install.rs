use crate::Plugin;
use crate::error::Result;
use crate::sdk_info::SdkInfo;
use mlua::{IntoLua, Lua, Value};
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
        };
        p.post_install(ctx).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            "runtime_version"
        );
        assert!(root.join("bin").join("dummy").exists());
    }
}
