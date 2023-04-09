use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use console::style;
use console::Alignment::Left;
use indexmap::IndexMap;
use itertools::Itertools;
use owo_colors::OwoColorize;
use serde_derive::Serialize;
use versions::Versioning;

use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::tool::Tool;
use crate::toolset::{ToolSource, ToolVersion, ToolsetBuilder};

/// List installed and/or currently selected tool versions
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Ls {
    /// Only show tool versions from [PLUGIN]
    #[clap(long, short)]
    plugin: Option<PluginName>,

    #[clap(hide = true)]
    plugin_arg: Option<PluginName>,

    /// Only show tool versions currently specified in a .tool-versions/.rtx.toml
    #[clap(long, short)]
    current: bool,

    /// Only show tool versions that are installed
    /// Hides missing ones defined in .tool-versions/.rtx.toml but not yet installed
    #[clap(long, short)]
    installed: bool,

    /// Output in an easily parseable format
    #[clap(long, hide = true, visible_short_alias = 'x', conflicts_with = "json")]
    parseable: bool,

    /// Output in json format
    #[clap(long)]
    json: bool,
}

impl Command for Ls {
    fn run(mut self, mut config: Config, out: &mut Output) -> Result<()> {
        self.plugin = self.plugin.clone().or(self.plugin_arg.clone());
        self.verify_plugin(&config)?;

        let mut runtimes = get_runtime_list(&mut config, &self.plugin)?;
        if self.current {
            runtimes.retain(|(_, _, source)| source.is_some());
        }
        if self.installed {
            runtimes.retain(|(p, tv, _)| p.is_version_installed(tv));
        }
        if self.json {
            self.display_json(runtimes, out)
        } else if self.parseable {
            self.display_parseable(runtimes, out)
        } else {
            self.display_user(runtimes, out)
        }
    }
}

type JSONOutput = IndexMap<PluginName, Vec<JSONToolVersion>>;

#[derive(Serialize)]
struct JSONToolVersion {
    version: String,
    requested_version: Option<String>,
    install_path: PathBuf,
    source: Option<IndexMap<String, String>>,
}

impl Ls {
    fn verify_plugin(&self, config: &Config) -> Result<()> {
        match &self.plugin {
            Some(plugin_name) => {
                let plugin = config.tools.get(plugin_name);
                if plugin.is_none() || !plugin.unwrap().is_installed() {
                    return Err(PluginNotInstalled(plugin_name.clone()))?;
                }
            }
            None => {}
        }
        Ok(())
    }

    fn display_json(&self, runtimes: Vec<RuntimeRow>, out: &mut Output) -> Result<()> {
        let mut plugins = JSONOutput::new();
        for (plugin_name, runtimes) in &runtimes
            .into_iter()
            .group_by(|(p, _, _)| p.name.to_string())
        {
            let runtimes = runtimes
                .map(|(_, tv, source)| JSONToolVersion {
                    install_path: tv.install_path(),
                    version: tv.version,
                    requested_version: source.as_ref().map(|_| tv.request.version()),
                    source: source.map(|source| source.as_json()),
                })
                .collect();
            if self.plugin.is_some() {
                // only display 1 plugin
                out.stdout.writeln(serde_json::to_string_pretty(&runtimes)?);
                return Ok(());
            }
            plugins.insert(plugin_name.clone(), runtimes);
        }
        out.stdout.writeln(serde_json::to_string_pretty(&plugins)?);
        Ok(())
    }

    fn display_parseable(&self, runtimes: Vec<RuntimeRow>, out: &mut Output) -> Result<()> {
        warn!("The parseable output format is deprecated and will be removed in a future release.");
        warn!("Please use the regular output format instead which has been modified to be more easily parseable.");
        runtimes
            .into_iter()
            .map(|(p, tv, _)| (p, tv))
            .filter(|(p, tv)| p.is_version_installed(tv))
            .for_each(|(_, tv)| {
                if self.plugin.is_some() {
                    // only displaying 1 plugin so only show the version
                    rtxprintln!(out, "{}", tv.version);
                } else {
                    rtxprintln!(out, "{} {}", tv.plugin_name, tv.version);
                }
            });
        Ok(())
    }

    fn display_user(&self, runtimes: Vec<RuntimeRow>, out: &mut Output) -> Result<()> {
        let output = runtimes
            .into_iter()
            .map(|(p, tv, source)| {
                let plugin = p.name.to_string();
                let version = if !p.is_version_installed(&tv) {
                    VersionStatus::Missing(tv.version)
                } else if source.is_some() {
                    VersionStatus::Active(tv.version)
                } else {
                    VersionStatus::Inactive(tv.version)
                };
                let request = source.map(|source| (source.to_string(), tv.request.version()));
                (plugin, version, request)
            })
            .collect::<Vec<_>>();
        let (max_plugin_len, max_version_len, max_source_len) = output.iter().fold(
            (0, 0, 0),
            |(max_plugin, max_version, max_source), (plugin, version, request)| {
                let plugin = max_plugin.max(plugin.len());
                let version = max_version.max(version.to_plain_string().len());
                let source = match request {
                    Some((source, _)) => max_source.max(source.len()),
                    None => max_source,
                };
                (plugin.min(10), version.min(15), source.min(30))
            },
        );
        for (plugin, version, request) in output {
            let pad = |s, len| console::pad_str(s, len, Left, None);
            let plugin_extra = (plugin.len() as i8 - max_plugin_len as i8).max(0) as usize;
            let plugin = pad(&plugin, max_plugin_len);
            let plugin = style(plugin).cyan();
            let version_extra = (version.to_plain_string().len() as i8 - max_version_len as i8
                + plugin_extra as i8)
                .max(0) as usize;
            let version = version.to_string();
            let version = pad(&version, max_version_len - plugin_extra);
            match &request {
                Some((source, requested)) => {
                    let source = pad(source, max_source_len - version_extra);
                    rtxprintln!(out, "{} {} {} {}", plugin, version, source, requested);
                }
                None => {
                    rtxprintln!(out, "{} {}", plugin, version);
                }
            }
        }
        Ok(())
    }
}

type RuntimeRow = (Arc<Tool>, ToolVersion, Option<ToolSource>);

fn get_runtime_list(
    config: &mut Config,
    plugin_flag: &Option<PluginName>,
) -> Result<Vec<RuntimeRow>> {
    let ts = ToolsetBuilder::new().build(config)?;
    let mut versions: HashMap<(PluginName, String), (Arc<Tool>, ToolVersion)> = ts
        .list_installed_versions(config)?
        .into_iter()
        .filter(|(p, _)| match plugin_flag {
            Some(plugin) => &p.name == plugin,
            None => true,
        })
        .map(|(p, tv)| ((p.name.clone(), tv.version.clone()), (p, tv)))
        .collect();

    let active = ts
        .list_current_versions(config)
        .into_iter()
        .map(|(p, tv)| ((p.name.clone(), tv.version.clone()), (p, tv)))
        .collect::<HashMap<(PluginName, String), (Arc<Tool>, ToolVersion)>>();

    versions.extend(
        active
            .clone()
            .into_iter()
            .filter(|((plugin_name, _), _)| match plugin_flag {
                Some(plugin) => plugin_name == plugin,
                None => true,
            })
            .collect::<Vec<((PluginName, String), (Arc<Tool>, ToolVersion))>>(),
    );

    let rvs: Vec<RuntimeRow> = versions
        .into_iter()
        .sorted_by_cached_key(|((plugin_name, version), _)| {
            (plugin_name.clone(), Versioning::new(version))
        })
        .map(|(k, (p, tv))| {
            let source = match &active.get(&k) {
                Some((_, tv)) => ts.versions.get(&tv.plugin_name).map(|tv| tv.source.clone()),
                None => None,
            };
            (p, tv, source)
        })
        .collect();

    Ok(rvs)
}

enum VersionStatus {
    Active(String),
    Inactive(String),
    Missing(String),
}

impl VersionStatus {
    fn to_plain_string(&self) -> String {
        match self {
            VersionStatus::Active(version) => version.to_string(),
            VersionStatus::Inactive(version) => version.to_string(),
            VersionStatus::Missing(version) => format!("{} (missing)", version),
        }
    }
}

impl Display for VersionStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionStatus::Active(version) => write!(f, "{}", style(version).green()),
            VersionStatus::Inactive(version) => write!(f, "{}", style(version).dim()),
            VersionStatus::Missing(version) => write!(
                f,
                "{} {}",
                style(version).strikethrough().red(),
                style("(missing)").red()
            ),
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx ls</bold>
  ⏵  nodejs     18.0.0 (set by ~/src/myapp/.tool-versions)
  ⏵  python     3.11.0 (set by ~/.tool-versions)
     python     3.10.0

  $ <bold>rtx ls --current</bold>
  ⏵  nodejs     18.0.0 (set by ~/src/myapp/.tool-versions)
  ⏵  python     3.11.0 (set by ~/.tool-versions)

  $ <bold>rtx ls --parseable</bold>
  nodejs 18.0.0
  python 3.11.0

  $ <bold>rtx ls --json</bold>
  {
    "nodejs": [
      {
        "version": "18.0.0",
        "install_path": "/Users/jdx/.rtx/installs/nodejs/18.0.0",
        "source": {
          "type": ".rtx.toml",
          "path": "/Users/jdx/.rtx.toml"
        }
      }
    ],
    "python": [...]
  }
"#
);

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::file::remove_all;
    use crate::{assert_cli, assert_cli_err, assert_cli_snapshot, dirs};

    #[test]
    fn test_ls() {
        let _ = remove_all(dirs::INSTALLS.as_path());
        assert_cli!("install");
        assert_cli_snapshot!("list");

        assert_cli!("install", "tiny@2.0.0");
        assert_cli_snapshot!("list");

        assert_cli!("uninstall", "tiny@3.1.0");
        assert_cli_snapshot!("list");

        assert_cli!("uninstall", "tiny@2.0.0");
        assert_cli_snapshot!("list");

        assert_cli!("install");
        assert_cli_snapshot!("list");
    }

    #[test]
    fn test_ls_current() {
        assert_cli_snapshot!("ls", "-c");
    }

    #[test]
    fn test_ls_json() {
        let _ = remove_all(dirs::INSTALLS.as_path());
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--json");
        assert_cli_snapshot!("ls", "--json", "tiny");
    }

    #[test]
    fn test_ls_parseable() {
        let _ = remove_all(dirs::INSTALLS.as_path());
        assert_cli!("install");
        assert_cli_snapshot!("ls", "-x");
        assert_cli_snapshot!("ls", "--parseable", "tiny");
    }

    #[test]
    fn test_ls_missing_plugin() {
        let err = assert_cli_err!("ls", "missing-plugin");
        assert_str_eq!(err.to_string(), r#"[missing-plugin] plugin not installed"#);
    }
}
