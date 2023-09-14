use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use console::style;
use console::Alignment::Left;
use indexmap::IndexMap;
use itertools::Itertools;
use serde_derive::Serialize;
use versions::Versioning;

use crate::cli::command::Command;
use crate::config::Config;
use crate::errors::Error::PluginNotInstalled;
use crate::output::Output;
use crate::plugins::{unalias_plugin, PluginName};
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

    /// Only show tool versions currently specified in a the global .tool-versions/.rtx.toml
    #[clap(long, short)]
    global: bool,

    /// Only show tool versions that are installed
    /// Hides missing ones defined in .tool-versions/.rtx.toml but not yet installed
    #[clap(long, short)]
    installed: bool,

    /// Output in an easily parseable format
    #[clap(long, hide = true, visible_short_alias = 'x', conflicts_with = "json")]
    parseable: bool,

    /// Output in json format
    #[clap(long, visible_short_alias = 'J', overrides_with = "parseable")]
    json: bool,

    /// Display missing tool versions
    #[clap(long, short, conflicts_with = "installed")]
    missing: bool,

    /// Display versions matching this prefix
    #[clap(long)]
    prefix: Option<String>,
}

impl Command for Ls {
    fn run(mut self, mut config: Config, out: &mut Output) -> Result<()> {
        self.plugin = self
            .plugin
            .clone()
            .or(self.plugin_arg.clone())
            .map(|p| PluginName::from(unalias_plugin(&p)));
        self.verify_plugin(&config)?;

        let mut runtimes = self.get_runtime_list(&mut config)?;
        if self.current || self.global {
            // TODO: global is a little weird: it will show global versions as the active ones even if
            // they're overridden locally
            runtimes.retain(|(_, _, source)| source.is_some());
        }
        if self.installed {
            runtimes.retain(|(p, tv, _)| p.is_version_installed(tv));
        }
        if self.missing {
            runtimes.retain(|(p, tv, _)| !p.is_version_installed(tv));
        }
        if let Some(prefix) = &self.prefix {
            if self.plugin.is_none() {
                panic!("--prefix requires --plugin");
            }
            runtimes.retain(|(_, tv, _)| tv.version.starts_with(prefix));
        }
        if self.json {
            self.display_json(runtimes, out)
        } else if self.parseable {
            self.display_parseable(runtimes, out)
        } else {
            self.display_user(&config, runtimes, out)
        }
    }
}

type JSONOutput = IndexMap<PluginName, Vec<JSONToolVersion>>;

#[derive(Serialize)]
struct JSONToolVersion {
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_version: Option<String>,
    install_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symlinked_to: Option<PathBuf>,
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
        if let Some(plugin) = &self.plugin {
            // only runtimes for 1 plugin
            let runtimes: Vec<JSONToolVersion> = runtimes
                .into_iter()
                .filter(|(p, _, _)| plugin.eq(&p.name))
                .map(|row| row.into())
                .collect();
            out.stdout.writeln(serde_json::to_string_pretty(&runtimes)?);
            return Ok(());
        }

        let mut plugins = JSONOutput::new();
        for (plugin_name, runtimes) in &runtimes
            .into_iter()
            .group_by(|(p, _, _)| p.name.to_string())
        {
            let runtimes = runtimes.map(|row| row.into()).collect();
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

    fn display_user(
        &self,
        config: &Config,
        runtimes: Vec<RuntimeRow>,
        out: &mut Output,
    ) -> Result<()> {
        let output = runtimes
            .into_iter()
            .map(|(p, tv, source)| {
                let plugin = p.name.to_string();
                let version = if let Some(symlink_path) = p.symlink_path(&tv) {
                    VersionStatus::Symlink(tv.version, symlink_path, source.is_some())
                } else if !p.is_version_installed(&tv) {
                    VersionStatus::Missing(tv.version)
                } else if source.is_some() {
                    VersionStatus::Active(tv.version.clone(), p.is_version_outdated(config, &tv))
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
            let plugin_extra =
                ((plugin.len() as i8 - max_plugin_len as i8).max(0) as usize).min(max_version_len);
            let plugin = pad(&plugin, max_plugin_len);
            let plugin = style(plugin).cyan();
            let version_extra = (version.to_plain_string().len() as i8 - max_version_len as i8
                + plugin_extra as i8)
                .max(0) as usize;
            let version = version.to_string();
            let version = pad(&version, max_version_len - plugin_extra);
            let line = match &request {
                Some((source, requested)) => {
                    let source = pad(source, max_source_len - version_extra);
                    format!("{} {} {} {}", plugin, version, source, requested)
                }
                None => {
                    format!("{} {}", plugin, version)
                }
            };
            rtxprintln!(out, "{}", line.trim_end());
        }
        Ok(())
    }

    fn get_runtime_list(&self, config: &mut Config) -> Result<Vec<RuntimeRow>> {
        let mut tsb = ToolsetBuilder::new().with_global_only(self.global);

        if let Some(plugin) = &self.plugin {
            tsb = tsb.with_tools(&[plugin]);
            config.tools.retain(|p, _| p == plugin);
        }
        let ts = tsb.build(config)?;
        let mut versions: HashMap<(PluginName, String), (Arc<Tool>, ToolVersion)> = ts
            .list_installed_versions(config)?
            .into_iter()
            .map(|(p, tv)| ((p.name.clone(), tv.version.clone()), (p, tv)))
            .collect();

        let active = ts
            .list_current_versions(config)
            .into_iter()
            .map(|(p, tv)| ((p.name.clone(), tv.version.clone()), (p, tv)))
            .collect::<HashMap<(PluginName, String), (Arc<Tool>, ToolVersion)>>();

        versions.extend(active.clone());

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
            // if it isn't installed and it's not specified, don't show it
            .filter(|(p, tv, source)| source.is_some() || p.is_version_installed(tv))
            .collect();

        Ok(rvs)
    }
}

type RuntimeRow = (Arc<Tool>, ToolVersion, Option<ToolSource>);

impl From<RuntimeRow> for JSONToolVersion {
    fn from(row: RuntimeRow) -> Self {
        let (p, tv, source) = row;
        JSONToolVersion {
            symlinked_to: p.symlink_path(&tv),
            install_path: tv.install_path(),
            version: tv.version,
            requested_version: source.as_ref().map(|_| tv.request.version()),
            source: source.map(|source| source.as_json()),
        }
    }
}

enum VersionStatus {
    Active(String, bool),
    Inactive(String),
    Missing(String),
    Symlink(String, PathBuf, bool),
}

impl VersionStatus {
    fn to_plain_string(&self) -> String {
        match self {
            VersionStatus::Active(version, outdated) => {
                if *outdated {
                    format!("{} (outdated)", version)
                } else {
                    version.to_string()
                }
            }
            VersionStatus::Inactive(version) => version.to_string(),
            VersionStatus::Missing(version) => format!("{} (missing)", version),
            VersionStatus::Symlink(version, _, _) => format!("{} (symlink)", version),
        }
    }
}

impl Display for VersionStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionStatus::Active(version, outdated) => {
                if *outdated {
                    write!(
                        f,
                        "{} {}",
                        style(version).yellow(),
                        style("(outdated)").yellow()
                    )
                } else {
                    write!(f, "{}", style(version).green())
                }
            }
            VersionStatus::Inactive(version) => write!(f, "{}", style(version).dim()),
            VersionStatus::Missing(version) => write!(
                f,
                "{} {}",
                if console::colors_enabled() {
                    style(version).strikethrough().red().to_string()
                } else {
                    version.to_string()
                },
                style("(missing)").red()
            ),
            VersionStatus::Symlink(version, _, active) => {
                write!(
                    f,
                    "{} {}",
                    if *active {
                        style(version)
                    } else {
                        style(version).dim()
                    },
                    style("(symlink)").dim()
                )
            }
        }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx ls</bold>
  node    20.0.0 ~/src/myapp/.tool-versions latest
  python  3.11.0 ~/.tool-versions           3.10
  python  3.10.0

  $ <bold>rtx ls --current</bold>
  node    20.0.0 ~/src/myapp/.tool-versions 20
  python  3.11.0 ~/.tool-versions           3.11.0

  $ <bold>rtx ls --json</bold>
  {
    "node": [
      {
        "version": "20.0.0",
        "install_path": "/Users/jdx/.rtx/installs/node/20.0.0",
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
    fn test_ls_missing() {
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--missing");
    }

    #[test]
    fn test_ls_missing_plugin() {
        let err = assert_cli_err!("ls", "missing-plugin");
        assert_str_eq!(err.to_string(), r#"[missing-plugin] plugin not installed"#);
    }

    #[test]
    fn test_ls_prefix() {
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--plugin=tiny", "--prefix=3");
    }
}
