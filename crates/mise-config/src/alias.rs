//! `[tool_alias]` and the deprecated `[alias]`: shorthands for a tool's backend and versions.

use std::borrow::Cow;
use std::fmt::Formatter;

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

use crate::deprecation::{Deprecation, report};

/// A `[tool_alias]` entry: where the alias points, and version aliases for that tool.
#[derive(Debug, Clone, Default)]
pub struct Alias {
    pub backend: Option<String>,
    pub versions: IndexMap<String, String>,
}

impl<'de> Deserialize<'de> for Alias {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AliasVisitor;

        impl<'de> Visitor<'de> for AliasVisitor {
            type Value = Alias;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("alias")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Alias {
                    backend: Some(v.to_string()),
                    ..Default::default()
                })
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut backend = None;
                let mut versions = IndexMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "backend" => {
                            backend = Some(map.next_value()?);
                        }
                        "versions" => {
                            versions = map.next_value()?;
                        }
                        _ => {
                            report(Deprecation::ToolVersionAliases);
                            versions.insert(key, map.next_value()?);
                        }
                    }
                }
                Ok(Alias { backend, versions })
            }
        }

        deserializer.deserialize_any(AliasVisitor)
    }
}

// Written by hand next to the parser: an entry is a backend string or a table, and the table
// still takes the deprecated flat `<from> = <to>` spelling.
impl JsonSchema for Alias {
    fn schema_name() -> Cow<'static, str> {
        "tool_alias_entry".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "the backend this tool name points to, or a table with its backend and version aliases",
            "oneOf": [
                {"description": "where the alias goes", "type": "string"},
                {
                    "description": "tool to set aliases for",
                    "type": "object",
                    "properties": {
                        "backend": {"description": "where the alias goes", "type": "string"},
                        "versions": {"$ref": "#/$defs/tool_alias_versions"}
                    },
                    "additionalProperties": {"description": "version alias points to", "type": "string"}
                }
            ]
        })
    }
}

/// `$defs/tool_alias_versions`, shared by every alias entry.
pub(crate) fn versions_schema() -> Schema {
    json_schema!({
        "description": "tool version aliases",
        "type": "object",
        "additionalProperties": {"description": "version alias points to", "type": "string"}
    })
}
