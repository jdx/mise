use crate::cli::args::tool::ToolArg;
use crate::toolset::ToolVersion;
use console::style;

pub fn style_tv(tv: &ToolVersion) -> String {
    format!(
        "{}{}",
        style(&tv.plugin_name).bright().for_stderr(),
        style(&format!("@{}", &tv.version)).dim().for_stderr()
    )
}
pub fn style_tool(tool: &ToolArg) -> String {
    let version = tool
        .tvr
        .as_ref()
        .map(|t| t.version())
        .unwrap_or(String::from("latest"));
    format!(
        "{}{}",
        style(&tool.plugin).bright().for_stderr(),
        style(&format!("@{version}",)).dim().for_stderr()
    )
}
