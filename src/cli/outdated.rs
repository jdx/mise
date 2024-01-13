use std::collections::HashSet;
use std::sync::Arc;

use console::{pad_str, style, Alignment};
use eyre::Result;

use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::forge::Forge;
use crate::toolset::{ToolVersion, ToolsetBuilder};

/// Shows outdated tool versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Outdated {
    /// Tool(s) to show outdated versions for
    /// e.g.: node@20 python@3.10
    /// If not specified, all tools in global and local configs will be shown
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    pub tool: Vec<ToolArg>,
}

impl Outdated {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(&config)?;
        let tool_set = self
            .tool
            .iter()
            .map(|t| t.forge.clone())
            .collect::<HashSet<_>>();
        ts.versions
            .retain(|_, tvl| tool_set.is_empty() || tool_set.contains(&tvl.forge));
        let outdated = ts.list_outdated_versions();
        if outdated.is_empty() {
            info!("All tools are up to date");
        } else {
            self.display(outdated);
        }

        Ok(())
    }

    fn display(&self, outdated: OutputVec) {
        // TODO: make a generic table printer in src/ui/table
        let plugins = outdated.iter().map(|(t, _, _)| t.id()).collect::<Vec<_>>();
        let requests = outdated
            .iter()
            .map(|(_, tv, _)| tv.request.version())
            .collect::<Vec<_>>();
        let currents = outdated
            .iter()
            .map(|(t, tv, _)| {
                if t.is_version_installed(tv) {
                    tv.version.clone()
                } else {
                    "MISSING".to_string()
                }
            })
            .collect::<Vec<_>>();
        let latests = outdated
            .iter()
            .map(|(_, _, c)| c.clone())
            .collect::<Vec<_>>();
        let plugin_width = plugins
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or_default()
            .max(6)
            + 1;
        let requested_width = requests
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or_default()
            .max(9)
            + 1;
        let current_width = currents
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or_default()
            .max(7)
            + 1;
        let pad_plugin = |s| pad_str(s, plugin_width, Alignment::Left, None);
        let pad_requested = |s| pad_str(s, requested_width, Alignment::Left, None);
        let pad_current = |s| pad_str(s, current_width, Alignment::Left, None);
        miseprintln!(
            "{} {} {} {}",
            style(pad_plugin("Tool")).dim(),
            style(pad_requested("Requested")).dim(),
            style(pad_current("Current")).dim(),
            style("Latest").dim(),
        );
        for i in 0..outdated.len() {
            miseprintln!(
                "{} {} {} {}",
                pad_plugin(plugins[i]),
                pad_requested(&requests[i]),
                pad_current(&currents[i]),
                latests[i]
            );
        }
    }
}

type OutputVec = Vec<(Arc<dyn Forge>, ToolVersion, String)>;

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise outdated</bold>
  Plugin  Requested  Current  Latest
  python  3.11       3.11.0   3.11.1
  node    20         20.0.0   20.1.0

  $ <bold>mise outdated node</bold>
  Plugin  Requested  Current  Latest
  node    20         20.0.0   20.1.0
"#
);

#[cfg(test)]
mod tests {
    use std::env;

    #[test]
    fn test_current() {
        assert_cli_snapshot!("outdated");
    }

    #[test]
    fn test_current_with_runtimes() {
        assert_cli_snapshot!("outdated", "tiny");
    }
}
