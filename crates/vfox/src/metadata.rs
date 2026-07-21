use mlua::{FromLua, Lua, Table, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::error::Result;
use crate::error::VfoxError;

#[derive(Debug, Clone)]
pub struct Metadata {
    pub name: String,
    pub legacy_filenames: Vec<String>,
    pub depends: Vec<String>,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub system_dependencies: Vec<SystemDependency>,
    pub hooks: BTreeSet<&'static str>,
}

/// A single `PLUGIN.systemDependencies` entry — a system prerequisite the
/// plugin needs before it can install (build tools, libraries, ...). Exactly
/// one of `bin`/`pkgconfig`/`sharedlib`/`command` must be set; `packages` maps
/// a package-manager name (brew, apt, dnf, pacman, apk) to the package that
/// provides the capability, used only as a remediation hint.
#[derive(Debug, Clone, Default)]
pub struct SystemDependency {
    pub bin: Option<String>,
    pub pkgconfig: Option<String>,
    pub sharedlib: Option<String>,
    pub command: Option<String>,
    pub version: Option<String>,
    pub optional: Option<String>,
    pub packages: BTreeMap<String, String>,
}

impl FromLua for SystemDependency {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        let table = value
            .as_table()
            .ok_or_else(|| mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "SystemDependency".to_string(),
                message: Some("each systemDependencies entry must be a table".to_string()),
            })?;
        // Parse fields only. Validation (exactly one of bin/pkgconfig/
        // sharedlib/command) is left to the consumer so that a single bad
        // entry never fails the whole metadata parse — mise warns and skips
        // invalid entries while keeping the rest of the plugin usable.
        Ok(SystemDependency {
            bin: table.get("bin")?,
            pkgconfig: table.get("pkgconfig")?,
            sharedlib: table.get("sharedlib")?,
            command: table.get("command")?,
            version: table.get("version")?,
            optional: table.get("optional")?,
            packages: table
                .get::<Option<BTreeMap<String, String>>>("packages")?
                .unwrap_or_default(),
        })
    }
}

impl TryFrom<Table> for Metadata {
    type Error = VfoxError;
    fn try_from(t: Table) -> Result<Self> {
        let legacy_filenames = t
            .get::<Option<Vec<String>>>("legacyFilenames")?
            .unwrap_or_default();
        let depends = t.get::<Option<Vec<String>>>("depends")?.unwrap_or_default();
        // Never let a malformed systemDependencies field break the rest of the
        // metadata (e.g. `depends`, which install hooks rely on) — warn and
        // treat it as absent.
        let system_dependencies = match t.get::<Option<Vec<SystemDependency>>>("systemDependencies")
        {
            Ok(deps) => deps.unwrap_or_default(),
            Err(e) => {
                warn!("ignoring malformed systemDependencies in plugin metadata: {e}");
                vec![]
            }
        };
        Ok(Metadata {
            name: t.get("name")?,
            legacy_filenames,
            depends,
            version: t.get("version")?,
            description: t.get("description")?,
            author: t.get("author")?,
            license: t.get("license")?,
            homepage: t.get("homepage")?,
            system_dependencies,
            hooks: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_from_lua(code: &str) -> Metadata {
        let lua = Lua::new();
        lua.load(code).exec().unwrap();
        let table: Table = lua.globals().get("PLUGIN").unwrap();
        Metadata::try_from(table).unwrap()
    }

    fn metadata_result(code: &str) -> Result<Metadata> {
        let lua = Lua::new();
        lua.load(code).exec().unwrap();
        let table: Table = lua.globals().get("PLUGIN").unwrap();
        Metadata::try_from(table)
    }

    #[test]
    fn test_depends_parsed() {
        let m = metadata_from_lua(
            r#"
            PLUGIN = {
                name = "test",
                version = "1.0.0",
                depends = {"node", "python"},
            }
            "#,
        );
        assert_eq!(m.depends, vec!["node", "python"]);
    }

    #[test]
    fn test_depends_defaults_to_empty() {
        let m = metadata_from_lua(
            r#"
            PLUGIN = {
                name = "test",
                version = "1.0.0",
            }
            "#,
        );
        assert!(m.depends.is_empty());
    }

    #[test]
    fn test_system_dependencies_parsed() {
        let m = metadata_from_lua(
            r#"
            PLUGIN = {
                name = "test",
                version = "1.0.0",
                systemDependencies = {
                    { bin = "bison", version = ">=3.0",
                      packages = { brew = "bison", apt = "bison" } },
                    { pkgconfig = "libxml-2.0",
                      packages = { brew = "libxml2", apt = "libxml2-dev" } },
                    { sharedlib = "libaio.so.1" },
                    { command = "test -d /opt/thing", optional = "extra feature" },
                },
            }
            "#,
        );
        assert_eq!(m.system_dependencies.len(), 4);
        let bison = &m.system_dependencies[0];
        assert_eq!(bison.bin.as_deref(), Some("bison"));
        assert_eq!(bison.version.as_deref(), Some(">=3.0"));
        assert_eq!(
            bison.packages.get("brew").map(|s| s.as_str()),
            Some("bison")
        );
        assert_eq!(bison.packages.get("apt").map(|s| s.as_str()), Some("bison"));
        assert_eq!(
            m.system_dependencies[1].pkgconfig.as_deref(),
            Some("libxml-2.0")
        );
        assert_eq!(
            m.system_dependencies[2].sharedlib.as_deref(),
            Some("libaio.so.1")
        );
        assert_eq!(
            m.system_dependencies[3].optional.as_deref(),
            Some("extra feature")
        );
    }

    #[test]
    fn test_system_dependencies_defaults_to_empty() {
        let m = metadata_from_lua(
            r#"
            PLUGIN = { name = "test", version = "1.0.0" }
            "#,
        );
        assert!(m.system_dependencies.is_empty());
    }

    #[test]
    fn test_system_dependencies_parse_is_lenient() {
        // Field parsing does not validate "exactly one check key" — that is the
        // consumer's job (mise warns and skips), so a questionable entry must
        // not fail the whole metadata parse and drop `depends`.
        let m = metadata_result(
            r#"
            PLUGIN = {
                name = "test", version = "1.0.0",
                depends = { "node" },
                systemDependencies = {
                    { version = ">=3.0" },                       -- zero check keys
                    { bin = "bison", pkgconfig = "libxml-2.0" }, -- two check keys
                },
            }
            "#,
        )
        .unwrap();
        assert_eq!(m.depends, vec!["node"]);
        assert_eq!(m.system_dependencies.len(), 2);
    }

    #[test]
    fn test_malformed_system_dependencies_does_not_break_metadata() {
        // A grossly malformed field (not a list of tables) is ignored, and the
        // rest of the metadata (e.g. depends) still parses.
        let m = metadata_from_lua(
            r#"
            PLUGIN = {
                name = "test", version = "1.0.0",
                depends = { "node" },
                systemDependencies = "not a table",
            }
            "#,
        );
        assert_eq!(m.depends, vec!["node"]);
        assert!(m.system_dependencies.is_empty());
    }
}
