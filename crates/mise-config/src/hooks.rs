//! `[hooks]`: commands, scripts and tasks run on `cd`, on entering or leaving a project, and
//! around tool installs.

mod decl;
pub use decl::*;

use std::fmt::Formatter;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

/// The event a hook runs on.
#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    Deserialize,
    strum::Display,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Hooks {
    Enter,
    Leave,
    Cd,
    Preinstall,
    Postinstall,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum HookScripts {
    One(String),
    Many(Vec<String>),
}

impl HookScripts {
    pub fn into_script(self) -> String {
        match self {
            Self::One(script) => script,
            Self::Many(scripts) => scripts.join("\n"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum HookDef {
    /// Array of hook definitions: `enter = ["echo hello", { task = "setup" }]`
    Array(Vec<HookDefItem>),
    One(HookDefItem),
}

impl HookDef {
    /// This definition followed by the hooks of `later`, so both run.
    pub fn then(self, later: HookDef) -> HookDef {
        let mut all = self.into_items();
        all.extend(later.into_items());
        HookDef::Array(all)
    }

    pub fn into_items(self) -> Vec<HookDefItem> {
        match self {
            HookDef::Array(items) => items,
            HookDef::One(item) => vec![item],
        }
    }
}

// Written by hand rather than `#[serde(untagged)]`, so a hook that matches no shape says which
// shapes there are, and a mistyped key in a hook table is named.
impl<'de> Deserialize<'de> for HookDef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HookDefVisitor;

        impl<'de> Visitor<'de> for HookDefVisitor {
            type Value = HookDef;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str(concat!(
                    "a command string, a table with `run`, `script`, `scripts` or `task`, ",
                    "or a list of those"
                ))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(HookDef::One(HookDefItem::RunString(v.to_string())))
            }

            fn visit_seq<S: de::SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut items = vec![];
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(HookDef::Array(items))
            }

            fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                HookTable::deserialize(de::value::MapAccessDeserializer::new(map))?
                    .into_item()
                    .map(HookDef::One)
            }
        }

        deserializer.deserialize_any(HookDefVisitor)
    }
}

#[derive(Debug, Clone)]
pub enum HookDefItem {
    /// Simple run string: `enter = "echo hello"`
    RunString(String),
    /// Table with run: `enter = { run = "echo hello" }`
    Run(HookRunTable),
    /// Table with script and optional shell: `enter = { script = "echo hello", shell = "bash" }`
    ScriptTable {
        script: HookScripts,
        shell: Option<String>,
    },
    /// Table with scripts and optional shell: `enter = { scripts = ["echo hello"], shell = "bash" }`
    ScriptsTable {
        scripts: Vec<String>,
        shell: Option<String>,
    },
    /// Task reference: `enter = { task = "setup" }`
    TaskRef { task: String },
}

impl<'de> Deserialize<'de> for HookDefItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HookDefItemVisitor;

        impl<'de> Visitor<'de> for HookDefItemVisitor {
            type Value = HookDefItem;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str(
                    "a command string or a table with `run`, `script`, `scripts` or `task`",
                )
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(HookDefItem::RunString(v.to_string()))
            }

            fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                HookTable::deserialize(de::value::MapAccessDeserializer::new(map))?.into_item()
            }
        }

        deserializer.deserialize_any(HookDefItemVisitor)
    }
}

/// Every key a hook table may hold. Which of them are set decides the [`HookDefItem`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HookTable {
    run: Option<String>,
    run_windows: Option<String>,
    shell: Option<String>,
    script: Option<HookScripts>,
    scripts: Option<Vec<String>>,
    task: Option<String>,
}

impl HookTable {
    fn into_item<E: de::Error>(self) -> Result<HookDefItem, E> {
        let runs = self.run.is_some() || self.run_windows.is_some();
        if let Some(task) = self.task {
            if runs || self.shell.is_some() || self.script.is_some() || self.scripts.is_some() {
                return Err(E::custom("a hook with `task` cannot set any other key"));
            }
            return Ok(HookDefItem::TaskRef { task });
        }
        match (self.script, self.scripts) {
            (Some(_), Some(_)) => Err(E::custom("a hook sets `script` or `scripts`, not both")),
            (Some(_), None) | (None, Some(_)) if runs => Err(E::custom(
                "a hook sets `run` or `script`/`scripts`, not both",
            )),
            (Some(script), None) => Ok(HookDefItem::ScriptTable {
                script,
                shell: self.shell,
            }),
            (None, Some(scripts)) => Ok(HookDefItem::ScriptsTable {
                scripts,
                shell: self.shell,
            }),
            (None, None) if runs => Ok(HookDefItem::Run(HookRunTable {
                run: self.run,
                run_windows: self.run_windows,
                shell: self.shell,
            })),
            (None, None) => Err(E::custom(
                "hook run table must define `run` or `run_windows`",
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookRunTable {
    pub run: Option<String>,
    pub run_windows: Option<String>,
    pub shell: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::{JsonSchema, schema_for};
    use std::collections::BTreeSet;

    fn keys<T: JsonSchema>() -> BTreeSet<String> {
        schema_for!(T)
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default()
    }

    // The schema is generated from the `Hook*` declaration types, and the parser reads
    // `HookTable`. A key added to one without the other fails here.
    #[test]
    fn the_declared_hook_keys_are_the_keys_the_parser_reads() {
        let declared: BTreeSet<String> = [
            keys::<HookRunTableDecl>(),
            keys::<HookCurrentShellScriptTable>(),
            keys::<HookCurrentShellScriptsTable>(),
            keys::<HookSpawnedScriptTable>(),
            keys::<HookSpawnedScriptsTable>(),
            keys::<HookInstallScriptTable>(),
            keys::<HookInstallScriptsTable>(),
            keys::<HookTaskRef>(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let read: BTreeSet<String> = de_fields::<HookTable>();
        assert_eq!(declared, read);
    }

    fn de_fields<T: serde::de::DeserializeOwned>() -> BTreeSet<String> {
        crate::de::struct_fields::<T>()
            .iter()
            .map(|field| field.to_string())
            .collect()
    }

    #[test]
    fn the_hook_events_are_the_events_of_the_schema() {
        // Naming every variant, so a new event fails to compile here until the schema has it.
        let _ = |event: Hooks| match event {
            Hooks::Enter | Hooks::Leave | Hooks::Cd | Hooks::Preinstall | Hooks::Postinstall => {}
        };
        let declared = keys::<HooksDecl>();
        let events: BTreeSet<String> = [
            Hooks::Enter,
            Hooks::Leave,
            Hooks::Cd,
            Hooks::Preinstall,
            Hooks::Postinstall,
        ]
        .iter()
        .map(|event| event.to_string().to_lowercase())
        .collect();
        assert_eq!(declared, events);
    }
}
