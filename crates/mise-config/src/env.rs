//! `[env]` and `[vars]`: the directives parsed from a config file, before any of them is
//! rendered or applied. mise's `config::env_directive` module resolves them.

use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::OnceLock;

use crate::deprecation::{Deprecation, report};
use crate::env_value::EnvValue;

use serde::de::Visitor;
use serde::{Deserialize, Deserializer, de};

mod decl;
pub use decl::*;

static DISPLAY_PATH: OnceLock<fn(&str) -> String> = OnceLock::new();

/// How [`EnvDirective`]'s `Display` shows a path, so the home directory reads as `~` the way
/// the rest of mise's output does. mise registers this at startup; without it paths print as
/// written.
pub fn set_display_path(display: fn(&str) -> String) {
    let _ = DISPLAY_PATH.set(display);
}

fn display_path(path: &str) -> String {
    match DISPLAY_PATH.get() {
        Some(display) => display(path),
        None => path.to_string(),
    }
}

/// The environment directives of one `[env]` or `[vars]` table, in the order they apply.
#[derive(Debug, Default, Clone)]
pub struct EnvList(pub Vec<EnvDirective>);

impl EnvList {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// A field of this type points at the hand-written definition in `schema`.
impl schemars::JsonSchema for EnvList {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        "env".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"$ref": "#/$defs/env"})
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum RequiredValue {
    #[default]
    False,
    True,
    Help(String),
}

impl RequiredValue {
    pub fn is_required(&self) -> bool {
        !matches!(self, RequiredValue::False)
    }

    pub fn help_text(&self) -> Option<&str> {
        match self {
            RequiredValue::Help(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

impl<'de> serde::Deserialize<'de> for RequiredValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct RequiredVisitor;

        impl<'de> Visitor<'de> for RequiredVisitor {
            type Value = RequiredValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a boolean or a string")
            }

            fn visit_bool<E>(self, value: bool) -> Result<RequiredValue, E>
            where
                E: de::Error,
            {
                Ok(if value {
                    RequiredValue::True
                } else {
                    RequiredValue::False
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<RequiredValue, E>
            where
                E: de::Error,
            {
                Ok(RequiredValue::Help(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<RequiredValue, E>
            where
                E: de::Error,
            {
                Ok(RequiredValue::Help(value))
            }
        }

        deserializer.deserialize_any(RequiredVisitor)
    }
}

impl serde::Serialize for RequiredValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RequiredValue::False => serializer.serialize_bool(false),
            RequiredValue::True => serializer.serialize_bool(true),
            RequiredValue::Help(text) => serializer.serialize_str(text),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvDirectiveOptions {
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub redact: Option<bool>,
    #[serde(default)]
    pub required: RequiredValue,
    #[serde(default)]
    pub expand: bool,
    /// `_.file` only: look for a relative path in the current directory and each parent
    /// directory, rather than only in the config file's directory.
    #[serde(default)]
    pub find_up: bool,
    /// `[vars]` only: ask for this value once and remember the answer on this machine.
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EnvDirective {
    /// simple key/value pair
    Val(String, String, EnvDirectiveOptions),
    /// use a fallback value if the key is not already set
    Default(String, String, EnvDirectiveOptions),
    /// remove a key
    Rm(String, EnvDirectiveOptions),
    /// Required variable that must be defined elsewhere
    Required(String, EnvDirectiveOptions),
    /// dotenv file
    File(String, EnvDirectiveOptions),
    /// Several `_.file` names written as one `path` list. With `find_up` they are searched
    /// together, directory by directory, instead of one full search per name.
    FileGroup(Vec<String>, EnvDirectiveOptions),
    /// add a path to the PATH
    Path(String, EnvDirectiveOptions),
    /// run a bash script and apply the resulting env diff
    Source(String, EnvDirectiveOptions),
    /// [experimental] age-encrypted value
    Age {
        key: String,
        value: String,
        format: Option<AgeFormat>,
        options: EnvDirectiveOptions,
    },
    PythonVenv {
        path: String,
        create: bool,
        python: Option<String>,
        uv_create_args: Option<Vec<String>>,
        python_create_args: Option<Vec<String>>,
        options: EnvDirectiveOptions,
    },
    Module(String, toml::Value, EnvDirectiveOptions),
}

impl EnvDirective {
    pub fn options(&self) -> &EnvDirectiveOptions {
        match self {
            EnvDirective::Val(_, _, opts)
            | EnvDirective::Default(_, _, opts)
            | EnvDirective::Rm(_, opts)
            | EnvDirective::Required(_, opts)
            | EnvDirective::File(_, opts)
            | EnvDirective::FileGroup(_, opts)
            | EnvDirective::Path(_, opts)
            | EnvDirective::Source(_, opts)
            | EnvDirective::Age { options: opts, .. }
            | EnvDirective::PythonVenv { options: opts, .. }
            | EnvDirective::Module(_, _, opts) => opts,
        }
    }
}

impl From<(String, String)> for EnvDirective {
    fn from((k, v): (String, String)) -> Self {
        Self::Val(k, v, Default::default())
    }
}

impl From<(String, i64)> for EnvDirective {
    fn from((k, v): (String, i64)) -> Self {
        (k, v.to_string()).into()
    }
}

impl Display for EnvDirective {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvDirective::Val(k, v, _) => write!(f, "{k}={v}"),
            EnvDirective::Default(k, v, _) => write!(f, "{k} default={v}"),
            EnvDirective::Rm(k, _) => write!(f, "unset {k}"),
            EnvDirective::Required(k, _) => write!(f, "{k} (required)"),
            EnvDirective::File(path, _) => write!(f, "_.file = \"{}\"", display_path(path)),
            EnvDirective::FileGroup(paths, _) => write!(
                f,
                "_.file = [{}]",
                paths
                    .iter()
                    .map(|path| format!("\"{}\"", display_path(path)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            EnvDirective::Path(path, _) => write!(f, "_.path = \"{}\"", display_path(path)),
            EnvDirective::Source(path, _) => write!(f, "_.source = \"{}\"", display_path(path)),
            EnvDirective::Age { key, format, .. } => {
                write!(f, "{key} (age-encrypted")?;
                if let Some(fmt) = format {
                    let fmt_str = match fmt {
                        AgeFormat::Zstd => "zstd",
                        AgeFormat::Raw => "raw",
                    };
                    write!(f, ", {fmt_str}")?;
                }
                write!(f, ")")
            }
            EnvDirective::Module(name, _, _) => write!(f, "module {name}"),
            EnvDirective::PythonVenv {
                path,
                create,
                python,
                uv_create_args,
                python_create_args,
                ..
            } => {
                write!(f, "python venv path={}", display_path(path))?;
                if *create {
                    write!(f, " create")?;
                }
                if let Some(python) = python {
                    write!(f, " python={python}")?;
                }
                if let Some(args) = uv_create_args {
                    write!(f, " uv_create_args={args:?}")?;
                }
                if let Some(args) = python_create_args {
                    write!(f, " python_create_args={args:?}")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum AgeFormat {
    #[serde(rename = "zstd")]
    Zstd,
    #[serde(rename = "raw")]
    #[default]
    Raw,
}

impl<'de> de::Deserialize<'de> for EnvList {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct EnvManVisitor;

        impl<'de> Visitor<'de> for EnvManVisitor {
            type Value = EnvList;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("environment variable table")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut env = vec![];
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "_" | "mise" => {
                            if key == "mise" {
                                report(Deprecation::EnvMise);
                            }
                            #[derive(Deserialize)]
                            #[serde(untagged)]
                            enum MiseTomlEnvDirectiveValue {
                                Single {
                                    #[serde(alias = "value")]
                                    path: String,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Multiple {
                                    #[serde(alias = "value", alias = "values", alias = "paths")]
                                    path: Vec<String>,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                            }

                            struct MiseTomlEnvDirective(MiseTomlEnvDirectiveValue);

                            impl<'de> Deserialize<'de> for MiseTomlEnvDirective {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    let value = toml::Value::deserialize(deserializer)?;
                                    let (uses_value, uses_values) = value
                                        .as_table()
                                        .map(|table| {
                                            (
                                                table.contains_key("value"),
                                                table.contains_key("values"),
                                            )
                                        })
                                        .unwrap_or_default();
                                    let directive = MiseTomlEnvDirectiveValue::deserialize(value)
                                        .map_err(de::Error::custom)?;

                                    if uses_value {
                                        report(Deprecation::DirectiveValue);
                                    }
                                    if uses_values {
                                        report(Deprecation::DirectiveValues);
                                    }

                                    Ok(MiseTomlEnvDirective(directive))
                                }
                            }

                            impl FromStr for MiseTomlEnvDirective {
                                type Err = String;
                                fn from_str(s: &str) -> Result<Self, Self::Err> {
                                    Ok(MiseTomlEnvDirective(MiseTomlEnvDirectiveValue::Single {
                                        path: s.to_string(),
                                        options: Default::default(),
                                    }))
                                }
                            }

                            struct EnvDirectivePythonVenv {
                                path: String,
                                create: bool,
                                python: Option<String>,
                                uv_create_args: Option<Vec<String>>,
                                python_create_args: Option<Vec<String>>,
                            }

                            #[derive(Deserialize, Default)]
                            #[serde(deny_unknown_fields)]
                            struct EnvDirectivePython {
                                #[serde(default)]
                                venv: Option<EnvDirectivePythonVenv>,
                            }

                            // Reuses `deserialize_arr` so each of `path`/`file`/`source`
                            // accepts either a single value or a list, while
                            // `ParsedEnvBlock` below iterates the `_` table in the order
                            // the keys were written.
                            #[derive(Deserialize)]
                            struct DirectiveArr(
                                #[serde(deserialize_with = "crate::de::deserialize_arr")]
                                Vec<MiseTomlEnvDirective>,
                            );

                            impl<'de> de::Deserialize<'de> for EnvDirectivePythonVenv {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    struct EnvDirectivePythonVenvVisitor;

                                    impl<'de> Visitor<'de> for EnvDirectivePythonVenvVisitor {
                                        type Value = EnvDirectivePythonVenv;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("python venv directive")
                                        }

                                        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(EnvDirectivePythonVenv {
                                                path: v.into(),
                                                create: false,
                                                python: None,
                                                uv_create_args: None,
                                                python_create_args: None,
                                            })
                                        }

                                        fn visit_map<M>(
                                            self,
                                            mut map: M,
                                        ) -> Result<Self::Value, M::Error>
                                        where
                                            M: de::MapAccess<'de>,
                                        {
                                            let mut path = None;
                                            let mut create = false;
                                            let mut python = None;
                                            let mut uv_create_args = None;
                                            let mut python_create_args = None;
                                            while let Some(key) = map.next_key::<String>()? {
                                                match key.as_str() {
                                                    "path" => {
                                                        path = Some(map.next_value()?);
                                                    }
                                                    "create" => {
                                                        create = map.next_value()?;
                                                    }
                                                    "python" => {
                                                        python = Some(map.next_value()?);
                                                    }
                                                    "uv_create_args" => {
                                                        uv_create_args = Some(map.next_value()?);
                                                    }
                                                    "python_create_args" => {
                                                        python_create_args =
                                                            Some(map.next_value()?);
                                                    }
                                                    _ => {
                                                        return Err(de::Error::unknown_field(
                                                            &key,
                                                            &["path", "create"],
                                                        ));
                                                    }
                                                }
                                            }
                                            let path = path
                                                .ok_or_else(|| de::Error::missing_field("path"))?;
                                            Ok(EnvDirectivePythonVenv {
                                                path,
                                                create,
                                                python,
                                                uv_create_args,
                                                python_create_args,
                                            })
                                        }
                                    }

                                    const FIELDS: &[&str] = &["path", "create"];
                                    deserializer.deserialize_struct(
                                        "PythonVenv",
                                        FIELDS,
                                        EnvDirectivePythonVenvVisitor,
                                    )
                                }
                            }

                            fn flatten_directives<F>(
                                directives: Vec<MiseTomlEnvDirective>,
                                constructor: F,
                            ) -> impl Iterator<Item = EnvDirective>
                            where
                                F: Fn(String, EnvDirectiveOptions) -> EnvDirective + 'static,
                            {
                                directives.into_iter().flat_map(move |d| match d.0 {
                                    MiseTomlEnvDirectiveValue::Single { path, options } => {
                                        vec![constructor(path, options)]
                                    }
                                    MiseTomlEnvDirectiveValue::Multiple { path, options } => path
                                        .into_iter()
                                        .map(|v| constructor(v, options.clone()))
                                        .collect(),
                                })
                            }

                            /// Splits a `find_up` list in one `path` into entries. A run of plain
                            /// relative names is one search over all of them, directory by
                            /// directory. A path that is not plain stands alone, in list order, as it
                            /// would as a separate entry: a template or `$VAR` can depend on a value
                            /// an earlier file in the list loaded, so it has to resolve after that
                            /// file, and an absolute path or `~` has no directory to search.
                            fn group_find_up_files(
                                paths: Vec<String>,
                                options: EnvDirectiveOptions,
                            ) -> Vec<EnvDirective> {
                                fn plain_relative(path: &str) -> bool {
                                    let as_path = std::path::Path::new(path);
                                    !["{{", "{%", "{#", "$"]
                                        .iter()
                                        .any(|marker| path.contains(marker))
                                        && !as_path.is_absolute()
                                        // `~` and `~/...` expand to the home directory, as
                                        // mise's `file::replace_path` does.
                                        && as_path.strip_prefix("~/").is_err()
                                }
                                fn flush(
                                    run: &mut Vec<String>,
                                    options: &EnvDirectiveOptions,
                                    out: &mut Vec<EnvDirective>,
                                ) {
                                    match run.len() {
                                        0 => {}
                                        1 => out.push(EnvDirective::File(
                                            run.remove(0),
                                            options.clone(),
                                        )),
                                        _ => out.push(EnvDirective::FileGroup(
                                            std::mem::take(run),
                                            options.clone(),
                                        )),
                                    }
                                    run.clear();
                                }
                                let mut out = vec![];
                                let mut run = vec![];
                                for path in paths {
                                    if plain_relative(&path) {
                                        run.push(path);
                                    } else {
                                        flush(&mut run, &options, &mut out);
                                        out.push(EnvDirective::File(path, options.clone()));
                                    }
                                }
                                flush(&mut run, &options, &mut out);
                                out
                            }

                            // Parse the `_` table preserving the written order of its
                            // sub-keys (`path`/`file`/`source`/modules) so that a later
                            // directive's template can reference a variable exported by
                            // an earlier one — e.g. `_.path` using a var from `_.source`
                            // (discussion #3783). `python.venv` is applied last
                            // regardless of position: it is a tools-phase directive whose
                            // PATH conventionally comes after tool paths.
                            struct ParsedEnvBlock {
                                directives: Vec<EnvDirective>,
                                venv: Option<EnvDirectivePythonVenv>,
                            }

                            impl<'de> Deserialize<'de> for ParsedEnvBlock {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    struct ParsedEnvBlockVisitor;
                                    impl<'de> Visitor<'de> for ParsedEnvBlockVisitor {
                                        type Value = ParsedEnvBlock;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("the env `_` directive table")
                                        }
                                        fn visit_map<M>(
                                            self,
                                            mut map: M,
                                        ) -> Result<Self::Value, M::Error>
                                        where
                                            M: de::MapAccess<'de>,
                                        {
                                            let mut directives = vec![];
                                            let mut venv = None;
                                            while let Some(key) = map.next_key::<String>()? {
                                                match key.as_str() {
                                                    "path" => {
                                                        directives.extend(flatten_directives(
                                                            map.next_value::<DirectiveArr>()?.0,
                                                            EnvDirective::Path,
                                                        ));
                                                    }
                                                    "file" => {
                                                        for d in map.next_value::<DirectiveArr>()?.0
                                                        {
                                                            match d.0 {
                                                                MiseTomlEnvDirectiveValue::Multiple {
                                                                    path,
                                                                    options,
                                                                } if options.find_up => {
                                                                    directives.extend(
                                                                        group_find_up_files(
                                                                            path, options,
                                                                        ),
                                                                    );
                                                                }
                                                                value => directives.extend(
                                                                    flatten_directives(
                                                                        vec![MiseTomlEnvDirective(
                                                                            value,
                                                                        )],
                                                                        EnvDirective::File,
                                                                    ),
                                                                ),
                                                            }
                                                        }
                                                    }
                                                    "source" => {
                                                        directives.extend(flatten_directives(
                                                            map.next_value::<DirectiveArr>()?.0,
                                                            EnvDirective::Source,
                                                        ));
                                                    }
                                                    "python" => {
                                                        venv = map
                                                            .next_value::<EnvDirectivePython>()?
                                                            .venv;
                                                    }
                                                    _ => {
                                                        let mut value =
                                                            map.next_value::<toml::Value>()?;
                                                        let mut opts =
                                                            EnvDirectiveOptions::default();
                                                        if let Some(table) = value.as_table_mut()
                                                            && let Some(tools) =
                                                                table.remove("tools")
                                                        {
                                                            opts.tools =
                                                                tools.as_bool().unwrap_or(false);
                                                        }
                                                        // The user's explicit `redact` overrides the plugin's own preference.
                                                        if let Some(table) = value.as_table_mut()
                                                            && let Some(redact) =
                                                                table.remove("redact")
                                                        {
                                                            opts.redact = Some(redact.as_bool().ok_or_else(|| {
                                                                de::Error::custom(format!(
                                                                    "_.{key}.redact must be a boolean"
                                                                ))
                                                            })?);
                                                        }
                                                        directives.push(EnvDirective::Module(
                                                            key, value, opts,
                                                        ));
                                                    }
                                                }
                                            }
                                            Ok(ParsedEnvBlock { directives, venv })
                                        }
                                    }
                                    deserializer.deserialize_map(ParsedEnvBlockVisitor)
                                }
                            }

                            let block = map.next_value::<ParsedEnvBlock>()?;
                            env.extend(block.directives);
                            if let Some(venv) = block.venv {
                                env.push(EnvDirective::PythonVenv {
                                    path: venv.path,
                                    create: venv.create,
                                    python: venv.python,
                                    uv_create_args: venv.uv_create_args,
                                    python_create_args: venv.python_create_args,
                                    options: EnvDirectiveOptions {
                                        tools: true,
                                        redact: Some(false),
                                        required: RequiredValue::False,
                                        expand: false,
                                        find_up: false,
                                        prompt: None,
                                    },
                                });
                            }
                        }
                        _ => {
                            #[derive(Deserialize)]
                            #[serde(untagged)]
                            enum Val {
                                AgeComplex {
                                    age: AgeComplexVal,
                                },
                                AgeWithOptions {
                                    age: String,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Map {
                                    value: EnvValue,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                DefaultMap {
                                    default: EnvValue,
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                OptionsOnly {
                                    #[serde(flatten)]
                                    options: EnvDirectiveOptions,
                                },
                                Primitive(EnvValue),
                            }

                            #[derive(Deserialize)]
                            struct AgeComplexVal {
                                value: String,
                                #[serde(default)]
                                format: Option<AgeFormat>,
                                #[serde(flatten)]
                                options: EnvDirectiveOptions,
                            }
                            let raw_value = map.next_value::<toml::Value>()?;
                            if let Some(table) = raw_value.as_table() {
                                let has_default = table.contains_key("default");
                                if has_default && table.contains_key("value") {
                                    return Err(serde::de::Error::custom(format!(
                                        "Environment variable '{}' cannot have both 'value' and 'default'. The 'value' field always overwrites, while 'default' only applies when the variable is unset or empty. Remove either the 'value' field or the 'default' field.",
                                        key
                                    )));
                                }
                                if has_default && table.contains_key("required") {
                                    return Err(serde::de::Error::custom(format!(
                                        "Environment variable '{}' cannot have both 'default' and 'required'. The 'required' flag means the variable must be defined elsewhere, while 'default' provides a fallback value. Remove either the 'default' field or the 'required' flag.",
                                        key
                                    )));
                                }
                                if has_default && table.contains_key("age") {
                                    return Err(serde::de::Error::custom(format!(
                                        "Environment variable '{}' cannot have both 'age' and 'default'. Remove either the 'age' field or the 'default' field.",
                                        key
                                    )));
                                }
                            }
                            let kind = match &raw_value {
                                toml::Value::Array(_) => Some("an array"),
                                toml::Value::Float(_) => Some("a float"),
                                toml::Value::Datetime(_) => Some("a date"),
                                _ => None,
                            };
                            if let Some(kind) = kind {
                                return Err(serde::de::Error::custom(format!(
                                    "Environment variable '{key}' cannot be {kind}. Set it to a string, integer or boolean, or to a table with `value`, `default`, `required` or `age`."
                                )));
                            }
                            let val_result = raw_value.try_into::<Val>().map_err(|e| {
                                // `Val` is untagged, whose own error only says no shape matched.
                                let e = e.to_string();
                                let detail = if e.contains("untagged enum Val") {
                                    "`value` takes a string, integer or boolean, `default` a string or integer, `required` a boolean or help text, and `age` an encrypted string or a table with `value`".to_string()
                                } else {
                                    e
                                };
                                serde::de::Error::custom(format!(
                                    "failed to parse environment variable '{}': {}",
                                    key, detail
                                ))
                            })?;

                            // Handle Age variants separately since they create different directive types
                            match &val_result {
                                Val::AgeComplex { age } => {
                                    let directive = EnvDirective::Age {
                                        key: key.clone(),
                                        value: age.value.clone(),
                                        format: age.format.clone(),
                                        options: age.options.clone(),
                                    };
                                    env.push(directive);
                                    continue;
                                }
                                Val::AgeWithOptions { age, options } => {
                                    let directive = EnvDirective::Age {
                                        key: key.clone(),
                                        value: age.clone(),
                                        format: None, // Default format for simplified syntax with options
                                        options: options.clone(),
                                    };
                                    env.push(directive);
                                    continue;
                                }
                                _ => {}
                            }

                            let directive = match val_result {
                                Val::Primitive(value) => match value.into_string() {
                                    Some(s) => {
                                        EnvDirective::Val(key, s, EnvDirectiveOptions::default())
                                    }
                                    None => EnvDirective::Rm(key, EnvDirectiveOptions::default()),
                                },
                                Val::Map { value, options } => {
                                    // Validate that required cannot be used with any value
                                    if options.required.is_required() {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' cannot have both 'value' and 'required'. The 'required' flag means the variable must be defined elsewhere (in the environment or a later config file). Remove either the 'value' field or the 'required' flag.",
                                            key
                                        )));
                                    }
                                    match value.into_string() {
                                        Some(s) => EnvDirective::Val(key, s, options),
                                        None => EnvDirective::Rm(key, options),
                                    }
                                }
                                Val::DefaultMap { default, options } => {
                                    let Some(default) = default.into_default_string() else {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' default cannot be a boolean. Use a string or integer fallback instead.",
                                            key
                                        )));
                                    };
                                    EnvDirective::Default(key, default, options)
                                }
                                Val::OptionsOnly { options } => {
                                    // No value provided - this creates a required variable that must be defined elsewhere
                                    if !options.required.is_required() {
                                        return Err(serde::de::Error::custom(format!(
                                            "Environment variable '{}' has no value. Either provide a value or set required=true to indicate it must be defined elsewhere.",
                                            key
                                        )));
                                    }
                                    // For required variables without a value, we create a Required directive
                                    EnvDirective::Required(key, options)
                                }
                                Val::AgeComplex { .. } | Val::AgeWithOptions { .. } => {
                                    unreachable!() // Already handled above
                                }
                            };
                            env.push(directive);
                        }
                    }
                }
                Ok(EnvList(env))
            }
        }

        deserializer.deserialize_map(EnvManVisitor)
    }
}

pub fn deserialize_vars<'de, D>(deserializer: D) -> std::result::Result<EnvList, D::Error>
where
    D: de::Deserializer<'de>,
{
    let value = toml::Value::deserialize(deserializer)?;
    fn contains_mise(value: &toml::Value) -> bool {
        match value {
            toml::Value::Table(table) => table.contains_key("mise"),
            toml::Value::Array(values) => values.iter().any(contains_mise),
            _ => false,
        }
    }
    if contains_mise(&value) {
        return Err(de::Error::custom("`vars.mise` is not supported"));
    }
    EnvList::deserialize(value).map_err(de::Error::custom)
}

pub fn deserialize_root_env<'de, D>(deserializer: D) -> std::result::Result<EnvList, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct RootEnvVisitor;

    impl<'de> Visitor<'de> for RootEnvVisitor {
        type Value = EnvList;

        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("env table or flat array of env tables")
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            EnvList::deserialize(de::value::MapAccessDeserializer::new(map))
        }

        fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let mut env = vec![];
            while let Some(list) = seq.next_element::<EnvList>()? {
                env.extend(list.0);
            }
            Ok(EnvList(env))
        }
    }

    deserializer.deserialize_any(RootEnvVisitor)
}

#[cfg(test)]
mod schema_tests {
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

    // The schema is generated from the `Env*` and `Vars*` declaration types; the parser reads
    // the options of a variable or directive into `EnvDirectiveOptions`. Every key the types
    // declare besides the ones that carry the value must be one of its fields, and each of its
    // fields must be declared, so a key added to one without the other fails here.
    #[test]
    fn the_declared_option_keys_are_the_options_the_parser_reads() {
        let declared: BTreeSet<String> = [
            keys::<EnvValueTable>(),
            keys::<EnvDefaultTable>(),
            keys::<EnvRequiredTable>(),
            keys::<EnvAgeTable>(),
            keys::<EnvAgeComplex>(),
            keys::<VarsValueTable>(),
            keys::<VarsDefaultTable>(),
            keys::<VarsRequiredTable>(),
            keys::<VarsAgeTable>(),
            keys::<VarsAgeComplex>(),
            keys::<EnvDirectiveDecl>(),
        ]
        .into_iter()
        .flatten()
        // what a table says the value is, rather than an option of it
        .filter(|key| {
            !["value", "default", "age", "path", "paths", "format"].contains(&key.as_str())
        })
        .collect();
        let read: BTreeSet<String> = crate::de::struct_fields::<EnvDirectiveOptions>()
            .iter()
            .map(|field| field.to_string())
            .collect();
        assert_eq!(declared, read);
    }
}
