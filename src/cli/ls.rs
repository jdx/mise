use std::cmp::max;
use std::collections::HashMap;

use color_eyre::eyre::Result;
use console::style;
use indoc::formatdoc;
use itertools::Itertools;
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use versions::Versioning;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::runtimes::RuntimeVersion;
use crate::toolset::{ToolSource, ToolsetBuilder};

/// list installed runtime versions
///
/// The "arrow (->)" indicates the runtime is installed, active, and will be used for running commands.
/// (Assuming `rtx activate` or `rtx env` is in use).
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Ls {
    /// Only show runtimes from [PLUGIN]
    #[clap(long, short)]
    plugin: Option<PluginName>,

    #[clap(hide = true)]
    plugin_arg: Option<PluginName>,

    /// Only show runtimes currently specified in .tool-versions
    #[clap(long, short)]
    current: bool,
}

impl Command for Ls {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        let plugin = self.plugin.or(self.plugin_arg);
        for (rtv, source) in get_runtime_list(&config, &plugin)? {
            if self.current && source.is_none() {
                continue;
            }
            rtxprintln!(
                out,
                "{} {} {}",
                match rtv.is_installed() && source.is_some() {
                    true => "->",
                    false => "  ",
                },
                styled_version(&rtv, !rtv.is_installed(), source.is_some()),
                match source {
                    Some(source) => format!("(set by {source})"),
                    None => "".into(),
                },
            );
        }

        Ok(())
    }
}

fn styled_version(rtv: &RuntimeVersion, missing: bool, active: bool) -> String {
    let styled = if missing {
        style(&rtv.version).strikethrough().red().to_string()
            + style(" (missing)").red().to_string().as_str()
    } else if active {
        style(&rtv.version).green().to_string()
    } else {
        style(&rtv.version).dim().to_string()
    };
    let unstyled = if missing {
        format!("{} {} (missing)", &rtv.plugin.name, &rtv.version)
    } else {
        format!("{} {}", &rtv.plugin.name, &rtv.version)
    };

    let pad = max(0, 25 - unstyled.len() as isize) as usize;
    format!(
        "{} {}{}",
        style(&rtv.plugin.name).cyan(),
        styled,
        " ".repeat(pad)
    )
}

fn get_runtime_list(
    config: &Config,
    plugin_flag: &Option<PluginName>,
) -> Result<Vec<(RuntimeVersion, Option<ToolSource>)>> {
    let ts = ToolsetBuilder::new().build(config);
    let mut versions: HashMap<(PluginName, String), RuntimeVersion> = ts
        .list_installed_versions()?
        .into_iter()
        .filter(|rtv| match plugin_flag {
            Some(plugin) => rtv.plugin.name == *plugin,
            None => true,
        })
        .map(|rtv| ((rtv.plugin.name.clone(), rtv.version.clone()), rtv))
        .collect();

    let active = ts
        .list_current_versions()
        .into_iter()
        .map(|rtv| ((rtv.plugin.name.clone(), rtv.version.clone()), rtv.clone()))
        .collect::<HashMap<(PluginName, String), RuntimeVersion>>();

    versions.extend(
        active
            .clone()
            .into_iter()
            .filter(|((plugin_name, _), _)| match plugin_flag {
                Some(plugin) => plugin_name == plugin,
                None => true,
            })
            .collect::<Vec<((PluginName, String), RuntimeVersion)>>(),
    );

    let rvs: Vec<(RuntimeVersion, Option<ToolSource>)> = versions
        .into_iter()
        .sorted_by_cached_key(|((plugin_name, version), _)| {
            (plugin_name.clone(), Versioning::new(version))
        })
        .map(|(k, rtv)| {
            let source = match &active.get(&k) {
                Some(rtv) => ts
                    .versions
                    .get(&rtv.plugin.name)
                    .map(|tv| tv.source.clone()),
                None => None,
            };
            (rtv, source)
        })
        .collect();

    Ok(rvs)
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx list
      -> nodejs     20.0.0 (set by ~/src/myapp/.tool-versions)
      -> python     3.11.0 (set by ~/.tool-versions)
         python     3.10.0

      $ rtx list --current
      -> nodejs     20.0.0 (set by ~/src/myapp/.tool-versions)
      -> python     3.11.0 (set by ~/.tool-versions)
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {

    use crate::assert_cli;

    #[test]
    fn test_list() {
        assert_cli!("install");
        assert_cli!("install", "shfmt@3.5.0");
        let stdout = assert_cli!("list");
        let re = regex!(r"-> shellcheck\s+0\.9\.0\s+");
        assert!(re.is_match(&stdout));
        let re = regex!(r" {3}shfmt\s+3\.5\.0\s+");
        assert!(re.is_match(&stdout));

        assert_cli!("uninstall", "shfmt@3.5.1");
        let stdout = assert_cli!("list");
        let re = regex!(r" {3}shfmt\s+3\.5\.1 \(missing\)\s+");
        assert!(re.is_match(&stdout));
    }
}
