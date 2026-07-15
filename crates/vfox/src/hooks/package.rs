use mlua::{FromLua, IntoLua, Lua, Value, prelude::LuaError};

use crate::{Plugin, error::Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRequest {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInstalledContext {
    pub packages: Vec<PackageRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageActionContext {
    pub packages: Vec<PackageRequest>,
    pub dry_run: bool,
    pub update: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub state: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInstalledResponse {
    pub packages: Vec<InstalledPackage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageActionResponse;

impl Plugin {
    pub async fn package_installed(
        &self,
        ctx: PackageInstalledContext,
    ) -> Result<PackageInstalledResponse> {
        debug!("[vfox:{}] package_installed", &self.name);
        self.eval_async(chunk! {
            require "hooks/package_installed"
            return PLUGIN:PackageInstalled($ctx)
        })
        .await
    }

    pub async fn package_install(
        &self,
        ctx: PackageActionContext,
    ) -> Result<PackageActionResponse> {
        debug!("[vfox:{}] package_install", &self.name);
        self.eval_async(chunk! {
            require "hooks/package_install"
            return PLUGIN:PackageInstall($ctx)
        })
        .await
    }

    pub async fn package_upgrade(
        &self,
        ctx: PackageActionContext,
    ) -> Result<PackageActionResponse> {
        debug!("[vfox:{}] package_upgrade", &self.name);
        self.eval_async(chunk! {
            require "hooks/package_upgrade"
            return PLUGIN:PackageUpgrade($ctx)
        })
        .await
    }
}

fn packages_into_lua(packages: Vec<PackageRequest>, lua: &Lua) -> mlua::Result<Value> {
    let list = lua.create_table()?;
    for (index, package) in packages.into_iter().enumerate() {
        let item = lua.create_table()?;
        item.set("name", package.name)?;
        item.set("version", package.version)?;
        list.set(index + 1, item)?;
    }
    Ok(Value::Table(list))
}

impl IntoLua for PackageInstalledContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("packages", packages_into_lua(self.packages, lua)?)?;
        Ok(Value::Table(table))
    }
}

impl IntoLua for PackageActionContext {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        table.set("packages", packages_into_lua(self.packages, lua)?)?;
        table.set("dry_run", self.dry_run)?;
        table.set("update", self.update)?;
        Ok(Value::Table(table))
    }
}

impl FromLua for PackageInstalledResponse {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        let Value::Table(table) = value else {
            return Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "PackageInstalledResponse".to_string(),
                message: Some("Expected table".to_string()),
            });
        };
        let packages = table
            .get::<mlua::Table>("packages")?
            .sequence_values::<mlua::Table>()
            .map(|item| {
                let item = item?;
                Ok(InstalledPackage {
                    name: item.get("name")?,
                    state: item.get("state")?,
                    version: item.get("version")?,
                })
            })
            .collect::<mlua::Result<Vec<_>>>()?;
        Ok(Self { packages })
    }
}

impl FromLua for PackageActionResponse {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        match value {
            Value::Table(_) | Value::Nil => Ok(Self),
            _ => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "PackageActionResponse".to_string(),
                message: Some("Expected table or nil".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_context_serializes_versions_as_opaque_strings() {
        let lua = Lua::new();
        let value = PackageActionContext {
            packages: vec![PackageRequest {
                name: "extension".into(),
                version: Some("nightly-2026.07".into()),
            }],
            dry_run: true,
            update: false,
        }
        .into_lua(&lua)
        .unwrap();
        let table = value.as_table().unwrap();
        let packages = table.get::<mlua::Table>("packages").unwrap();
        let package = packages.get::<mlua::Table>(1).unwrap();
        assert_eq!(package.get::<String>("version").unwrap(), "nightly-2026.07");
        assert!(table.get::<bool>("dry_run").unwrap());
    }

    #[test]
    fn installed_response_deserializes() {
        let lua = Lua::new();
        let value: Value = lua
            .load(r#"return { packages = {{ name = "diff", state = "installed", version = "1.3.4" }, { name = "s3", state = "missing" }}}"#)
            .eval()
            .unwrap();
        let response = PackageInstalledResponse::from_lua(value, &lua).unwrap();
        assert_eq!(response.packages[0].version.as_deref(), Some("1.3.4"));
        assert_eq!(response.packages[1].state, "missing");
    }
}
