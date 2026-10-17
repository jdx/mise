//! `[hooks]`: commands, scripts and tasks run on `cd`, on entering or leaving a project, and
//! around tool installs.

mod schema;
pub(crate) use schema::defs as schema_defs;

use serde::Deserialize;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
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

#[derive(Debug, Clone)]
pub struct HookRunTable {
    pub run: Option<String>,
    pub run_windows: Option<String>,
    pub shell: Option<String>,
}

impl<'de> Deserialize<'de> for HookRunTable {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Helper {
            run: Option<String>,
            run_windows: Option<String>,
            shell: Option<String>,
        }

        let helper = <Helper as Deserialize>::deserialize(deserializer)?;
        if helper.run.is_none() && helper.run_windows.is_none() {
            return Err(serde::de::Error::custom(
                "hook run table must define `run` or `run_windows`",
            ));
        }
        Ok(Self {
            run: helper.run,
            run_windows: helper.run_windows,
            shell: helper.shell,
        })
    }
}
