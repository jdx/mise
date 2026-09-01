use indexmap::IndexMap;
use serde::Deserialize;

/// A command that intercepts a binary name before delegating to the toolset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum CommandWrapper {
    Command(String),
    Detailed(CommandWrapperOptions),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CommandWrapperOptions {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: IndexMap<String, String>,
}

impl CommandWrapper {
    pub(crate) fn command(&self) -> &str {
        match self {
            Self::Command(command) => command,
            Self::Detailed(options) => &options.command,
        }
    }

    pub(crate) fn args(&self) -> &[String] {
        match self {
            Self::Command(_) => &[],
            Self::Detailed(options) => &options.args,
        }
    }

    pub(crate) fn env(&self) -> &IndexMap<String, String> {
        static EMPTY: std::sync::LazyLock<IndexMap<String, String>> =
            std::sync::LazyLock::new(IndexMap::new);
        match self {
            Self::Command(_) => &EMPTY,
            Self::Detailed(options) => &options.env,
        }
    }
}
