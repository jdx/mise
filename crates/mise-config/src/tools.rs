//! `[tools]`: what a config file declares for each tool, keyed by the tool as written.
//!
//! mise turns each key into a backend and each [`ToolSpec`] into a tool request; it also checks
//! the options there, because some of those checks need its settings.

use std::fmt::Formatter;

use indexmap::IndexMap;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

use crate::env_value::EnvValue;
use crate::task::ParsedToolMap;

mod decl;
pub use decl::*;
pub(crate) mod schema;
pub(crate) use schema::defs as schema_defs;
pub(crate) use schema::task_tool_patterns;

/// The option sets a tool may take, generated from the types in `decl.rs`, keyed by their
/// `$defs` names. A backend's tool composes them with `allOf`, which closes the result with
/// `unevaluatedProperties`, so closing each here as well would reject the keys the others add.
pub(crate) fn option_defs(
    generator: &mut schemars::SchemaGenerator,
) -> Vec<(&'static str, serde_json::Value)> {
    use crate::schema::open_fragment;
    vec![
        (
            "tool_options",
            open_fragment::<decl::ToolOptions>(generator),
        ),
        (
            "tool_forge_options",
            open_fragment::<decl::ToolForgeOptions>(generator),
        ),
        (
            "tool_github_options",
            open_fragment::<decl::ToolGithubOptions>(generator),
        ),
        (
            "tool_gitlab_options",
            open_fragment::<decl::ToolGitlabOptions>(generator),
        ),
        (
            "tool_forgejo_options",
            open_fragment::<decl::ToolForgejoOptions>(generator),
        ),
        (
            "tool_ubi_options",
            open_fragment::<decl::ToolUbiOptions>(generator),
        ),
        (
            "tool_http_options",
            open_fragment::<decl::ToolHttpOptions>(generator),
        ),
        (
            "tool_s3_options",
            open_fragment::<decl::ToolS3Options>(generator),
        ),
        (
            "tool_aqua_options",
            open_fragment::<decl::ToolAquaOptions>(generator),
        ),
        (
            "tool_cargo_options",
            open_fragment::<decl::ToolCargoOptions>(generator),
        ),
        (
            "tool_npm_options",
            open_fragment::<decl::ToolNpmOptions>(generator),
        ),
        (
            "tool_pypi_options",
            open_fragment::<decl::ToolPypiOptions>(generator),
        ),
        (
            "tool_gem_options",
            open_fragment::<decl::ToolGemOptions>(generator),
        ),
        (
            "tool_go_options",
            open_fragment::<decl::ToolGoOptions>(generator),
        ),
        (
            "tool_conda_options",
            open_fragment::<decl::ToolCondaOptions>(generator),
        ),
        (
            "tool_spm_options",
            open_fragment::<decl::ToolSpmOptions>(generator),
        ),
        (
            "tool_packslip_options",
            open_fragment::<decl::ToolPackslipOptions>(generator),
        ),
        (
            "tool_spinel_options",
            open_fragment::<decl::ToolSpinelOptions>(generator),
        ),
        (
            "tool_core_python_options",
            open_fragment::<decl::ToolCorePythonOptions>(generator),
        ),
        (
            "tool_core_java_options",
            open_fragment::<decl::ToolCoreJavaOptions>(generator),
        ),
        (
            "tool_core_rust_options",
            open_fragment::<decl::ToolCoreRustOptions>(generator),
        ),
        (
            "tool_core_dotnet_options",
            open_fragment::<decl::ToolCoreDotnetOptions>(generator),
        ),
    ]
}

/// `[tools]` as written: tool key (`node`, `"cargo:ripgrep"`, `"node[os=linux]"`) to its versions.
pub type ToolsDecl<T = ToolSpec> = IndexMap<String, ToolSpecList<T>>;

/// The value of one `[tools]` key: a version string, a table, or a list of either.
///
/// Generic over the element so mise can parse straight into its checked tool type: each element
/// then fails with its own span.
#[derive(Debug, Clone)]
pub struct ToolSpecList<T = ToolSpec>(pub Vec<T>);

impl<T> Default for ToolSpecList<T> {
    fn default() -> Self {
        Self(vec![])
    }
}

/// One version of a tool as written.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// The version request exactly as written, still un-rendered. A `:` inside a template
    /// belongs to the template, so mise parses selectors only after rendering.
    pub request: String,
    /// The table's other keys, as written. `None` for the string form; a table always gives
    /// `Some`, even with no options, because mise treats the two forms differently.
    pub options: Option<IndexMap<String, toml::Value>>,
}

impl From<ParsedToolMap> for ToolSpec {
    fn from(parsed: ParsedToolMap) -> Self {
        Self {
            request: parsed.request,
            options: Some(parsed.options),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ToolSpecList<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ToolSpecListVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ToolSpecListVisitor<T> {
            type Value = ToolSpecList<T>;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("tool list")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let tool = T::deserialize(de::value::StrDeserializer::<E>::new(v))?;
                Ok(ToolSpecList(vec![tool]))
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: de::SeqAccess<'de>,
            {
                let mut tools = vec![];
                while let Some(tool) = seq.next_element::<T>()? {
                    tools.push(tool);
                }
                Ok(ToolSpecList(tools))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let tool = T::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(ToolSpecList(vec![tool]))
            }
        }

        deserializer.deserialize_any(ToolSpecListVisitor(std::marker::PhantomData))
    }
}

impl ToolSpec {
    fn string(request: &str) -> Self {
        Self {
            request: request.to_string(),
            options: None,
        }
    }
}

impl<'de> Deserialize<'de> for ToolSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ToolSpecVisitor;

        impl<'de> Visitor<'de> for ToolSpecVisitor {
            type Value = ToolSpec;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("tool definition")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ToolSpec::string(v))
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let parsed =
                    ParsedToolMap::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(parsed.into())
            }
        }

        deserializer.deserialize_any(ToolSpecVisitor)
    }
}

/// The options every backend understands, as typed data. mise fills this in when it checks a
/// tool's options.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct CoreToolOptions {
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(default)]
    pub depends: Option<Vec<String>>,
    #[serde(default)]
    pub install_env: IndexMap<String, EnvValue>,
    #[serde(default)]
    pub lazy: Option<bool>,
    #[serde(default)]
    pub lazy_bins: Vec<String>,
    /// `true`, `false`, or a check interval for background updates. Only
    /// honored in global config; never passed to a backend or stored in an
    /// install manifest.
    #[serde(default)]
    pub auto_update: Option<String>,
}
