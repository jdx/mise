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

/// Rust's opt-in wrapper shares the same dispatch path as explicit wrappers.
/// Inspect effective requests so project, platform, and runtime selection apply.
pub(crate) fn add_rust_wrapper<'a>(
    wrappers: &mut IndexMap<String, CommandWrapper>,
    tools: impl IntoIterator<Item = &'a crate::toolset::ToolRequest>,
) -> eyre::Result<()> {
    use crate::backend::options::BackendOptions;
    use crate::cli::args::BackendArg;

    if wrappers
        .keys()
        .any(|name| crate::shims::command_names_eq(name, "cargo"))
    {
        return Ok(());
    }
    let tools: Vec<_> = tools
        .into_iter()
        .filter(|tool| tool.is_os_supported())
        .collect();
    let enabled = tools
        .iter()
        .find(|tool| tool.ba().full() == "core:rust")
        .is_some_and(|rust| {
            // Match explicit wrappers: safe mode cannot activate project commands.
            let safe = !crate::config::Settings::safe_mode()
                || rust
                    .source()
                    .path()
                    .is_none_or(crate::config::is_global_config);
            safe && BackendOptions::new(&rust.options()).bool("mr_boxington")
        });
    if !enabled {
        return Ok(());
    }
    let mbx = BackendArg::from("mr-boxington");
    if !tools
        .iter()
        .any(|tool| tool.ba().short == mbx.short || tool.ba().full() == mbx.full())
    {
        eyre::bail!(
            "rust's mr_boxington option requires mr-boxington in [tools]; add `mr-boxington = \"latest\"`"
        );
    }
    wrappers.insert(
        "cargo".into(),
        CommandWrapper::Detailed(CommandWrapperOptions {
            command: "mbx".into(),
            args: Vec::new(),
            env: [("MBX_CARGO_SHIM_MODE".into(), "1".into())].into(),
        }),
    );
    Ok(())
}
