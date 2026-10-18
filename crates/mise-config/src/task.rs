//! Task declaration field types: what a `[tasks.<name>]` table, a task template or a `#MISE`
//! header can say, before mise renders templates or resolves tools.
//!
//! Methods that need mise's tera engine or backends stay in mise as extension traits.

use std::fmt::{self, Display, Formatter};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

mod confirm;
mod decl;
mod decls;
mod dep;
mod outputs;
mod schema;
mod secrets;
pub mod shapes;

pub use confirm::TaskConfirm;
pub use decl::{TaskDecl, TaskTemplate};
pub use decls::{TaskDecls, TaskTemplateDecls};
pub use dep::TaskDep;
pub use outputs::TaskOutputs;
pub(crate) use schema::{defs as schema_defs, task_template as template_schema};
pub use secrets::{G7_TEXT, TaskSecrets};

// A `{ version = "..." }`-style tool table, shared by `[tools]` and a task's `tools`: exactly one
// of `version`, `path`, `prefix` or `ref`, plus backend options.
#[derive(Deserialize)]
#[serde(try_from = "RawToolMap")]
pub struct ParsedToolMap {
    pub request: String,
    pub options: IndexMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct RawToolMap {
    version: Option<toml::Value>,
    path: Option<toml::Value>,
    prefix: Option<toml::Value>,
    #[serde(rename = "ref")]
    ref_: Option<toml::Value>,
    #[serde(flatten)]
    options: IndexMap<String, toml::Value>,
}

fn parse_tool_selector(
    key: &'static str,
    value: Option<toml::Value>,
) -> std::result::Result<Option<(&'static str, String)>, String> {
    match value {
        Some(toml::Value::String(value)) => Ok(Some((key, value))),
        Some(_) => Err(format!("tool selector `{key}` must be a string")),
        None => Ok(None),
    }
}

impl TryFrom<RawToolMap> for ParsedToolMap {
    type Error = String;

    fn try_from(raw: RawToolMap) -> std::result::Result<Self, Self::Error> {
        let selectors = [
            parse_tool_selector("version", raw.version)?,
            parse_tool_selector("path", raw.path)?,
            parse_tool_selector("prefix", raw.prefix)?,
            parse_tool_selector("ref", raw.ref_)?,
        ];
        let mut selectors = selectors.into_iter().flatten();
        let Some((key, value)) = selectors.next() else {
            return Err(
                "tool definition must include exactly one of `version`, `path`, `prefix`, or `ref`"
                    .to_string(),
            );
        };
        if let Some((other, _)) = selectors.next() {
            return Err(format!(
                "tool definition cannot specify both `{key}` and `{other}`"
            ));
        }
        let request = if key == "version" {
            value
        } else {
            format!("{key}:{value}")
        };
        Ok(Self {
            request,
            options: raw.options,
        })
    }
}

/// Represents a tool value in task-level tools field.
/// Supports both string syntax (e.g., "1.0.0") and object syntax
/// (e.g., { version = "1.0.0", targets = ["x86_64"] })
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TaskToolValue {
    String(String),
    Map(TaskToolValueMap),
}

impl<'de> Deserialize<'de> for TaskToolValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct TaskToolValueVisitor;

        impl<'de> serde::de::Visitor<'de> for TaskToolValueVisitor {
            type Value = TaskToolValue;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a task tool definition as a string or table")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(TaskToolValue::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(TaskToolValue::String(value))
            }

            fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let parsed =
                    ParsedToolMap::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(TaskToolValue::Map(parsed.into()))
            }
        }

        deserializer.deserialize_any(TaskToolValueVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskToolValueMap {
    pub version: String,
    #[serde(flatten)]
    pub opts: IndexMap<String, toml::Value>,
}

impl From<ParsedToolMap> for TaskToolValueMap {
    fn from(parsed: ParsedToolMap) -> Self {
        Self {
            version: parsed.request,
            opts: parsed.options,
        }
    }
}

impl<'de> Deserialize<'de> for TaskToolValueMap {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(ParsedToolMap::deserialize(deserializer)?.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RunEntry {
    /// Shell script entry
    Script(String),
    /// Run a single task with optional args and env
    SingleTask {
        task: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
        env: IndexMap<String, String>,
    },
    /// Run multiple tasks in parallel
    TaskGroup { tasks: Vec<String> },
}

// Written by hand rather than `#[serde(untagged)]`, so an entry that matches no shape says which
// shapes there are. A table reads as untagged did: as a single task when `task`, `args` and `env`
// parse, which then ignores `tasks`, and otherwise as a task group, which ignores the others.
impl<'de> Deserialize<'de> for RunEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RunEntryVisitor;

        impl<'de> serde::de::Visitor<'de> for RunEntryVisitor {
            type Value = RunEntry;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a script string or a table with `task` or `tasks`")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(RunEntry::Script(v.to_string()))
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                use serde::de::Error;

                // The known keys are buffered, so only the ones the chosen shape reads are
                // checked; any other key is skipped where serde_ignored can report it.
                let mut table = toml::Table::new();
                while let Some(key) = map.next_key::<String>()? {
                    if matches!(key.as_str(), "task" | "args" | "env" | "tasks") {
                        table.insert(key, map.next_value()?);
                    } else {
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
                let field = |key: &str| table.get(key).cloned();
                let single = field("task").map(|task| {
                    let task = task
                        .try_into()
                        .map_err(|e| M::Error::custom(format!("`task`: {e}")))?;
                    let args = field("args")
                        .map(toml::Value::try_into)
                        .transpose()
                        .map_err(|e| M::Error::custom(format!("`args`: {e}")))?
                        .unwrap_or_default();
                    let env = field("env")
                        .map(toml::Value::try_into)
                        .transpose()
                        .map_err(|e| M::Error::custom(format!("`env`: {e}")))?
                        .unwrap_or_default();
                    Ok(RunEntry::SingleTask { task, args, env })
                });
                let single = match single {
                    Some(Ok(single)) => return Ok(single),
                    Some(Err(e)) => Some(e),
                    None => None,
                };
                // As with untagged, a table that is not a valid single task can still be a group.
                let group = field("tasks").map(|tasks| {
                    tasks
                        .try_into()
                        .map(|tasks| RunEntry::TaskGroup { tasks })
                        .map_err(|e| M::Error::custom(format!("`tasks`: {e}")))
                });
                match (group, single) {
                    (Some(Ok(group)), _) => Ok(group),
                    // a table with `task` reports what is wrong with it as a single task
                    (_, Some(e)) | (Some(Err(e)), None) => Err(e),
                    (None, None) => Err(M::Error::custom(
                        "a run entry table needs `task` (one task) or `tasks` (tasks run in parallel)",
                    )),
                }
            }
        }

        deserializer.deserialize_any(RunEntryVisitor)
    }
}

#[cfg(test)]
mod run_entry_tests {
    use super::RunEntry;

    fn parse(value: &str) -> Result<RunEntry, toml::de::Error> {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            run: RunEntry,
        }
        toml::from_str::<Wrapper>(&format!("run = {value}")).map(|w| w.run)
    }

    #[test]
    fn a_table_reads_as_untagged_did() {
        // `task` wins, and `tasks` beside it is not read, whatever its type.
        assert!(matches!(
            parse(r#"{ task = "lint", tasks = "ignored" }"#).unwrap(),
            RunEntry::SingleTask { task, .. } if task == "lint"
        ));
        // a group does not read `args` or `env`
        assert!(matches!(
            parse(r#"{ tasks = ["a", "b"], args = 1 }"#).unwrap(),
            RunEntry::TaskGroup { tasks } if tasks == ["a", "b"]
        ));
        assert!(
            parse(r#"{ task = "lint", args = 1 }"#)
                .unwrap_err()
                .to_string()
                .contains("`args`")
        );
        // a table that fails as a single task still reads as a group
        assert!(matches!(
            parse(r#"{ task = 1, tasks = ["lint"] }"#).unwrap(),
            RunEntry::TaskGroup { tasks } if tasks == ["lint"]
        ));
        assert!(matches!(
            parse(r#"{ task = "lint", args = 1, tasks = ["a"] }"#).unwrap(),
            RunEntry::TaskGroup { tasks } if tasks == ["a"]
        ));
        assert!(
            parse(r#"{ task = 1, tasks = 2 }"#)
                .unwrap_err()
                .to_string()
                .contains("`task`")
        );
        assert!(
            parse(r#"{ tsk = "lint" }"#)
                .unwrap_err()
                .to_string()
                .contains("needs `task`")
        );
        // an unknown key is skipped where serde_ignored sees it
        let mut ignored = vec![];
        let des = toml::Deserializer::parse(r#"run = { task = "lint", tsak = 1 }"#).unwrap();
        #[derive(serde::Deserialize)]
        struct Wrapper {
            run: RunEntry,
        }
        let wrapper: Wrapper =
            serde_ignored::deserialize(des, |path| ignored.push(path.to_string())).unwrap();
        assert!(matches!(wrapper.run, RunEntry::SingleTask { .. }));
        assert_eq!(ignored, ["run.tsak"]);
    }
}

impl std::hash::Hash for RunEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            RunEntry::Script(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            RunEntry::SingleTask { task, args, env } => {
                1u8.hash(state);
                task.hash(state);
                args.hash(state);
                let mut pairs: Vec<_> = env.iter().collect();
                pairs.sort_by_key(|(k, _)| k.as_str());
                for (k, v) in pairs {
                    k.hash(state);
                    v.hash(state);
                }
            }
            RunEntry::TaskGroup { tasks } => {
                2u8.hash(state);
                tasks.hash(state);
            }
        }
    }
}

impl std::str::FromStr for RunEntry {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RunEntry::Script(s.to_string()))
    }
}

impl Display for RunEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RunEntry::Script(s) => write!(f, "{}", s),
            RunEntry::SingleTask { task, args, env } => {
                for (k, v) in env {
                    write!(f, "{}={} ", k, v)?;
                }
                write!(f, "task: {task}")?;
                if !args.is_empty() {
                    write!(f, " {}", args.join(" "))?;
                }
                Ok(())
            }
            RunEntry::TaskGroup { tasks } => write!(f, "tasks: {}", tasks.join(", ")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Silent {
    #[default]
    Off,
    Bool(bool),
    Stdout,
    Stderr,
}

impl Silent {
    pub fn is_silent(&self) -> bool {
        matches!(self, Silent::Bool(true) | Silent::Stdout | Silent::Stderr)
    }

    pub fn suppresses_stdout(&self) -> bool {
        matches!(self, Silent::Bool(true) | Silent::Stdout)
    }

    pub fn suppresses_stderr(&self) -> bool {
        matches!(self, Silent::Bool(true) | Silent::Stderr)
    }

    pub fn suppresses_both(&self) -> bool {
        matches!(self, Silent::Bool(true))
    }
}

impl Serialize for Silent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Silent::Off | Silent::Bool(false) => serializer.serialize_bool(false),
            Silent::Bool(true) => serializer.serialize_bool(true),
            Silent::Stdout => serializer.serialize_str("stdout"),
            Silent::Stderr => serializer.serialize_str("stderr"),
        }
    }
}

impl From<bool> for Silent {
    fn from(b: bool) -> Self {
        if b { Silent::Bool(true) } else { Silent::Off }
    }
}

impl std::str::FromStr for Silent {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "true" => Ok(Silent::Bool(true)),
            "false" => Ok(Silent::Off),
            "stdout" => Ok(Silent::Stdout),
            "stderr" => Ok(Silent::Stderr),
            _ => Err(format!(
                "invalid silent value: {}, expected true, false, 'stdout', or 'stderr'",
                s
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Silent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SilentVisitor;

        impl<'de> serde::de::Visitor<'de> for SilentVisitor {
            type Value = Silent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a boolean or a string ('stdout' or 'stderr')")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Silent, E>
            where
                E: serde::de::Error,
            {
                Ok(Silent::from(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Silent, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "stdout" => Ok(Silent::Stdout),
                    "stderr" => Ok(Silent::Stderr),
                    _ => Err(E::custom(format!(
                        "invalid silent value: '{}', expected 'stdout' or 'stderr'",
                        value
                    ))),
                }
            }
        }

        deserializer.deserialize_any(SilentVisitor)
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TaskWatchOptions {
    /// Ignore VCS ignore files such as `.gitignore` when running `mise watch`.
    pub no_vcs_ignore: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TaskRustCacheOptions {
    enabled: bool,
    #[serde(rename = "verify")]
    _verify: bool,
}

impl Default for TaskRustCacheOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            _verify: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRustCacheConfig {
    pub enabled: bool,
}

impl Default for TaskRustCacheConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl<'de> Deserialize<'de> for TaskRustCacheConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RustCacheVisitor;

        impl<'de> serde::de::Visitor<'de> for RustCacheVisitor {
            type Value = TaskRustCacheConfig;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a boolean or Rust cache options table")
            }

            fn visit_bool<E>(self, enabled: bool) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(TaskRustCacheConfig { enabled })
            }

            fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let options = TaskRustCacheOptions::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(TaskRustCacheConfig {
                    enabled: options.enabled,
                })
            }
        }

        deserializer.deserialize_any(RustCacheVisitor)
    }
}

/// Project daemons a task requires: `true` for every daemon declared in the
/// project, or an explicit name or list of names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TaskDaemons {
    All(bool),
    One(String),
    Names(Vec<String>),
}

impl TaskDaemons {
    /// The names written in the declaration. Empty for `true` and `false`,
    /// which name no daemon.
    pub fn names(&self) -> &[String] {
        match self {
            Self::All(_) => &[],
            Self::One(name) => std::slice::from_ref(name),
            Self::Names(names) => names.as_slice(),
        }
    }

    pub fn names_mut(&mut self) -> &mut [String] {
        match self {
            Self::All(_) => &mut [],
            Self::One(name) => std::slice::from_mut(name),
            Self::Names(names) => names.as_mut_slice(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskCacheConfig {
    pub enabled: bool,
    /// Report project files read or written outside the declared cache contract.
    pub audit: bool,
    /// Ambient environment variables whose resolved values affect the cache key.
    pub env: Vec<String>,
    /// Commands whose stdout and stderr affect the cache key.
    pub command_inputs: Vec<String>,
}

/// How task output is presented. mise's task runner resolves it further; see
/// `TaskOutputExt` there.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    strum::Display,
    strum::EnumString,
    strum::EnumIs,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum TaskOutput {
    Interleave,
    KeepOrder,
    #[default]
    Prefix,
    Replacing,
    Timed,
    Quiet,
    Silent,
}

// `[task_config]`: how this config root's tasks are found and run.
/// configuration for task execution/management
#[derive(Clone, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
#[schemars(extend("unevaluatedProperties" = false))]
pub struct TaskConfig {
    /// cascade task configuration to descendant config roots
    #[schemars(extend("default" = false))]
    pub cascade: Option<bool>,
    /// default directory to run tasks in defined in this file
    pub dir: Option<String>,
    /// default shell to run tasks with in this config scope
    pub shell: Option<String>,
    #[schemars(with = "Option<shapes::TaskConfigCacheDecl>")]
    pub cache: Option<TaskCacheConfig>,
    #[schemars(with = "Option<shapes::RustCacheDecl>")]
    pub rust_cache: Option<TaskRustCacheConfig>,
    /// experimental ambient environment variable names added to every enabled task cache key in this config scope
    pub global_env: Vec<String>,
    /// experimental ambient environment variable names preserved under environment denial without affecting task cache keys
    pub global_pass_through_env: Vec<String>,
    /// experimental config-root-relative source patterns added to every task in this config scope; entries may reference input groups with @group:<name>
    pub global_inputs: Vec<String>,
    /// experimental reusable config-root-relative source groups referenced with @group:<name>
    #[schemars(with = "IndexMap<String, shapes::TaskInputGroup>")]
    pub input_groups: IndexMap<String, Vec<String>>,
    #[schemars(with = "Option<shapes::TaskIncludes>")]
    pub includes: Option<Vec<String>>,
    /// config-root-relative paths or glob patterns to exclude from file-task discovery
    pub excludes: Option<Vec<String>>,
}

#[cfg(test)]
mod shape_tests {
    use super::shapes::*;
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

    fn fields<T: serde::de::DeserializeOwned>() -> BTreeSet<String> {
        crate::de::struct_fields::<T>()
            .iter()
            .map(|field| field.to_string())
            .collect()
    }

    // The schema is generated from the types in `shapes.rs`; the parsers read these structs. A
    // key added to one without the other fails here.
    #[test]
    fn the_declared_keys_are_the_keys_the_parsers_read() {
        assert_eq!(keys::<TaskWatchDecl>(), fields::<TaskWatchOptions>());
        assert_eq!(keys::<TaskCacheDecl>(), fields::<TaskCacheConfig>());
        assert_eq!(keys::<TaskConfigCacheDecl>(), fields::<TaskCacheConfig>());
        assert_eq!(keys::<RustCacheTable>(), fields::<TaskRustCacheOptions>());
    }

    // `TaskDep` reads a table with a visitor, so each declared key is set in turn.
    #[test]
    fn a_dependency_table_reads_the_keys_the_schema_declares() {
        let declared = keys::<TaskDependencyTable>();
        assert_eq!(
            declared,
            ["args", "env", "optional", "task"].map(String::from).into()
        );
        for key in &declared {
            let sample = match key.as_str() {
                "task" => "\"build\"",
                "args" => "[\"a\"]",
                "env" => "{ A = \"b\" }",
                _ => "true",
            };
            let table = format!(
                "task = \"build\"\n{}",
                if key == "task" {
                    String::new()
                } else {
                    format!("{key} = {sample}")
                }
            );
            let dep: Result<TaskDep, _> = toml::from_str(&table);
            assert!(
                dep.is_ok(),
                "`{key}` is declared, but a dependency rejects it: {dep:?}"
            );
        }
    }
}
