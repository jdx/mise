use crate::config::SettingsExt;
use indexmap::IndexMap;

pub use mise_config::wrappers::{CommandWrapper, CommandWrapperOptions};

/// Rust's opt-in wrapper shares the same dispatch path as explicit wrappers.
/// Inspect effective requests so project, platform, and runtime selection apply.
pub(crate) fn add_rust_wrapper<'a>(
    wrappers: &mut IndexMap<String, CommandWrapper>,
    tools: impl IntoIterator<Item = &'a crate::toolset::ToolRequest>,
) -> eyre::Result<()> {
    use crate::args::BackendArg;
    use crate::backend::options::BackendOptions;
    use crate::toolset::ToolOptionSource;

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
            // The source only says who picked the version. Options are also layered
            // on from elsewhere (a project's tool alias, the project entry a runtime
            // argument inherits), so the entry itself has to set this one.
            let safe = !crate::config::Settings::safe_mode()
                || (rust
                    .source()
                    .path()
                    .is_none_or(crate::config::is_global_config)
                    && matches!(
                        rust.option_source("mr_boxington"),
                        Some(ToolOptionSource::Request | ToolOptionSource::InlineBackendArg)
                    ));
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
