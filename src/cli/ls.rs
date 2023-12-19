use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use console::style;
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use serde_derive::Serialize;

use tabled::{Table, Tabled};
use versions::Versioning;

use crate::config::Config;
use crate::errors::Error::PluginNotInstalled;
use crate::plugins::{unalias_plugin, Plugin, PluginName};
use crate::toolset::{ToolSource, ToolVersion, ToolsetBuilder};
use crate::ui::table;

/// List installed and/or currently selected tool versions
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Ls {
    /// Only show tool versions from [PLUGIN]
    #[clap(conflicts_with = "plugin_flag")]
    plugin: Option<Vec<String>>,

    #[clap(long = "plugin", short, hide = true)]
    plugin_flag: Option<String>,

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
    #[clap(long, hide = true, conflicts_with = "json")]
    parseable: bool,

    /// Output in json format
    #[clap(long, short = 'J', overrides_with = "parseable")]
    json: bool,

    /// Display missing tool versions
    #[clap(long, short, conflicts_with = "installed")]
    missing: bool,

    /// Display versions matching this prefix
    #[clap(long, requires = "plugin")]
    prefix: Option<String>,

    /// Don't display headers
    #[clap(long, alias="no-headers", verbatim_doc_comment, conflicts_with_all = &["json", "parseable"])]
    no_header: bool,
}

impl Ls {
    pub fn run(mut self, config: &Config) -> Result<()> {
        self.plugin = self
            .plugin
            .or_else(|| self.plugin_flag.clone().map(|p| vec![p]))
            .map(|p| p.into_iter().map(|p| unalias_plugin(&p).into()).collect());
        self.verify_plugin(config)?;

        let mut runtimes = self.get_runtime_list(config)?;
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
            runtimes.retain(|(_, tv, _)| tv.version.starts_with(prefix));
        }
        if self.json {
            self.display_json(runtimes)
        } else if self.parseable {
            self.display_parseable(runtimes)
        } else {
            self.display_user(runtimes)
        }
    }

    fn verify_plugin(&self, config: &Config) -> Result<()> {
        match &self.plugin {
            Some(plugins) => {
                for plugin_name in plugins {
                    let plugin = config.get_or_create_plugin(plugin_name);
                    if !plugin.is_installed() {
                        return Err(PluginNotInstalled(plugin_name.clone()))?;
                    }
                }
            }
            None => {}
        }
        Ok(())
    }

    fn display_json(&self, runtimes: Vec<RuntimeRow>) -> Result<()> {
        if let Some(plugins) = &self.plugin {
            // only runtimes for 1 plugin
            let runtimes: Vec<JSONToolVersion> = runtimes
                .into_iter()
                .filter(|(p, _, _)| plugins.contains(&p.name().to_string()))
                .map(|row| row.into())
                .collect();
            rtxprintln!("{}", serde_json::to_string_pretty(&runtimes)?);
            return Ok(());
        }

        let mut plugins = JSONOutput::new();
        for (plugin_name, runtimes) in &runtimes
            .into_iter()
            .group_by(|(p, _, _)| p.name().to_string())
        {
            let runtimes = runtimes.map(|row| row.into()).collect();
            plugins.insert(plugin_name.clone(), runtimes);
        }
        rtxprintln!("{}", serde_json::to_string_pretty(&plugins)?);
        Ok(())
    }

    fn display_parseable(&self, runtimes: Vec<RuntimeRow>) -> Result<()> {
        warn!("The parseable output format is deprecated and will be removed in a future release.");
        warn!("Please use the regular output format instead which has been modified to be more easily parseable.");
        runtimes
            .into_iter()
            .map(|(p, tv, _)| (p, tv))
            .filter(|(p, tv)| p.is_version_installed(tv))
            .for_each(|(_, tv)| {
                if self.plugin.is_some() {
                    // only displaying 1 plugin so only show the version
                    rtxprintln!("{}", tv.version);
                } else {
                    rtxprintln!("{} {}", tv.plugin_name, tv.version);
                }
            });
        Ok(())
    }

    fn display_user(&self, runtimes: Vec<RuntimeRow>) -> Result<()> {
        // let data = runtimes
        //     .into_iter()
        //     .map(|(plugin, tv, source)| (plugin.to_string(), tv.to_string()))
        //     .collect_vec();
        let rows = runtimes.into_iter().map(|(p, tv, source)| Row {
            plugin: p.clone(),
            version: (p.as_ref(), &tv, &source).into(),
            requested: match source.is_some() {
                true => Some(tv.request.version()),
                false => None,
            },
            source,
        });
        let mut table = Table::new(rows);
        table::default_style(&mut table, self.no_header);
        rtxprintln!("{}", table.to_string());
        Ok(())
    }

    fn get_runtime_list(&self, config: &Config) -> Result<Vec<RuntimeRow>> {
        let mut tsb = ToolsetBuilder::new().with_global_only(self.global);

        if let Some(plugins) = &self.plugin {
            let plugins = plugins.iter().map(|p| p.as_str()).collect_vec();
            tsb = tsb.with_tools(&plugins);
        }
        let ts = tsb.build(config)?;
        let mut versions: HashMap<(String, String), (Arc<dyn Plugin>, ToolVersion)> = ts
            .list_installed_versions(config)?
            .into_iter()
            .map(|(p, tv)| ((p.name().into(), tv.version.clone()), (p, tv)))
            .collect();

        let active = ts
            .list_current_versions()
            .into_iter()
            .map(|(p, tv)| ((p.name().into(), tv.version.clone()), (p, tv)))
            .collect::<HashMap<(String, String), (Arc<dyn Plugin>, ToolVersion)>>();

        versions.extend(active.clone());

        let rvs: Vec<RuntimeRow> = versions
            .into_iter()
            .filter(|((plugin_name, _), _)| match &self.plugin {
                Some(p) => p.contains(plugin_name),
                None => true,
            })
            .sorted_by_cached_key(|((plugin_name, version), _)| {
                (
                    plugin_name.clone(),
                    Versioning::new(version),
                    version.clone(),
                )
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

type RuntimeRow = (Arc<dyn Plugin>, ToolVersion, Option<ToolSource>);

#[derive(Tabled)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    #[tabled(display_with = "Self::display_plugin")]
    plugin: Arc<dyn Plugin>,
    version: VersionStatus,
    #[tabled(rename = "Config Source", display_with = "Self::display_source")]
    source: Option<ToolSource>,
    #[tabled(display_with = "Self::display_option")]
    requested: Option<String>,
}

impl Row {
    fn display_option(arg: &Option<String>) -> String {
        match arg {
            Some(s) => s.clone(),
            None => String::new(),
        }
    }
    fn display_plugin(plugin: &Arc<dyn Plugin>) -> String {
        style(plugin).blue().to_string()
    }
    fn display_source(source: &Option<ToolSource>) -> String {
        match source {
            Some(source) => source.to_string(),
            None => String::new(),
        }
    }
}

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

impl From<(&dyn Plugin, &ToolVersion, &Option<ToolSource>)> for VersionStatus {
    fn from((p, tv, source): (&dyn Plugin, &ToolVersion, &Option<ToolSource>)) -> Self {
        if let Some(symlink_path) = p.symlink_path(tv) {
            VersionStatus::Symlink(tv.version.clone(), symlink_path, source.is_some())
        } else if !p.is_version_installed(tv) {
            VersionStatus::Missing(tv.version.clone())
        } else if source.is_some() {
            VersionStatus::Active(tv.version.clone(), p.is_version_outdated(tv, p))
        } else {
            VersionStatus::Inactive(tv.version.clone())
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
                style(version).strikethrough().red(),
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

    use crate::dirs;
    use crate::file::remove_all;

    #[test]
    fn test_ls() {
        let _ = remove_all(dirs::INSTALLS.as_path());
        assert_cli!("install");
        assert_cli_snapshot!("list", @r###"
        dummy  ref:master  ~/.test-tool-versions     ref:master
        tiny   3.1.0       ~/cwd/.test-tool-versions 3
        "###);

        assert_cli!("install", "tiny@2.0.0");
        assert_cli_snapshot!("list", @r###"
        dummy  ref:master  ~/.test-tool-versions     ref:master
        tiny   2.0.0                                           
        tiny   3.1.0       ~/cwd/.test-tool-versions 3
        "###);

        assert_cli!("uninstall", "tiny@3.1.0");
        assert_cli_snapshot!("list", @r###"
        dummy  ref:master       ~/.test-tool-versions     ref:master
        tiny   2.0.0                                                
        tiny   3.1.0 (missing)  ~/cwd/.test-tool-versions 3
        "###);

        assert_cli!("uninstall", "tiny@2.0.0");
        assert_cli_snapshot!("list", @r###"
        dummy  ref:master       ~/.test-tool-versions     ref:master
        tiny   3.1.0 (missing)  ~/cwd/.test-tool-versions 3
        "###);

        assert_cli!("install");
        assert_cli_snapshot!("list", @r###"
        dummy  ref:master  ~/.test-tool-versions     ref:master
        tiny   3.1.0       ~/cwd/.test-tool-versions 3
        "###);
    }

    #[test]
    fn test_ls_current() {
        assert_cli_snapshot!("ls", "-c", @r###"
        dummy  ref:master  ~/.test-tool-versions     ref:master
        tiny   3.1.0       ~/cwd/.test-tool-versions 3
        "###);
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
        assert_cli_snapshot!("ls", "--parseable", @r###"
        dummy ref:master
        tiny 3.1.0
        "###);
        assert_cli_snapshot!("ls", "--parseable", "tiny", @"3.1.0");
    }

    #[test]
    fn test_ls_missing() {
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--missing", @"");
    }

    #[test]
    fn test_ls_missing_plugin() {
        let err = assert_cli_err!("ls", "missing-plugin");
        assert_str_eq!(err.to_string(), r#"[missing-plugin] plugin not installed"#);
    }

    #[test]
    fn test_ls_prefix() {
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--plugin=tiny", "--prefix=3", @"tiny  3.1.0  ~/cwd/.test-tool-versions 3");
    }
}
