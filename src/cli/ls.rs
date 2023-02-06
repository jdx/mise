use atty::Stream::Stdout;
use std::cmp::max;
use std::collections::HashMap;
use std::sync::Arc;

use color_eyre::eyre::Result;
use itertools::Itertools;
use owo_colors::{OwoColorize, Stream};
use versions::Mess;

use crate::cli::command::Command;
use crate::config::{Config, PluginSource};
use crate::output::Output;
use crate::plugins::PluginName;
use crate::runtimes::RuntimeVersion;
use crate::ui::color::{cyan, dimmed, green, red};

/// list installed runtime versions
///
/// The "arrow (->)" indicates the runtime is installed, active, and will be used for running commands.
/// (Assuming `rtx activate` or `rtx env` is in use).
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Ls {
    /// Only show runtimes from [PLUGIN]
    #[clap(long, short)]
    plugin: Option<PluginName>,

    /// Only show runtimes currently specified in .tool-versions
    #[clap(long, short)]
    current: bool,
}

impl Command for Ls {
    fn run(self, config: Config, out: &mut Output) -> Result<()> {
        for (rtv, source) in get_runtime_list(&config, &self.plugin)? {
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
        rtv.version
            .if_supports_color(Stream::Stdout, |t| t.strikethrough().red().to_string())
            .to_string()
            + red(Stdout, " (missing)").as_str()
    } else if active {
        green(Stdout, &rtv.version)
    } else {
        dimmed(Stdout, &rtv.version)
    };
    let unstyled = if missing {
        format!("{} {} (missing)", &rtv.plugin.name, &rtv.version)
    } else {
        format!("{} {}", &rtv.plugin.name, &rtv.version)
    };

    let pad = max(0, 25 - unstyled.len() as isize) as usize;
    format!(
        "{} {}{}",
        cyan(Stdout, &rtv.plugin.name),
        styled,
        " ".repeat(pad)
    )
}

fn get_runtime_list(
    config: &Config,
    plugin_flag: &Option<PluginName>,
) -> Result<Vec<(Arc<RuntimeVersion>, Option<PluginSource>)>> {
    let mut versions: HashMap<(PluginName, String), Arc<RuntimeVersion>> = config
        .ts
        .list_installed_versions()
        .into_iter()
        .filter(|rtv| match plugin_flag {
            Some(plugin) => rtv.plugin.name == *plugin,
            None => true,
        })
        .map(|rtv| ((rtv.plugin.name.clone(), rtv.version.clone()), rtv))
        .collect();

    let active = config
        .ts
        .list_current_versions()
        .into_iter()
        .map(|rtv| ((rtv.plugin.name.clone(), rtv.version.clone()), rtv.clone()))
        .collect::<HashMap<(PluginName, String), Arc<RuntimeVersion>>>();

    versions.extend(
        active
            .clone()
            .into_iter()
            .filter(|((plugin_name, _), _)| match plugin_flag {
                Some(plugin) => plugin_name == plugin,
                None => true,
            })
            .collect::<Vec<((PluginName, String), Arc<RuntimeVersion>)>>(),
    );

    let rvs: Vec<(Arc<RuntimeVersion>, Option<PluginSource>)> = versions
        .into_iter()
        .sorted_by_cached_key(|((plugin_name, version), _)| {
            (plugin_name.clone(), Mess::new(version).unwrap_or_default())
        })
        .map(|(k, rtv)| {
            let source = match &active.get(&k) {
                Some(rtv) => config.ts.get_source_for_plugin(&rtv.plugin.name),
                None => None,
            };
            (rtv, source)
        })
        .collect();

    Ok(rvs)
}

const AFTER_LONG_HELP: &str = r#"
Examples:
  $ rtx list
  -> nodejs     20.0.0 (set by ~/src/myapp/.tool-versions)
  -> python     3.11.0 (set by ~/.tool-versions)
     python     3.10.0
     
  $ rtx list --current
  -> nodejs     20.0.0 (set by ~/src/myapp/.tool-versions)
  -> python     3.11.0 (set by ~/.tool-versions)
"#;

#[cfg(test)]
mod test {
    use regex::Regex;

    use crate::assert_cli;

    #[test]
    fn test_list() {
        assert_cli!("install");
        assert_cli!("install", "shfmt@3.5.0");
        let stdout = assert_cli!("list");
        let re = Regex::new(r"-> shellcheck\s+0\.9\.0\s+").unwrap();
        assert!(re.is_match(&stdout));
        let re = Regex::new(r" {3}shfmt\s+3\.5\.0\s+").unwrap();
        assert!(re.is_match(&stdout));

        assert_cli!("uninstall", "shfmt@3.5.2");
        let stdout = assert_cli!("list");
        let re = Regex::new(r" {3}shfmt\s+3\.5\.2 \(missing\)\s+").unwrap();
        assert!(re.is_match(&stdout));
    }
}
