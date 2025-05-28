use comfy_table::{Attribute, Cell, Color};
use eyre::{Result, ensure};
use indexmap::IndexMap;
use itertools::Itertools;
use serde_derive::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use versions::Versioning;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cli::prune;
use crate::config;
use crate::config::Config;
use crate::toolset::{ToolSource, ToolVersion, Toolset};
use crate::ui::table::MiseTable;

/// List installed and active tool versions
///
/// This command lists tools that mise "knows about".
/// These may be tools that are currently installed, or those
/// that are in a config file (active) but may or may not be installed.
///
/// It's a useful command to get the current state of your tools.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Ls {
    /// Only show tool versions from [TOOL]
    #[clap(conflicts_with = "tool_flag")]
    installed_tool: Option<Vec<BackendArg>>,

    #[clap(long = "plugin", short = 'p', hide = true)]
    tool_flag: Option<BackendArg>,

    /// Only show tool versions currently specified in a mise.toml
    #[clap(long, short)]
    current: bool,

    /// Only show tool versions currently specified in the global mise.toml
    #[clap(long, short, conflicts_with = "local")]
    global: bool,

    /// Only show tool versions currently specified in the local mise.toml
    #[clap(long, short, conflicts_with = "global")]
    local: bool,

    /// Only show tool versions that are installed
    /// (Hides tools defined in mise.toml but not installed)
    #[clap(long, short)]
    installed: bool,

    /// Don't fetch information such as outdated versions
    #[clap(long, short, hide = true)]
    offline: bool,

    /// Display whether a version is outdated
    #[clap(long)]
    outdated: bool,

    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Display missing tool versions
    #[clap(long, short, conflicts_with = "installed")]
    missing: bool,

    /// Display versions matching this prefix
    #[clap(long, requires = "installed_tool")]
    prefix: Option<String>,

    /// List only tools that can be pruned with `mise prune`
    #[clap(long)]
    prunable: bool,

    /// Don't display headers
    #[clap(long, alias = "no-headers", verbatim_doc_comment, conflicts_with_all = &["json"])]
    no_header: bool,
}

impl Ls {
    pub async fn run(mut self) -> Result<()> {
        let config = Config::get().await?;
        self.installed_tool = self
            .installed_tool
            .or_else(|| self.tool_flag.clone().map(|p| vec![p]));
        self.verify_plugin()?;

        let mut runtimes = if self.prunable {
            self.get_prunable_runtime_list(&config).await?
        } else {
            self.get_runtime_list(&config).await?
        };
        if self.current || self.global || self.local {
            // TODO: global is a little weird: it will show global versions as the active ones even if
            // they're overridden locally
            runtimes.retain(|(_, _, _, source)| !source.is_unknown());
        }
        if self.installed {
            let mut installed_runtimes = vec![];
            for (ls, p, tv, source) in runtimes {
                if p.is_version_installed(&config, &tv, true) {
                    installed_runtimes.push((ls, p, tv, source));
                }
            }
            runtimes = installed_runtimes;
        }
        if self.missing {
            let mut missing_runtimes = vec![];
            for (ls, p, tv, source) in runtimes {
                if !p.is_version_installed(&config, &tv, true) {
                    missing_runtimes.push((ls, p, tv, source));
                }
            }
            runtimes = missing_runtimes;
        }
        if let Some(prefix) = &self.prefix {
            runtimes.retain(|(_, _, tv, _)| tv.version.starts_with(prefix));
        }
        if self.json {
            self.display_json(&config, runtimes).await
        } else {
            self.display_user(&config, runtimes).await
        }
    }

    fn verify_plugin(&self) -> Result<()> {
        if let Some(plugins) = &self.installed_tool {
            for ba in plugins {
                if let Some(plugin) = ba.backend()?.plugin() {
                    ensure!(plugin.is_installed(), "{ba} is not installed");
                }
            }
        }
        Ok(())
    }

    async fn display_json(
        &self,
        config: &Arc<Config>,
        runtimes: Vec<RuntimeRow<'_>>,
    ) -> Result<()> {
        if let Some(plugins) = &self.installed_tool {
            // only runtimes for 1 plugin
            let runtimes: Vec<RuntimeRow<'_>> = runtimes
                .into_iter()
                .filter(|(_, p, _, _)| plugins.contains(p.ba()))
                .collect();
            let mut r = vec![];
            for row in runtimes {
                r.push(json_tool_version_from(config, row).await);
            }
            miseprintln!("{}", serde_json::to_string_pretty(&r)?);
            return Ok(());
        }

        let mut plugins = JSONOutput::new();
        for (plugin_name, runtimes) in &runtimes
            .into_iter()
            .chunk_by(|(_, p, _, _)| p.id().to_string())
        {
            let mut r = vec![];
            for (ls, p, tv, source) in runtimes {
                r.push(json_tool_version_from(config, (ls, p, tv, source)).await);
            }
            plugins.insert(plugin_name.clone(), r);
        }
        miseprintln!("{}", serde_json::to_string_pretty(&plugins)?);
        Ok(())
    }

    async fn display_user<'a>(
        &'a self,
        config: &Arc<Config>,
        runtimes: Vec<RuntimeRow<'a>>,
    ) -> Result<()> {
        let mut rows = vec![];
        for (ls, p, tv, source) in runtimes {
            rows.push(Row {
                tool: p.clone(),
                version: version_status_from(config, (ls, p.as_ref(), &tv, &source)).await,
                requested: match source.is_unknown() {
                    true => None,
                    false => Some(tv.request.version()),
                },
                source: if source.is_unknown() {
                    None
                } else {
                    Some(source)
                },
            });
        }
        let mut table = MiseTable::new(self.no_header, &["Tool", "Version", "Source", "Requested"]);
        for r in rows {
            let row = vec![
                r.display_tool(),
                r.display_version(),
                r.display_source(),
                r.display_requested(),
            ];
            table.add_row(row);
        }
        table.truncate(true).print()
    }

    async fn get_prunable_runtime_list(&self, config: &Arc<Config>) -> Result<Vec<RuntimeRow>> {
        let installed_tool = self.installed_tool.clone().unwrap_or_default();
        Ok(
            prune::prunable_tools(config, installed_tool.iter().collect())
                .await?
                .into_iter()
                .map(|(p, tv)| (self, p, tv, ToolSource::Unknown))
                .collect(),
        )
    }
    async fn get_runtime_list(&self, config: &Arc<Config>) -> Result<Vec<RuntimeRow>> {
        let mut trs = config.get_tool_request_set().await?.clone();
        if self.global {
            trs = trs
                .iter()
                .filter(|(.., ts)| match ts {
                    ToolSource::MiseToml(p) => config::is_global_config(p),
                    _ => false,
                })
                .map(|(fa, tv, ts)| (fa.clone(), tv.clone(), ts.clone()))
                .collect()
        } else if self.local {
            trs = trs
                .iter()
                .filter(|(.., ts)| match ts {
                    ToolSource::MiseToml(p) => !config::is_global_config(p),
                    _ => false,
                })
                .map(|(fa, tv, ts)| (fa.clone(), tv.clone(), ts.clone()))
                .collect()
        }

        let mut ts = Toolset::from(trs);
        ts.resolve(config).await?;

        let rvs: Vec<RuntimeRow> = ts
            .list_all_versions(config)
            .await?
            .into_iter()
            .map(|(b, tv)| ((b, tv.version.clone()), tv))
            .filter(|((b, _), _)| match &self.installed_tool {
                Some(p) => p.contains(b.ba()),
                None => true,
            })
            .sorted_by_cached_key(|((plugin_name, version), _)| {
                (
                    plugin_name.clone(),
                    Versioning::new(version),
                    version.clone(),
                )
            })
            .map(|(k, tv)| (self, k.0, tv.clone(), tv.request.source().clone()))
            // if it isn't installed and it's not specified, don't show it
            .filter(|(_ls, p, tv, source)| {
                !source.is_unknown() || p.is_version_installed(config, tv, true)
            })
            .filter(|(_ls, p, _, _)| match &self.installed_tool {
                Some(backend) => backend.contains(p.ba()),
                None => true,
            })
            .collect();

        Ok(rvs)
    }
}

type JSONOutput = IndexMap<String, Vec<JSONToolVersion>>;

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
    installed: bool,
    active: bool,
}

type RuntimeRow<'a> = (&'a Ls, Arc<dyn Backend>, ToolVersion, ToolSource);

struct Row {
    tool: Arc<dyn Backend>,
    version: VersionStatus,
    source: Option<ToolSource>,
    requested: Option<String>,
}

impl Row {
    fn display_tool(&self) -> Cell {
        Cell::new(&self.tool).fg(Color::Blue)
    }
    fn display_version(&self) -> Cell {
        match &self.version {
            VersionStatus::Active(version, outdated) => {
                if *outdated {
                    Cell::new(format!("{version} (outdated)"))
                        .fg(Color::Yellow)
                        .add_attribute(Attribute::Bold)
                } else {
                    Cell::new(version).fg(Color::Green)
                }
            }
            VersionStatus::Inactive(version) => Cell::new(version).add_attribute(Attribute::Dim),
            VersionStatus::Missing(version) => Cell::new(format!("{version} (missing)"))
                .fg(Color::Red)
                .add_attribute(Attribute::CrossedOut),
            VersionStatus::Symlink(version, active) => {
                let mut cell = Cell::new(format!("{version} (symlink)"));
                if !*active {
                    cell = cell.add_attribute(Attribute::Dim);
                }
                cell
            }
        }
    }
    fn display_source(&self) -> Cell {
        Cell::new(match &self.source {
            Some(source) => source.to_string(),
            None => String::new(),
        })
    }
    fn display_requested(&self) -> Cell {
        Cell::new(match &self.requested {
            Some(s) => s.clone(),
            None => String::new(),
        })
    }
}

async fn json_tool_version_from(config: &Arc<Config>, row: RuntimeRow<'_>) -> JSONToolVersion {
    let (ls, p, tv, source) = row;
    let vs: VersionStatus = version_status_from(config, (ls, p.as_ref(), &tv, &source)).await;
    JSONToolVersion {
        symlinked_to: p.symlink_path(&tv),
        install_path: tv.install_path(),
        version: tv.version.clone(),
        requested_version: if source.is_unknown() {
            None
        } else {
            Some(tv.request.version())
        },
        source: if source.is_unknown() {
            None
        } else {
            Some(source.as_json())
        },
        installed: !matches!(vs, VersionStatus::Missing(_)),
        active: match vs {
            VersionStatus::Active(_, _) => true,
            VersionStatus::Symlink(_, active) => active,
            _ => false,
        },
    }
}

#[derive(Debug)]
enum VersionStatus {
    Active(String, bool),
    Inactive(String),
    Missing(String),
    Symlink(String, bool),
}

async fn version_status_from(
    config: &Arc<Config>,
    (ls, p, tv, source): (&Ls, &dyn Backend, &ToolVersion, &ToolSource),
) -> VersionStatus {
    if p.symlink_path(tv).is_some() {
        VersionStatus::Symlink(tv.version.clone(), !source.is_unknown())
    } else if !p.is_version_installed(config, tv, true) {
        VersionStatus::Missing(tv.version.clone())
    } else if !source.is_unknown() {
        let outdated = if ls.outdated {
            p.is_version_outdated(config, tv).await
        } else {
            false
        };
        VersionStatus::Active(tv.version.clone(), outdated)
    } else {
        VersionStatus::Inactive(tv.version.clone())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise ls</bold>
    node    20.0.0 ~/src/myapp/.tool-versions latest
    python  3.11.0 ~/.tool-versions           3.10
    python  3.10.0

    $ <bold>mise ls --current</bold>
    node    20.0.0 ~/src/myapp/.tool-versions 20
    python  3.11.0 ~/.tool-versions           3.11.0

    $ <bold>mise ls --json</bold>
    {
      "node": [
        {
          "version": "20.0.0",
          "install_path": "/Users/jdx/.mise/installs/node/20.0.0",
          "source": {
            "type": "mise.toml",
            "path": "/Users/jdx/mise.toml"
          }
        }
      ],
      "python": [...]
    }
"#
);
