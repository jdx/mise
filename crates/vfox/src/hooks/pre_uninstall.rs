use crate::Plugin;
use crate::error::Result;
use crate::sdk_info::SdkInfo;
use mlua::{IntoLua, Lua, Value};
use std::collections::BTreeMap;

impl Plugin {
    pub async fn pre_uninstall(&self, ctx: PreUninstallContext) -> Result<()> {
        debug!("[vfox:{}] pre_uninstall", &self.name);
        self.exec_async(chunk! {
            require "hooks/pre_uninstall"
            PLUGIN:PreUninstall($ctx)
        })
        .await
    }
}

pub struct PreUninstallContext {
    pub main: SdkInfo,
    pub sdk_info: BTreeMap<String, SdkInfo>,
}

impl IntoLua for PreUninstallContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("main", self.main)?;
        table.set("sdkInfo", self.sdk_info)?;
        Ok(Value::Table(table))
    }
}
