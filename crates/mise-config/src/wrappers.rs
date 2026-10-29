//! `[wrappers]`: commands that intercept a binary name before delegating to the toolset.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// A command that intercepts a binary name before delegating to the toolset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum CommandWrapper {
    /// command to run
    Command(#[schemars(length(min = 1), regex(pattern = r".*\S.*"))] String),
    Detailed(CommandWrapperOptions),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct CommandWrapperOptions {
    /// command to run
    #[schemars(length(min = 1), regex(pattern = r".*\S.*"))]
    pub command: String,
    /// arguments inserted before the intercepted command's arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// environment variables set for the wrapper
    #[serde(default)]
    #[schemars(with = "IndexMap<String, crate::schema::EnvVarValue>")]
    pub env: IndexMap<String, String>,
}

impl CommandWrapper {
    pub fn command(&self) -> &str {
        match self {
            Self::Command(command) => command,
            Self::Detailed(options) => &options.command,
        }
    }

    pub fn args(&self) -> &[String] {
        match self {
            Self::Command(_) => &[],
            Self::Detailed(options) => &options.args,
        }
    }

    pub fn env(&self) -> &IndexMap<String, String> {
        static EMPTY: std::sync::LazyLock<IndexMap<String, String>> =
            std::sync::LazyLock::new(IndexMap::new);
        match self {
            Self::Command(_) => &EMPTY,
            Self::Detailed(options) => &options.env,
        }
    }
}
