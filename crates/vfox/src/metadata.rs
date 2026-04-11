use mlua::Table;
use std::collections::BTreeSet;

use crate::error::Result;
use crate::error::VfoxError;

#[cfg(test)]
use mlua::Lua;

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
    pub hooks: BTreeSet<&'static str>,
}

impl TryFrom<Table> for Metadata {
    type Error = VfoxError;
    fn try_from(t: Table) -> Result<Self> {
        let legacy_filenames = t
            .get::<Option<Vec<String>>>("legacyFilenames")?
            .unwrap_or_default();
        let depends = t
            .get::<Option<Vec<String>>>("depends")?
            .unwrap_or_default();
        Ok(Metadata {
            name: t.get("name")?,
            legacy_filenames,
            depends,
            version: t.get("version")?,
            description: t.get("description")?,
            author: t.get("author")?,
            license: t.get("license")?,
            homepage: t.get("homepage")?,
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
}
