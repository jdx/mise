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

use crate::cli::args::ForgeArg;
use crate::config::Config;
use crate::forge;
use crate::forge::Forge;
use crate::toolset::{ToolSource, ToolVersion, ToolsetBuilder};
use crate::ui::table;

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
    /// Only show tool versions from [PLUGIN]
    #[clap(conflicts_with = "plugin_flag")]
    plugin: Option<Vec<ForgeArg>>,

    #[clap(long = "plugin", short, hide = true)]
    plugin_flag: Option<ForgeArg>,

    /// Only show tool versions currently specified in a .tool-versions/.mise.toml
    #[clap(long, short)]
    current: bool,

    /// Only show tool versions currently specified in a the global .tool-versions/.mise.toml
    #[clap(long, short)]
    global: bool,

    /// Only show tool versions that are installed
    /// (Hides tools defined in .tool-versions/.mise.toml but not installed)
    #[clap(long, short)]
    installed: bool,

    /// Output in an easily parseable format
    #[clap(long, hide = true, conflicts_with = "json")]
    parseable: bool,

    /// Output in JSON format
    #[clap(long, short = 'J', overrides_with = "parseable")]
    json: bool,

    /// Display missing tool versions
    #[clap(long, short, conflicts_with = "installed")]
    missing: bool,

    /// Display versions matching this prefix
    #[clap(long, requires = "plugin")]
    prefix: Option<String>,

    /// Don't display headers
    #[clap(long, alias = "no-headers", verbatim_doc_comment, conflicts_with_all = & ["json", "parseable"])]
    no_header: bool,
}

impl Ls {
    pub fn run(mut self) -> Result<()> {
        let config = Config::try_get()?;
        self.plugin = self
            .plugin
            .or_else(|| self.plugin_flag.clone().map(|p| vec![p]));
        self.verify_plugin()?;

        let mut runtimes = self.get_runtime_list(&config)?;
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

    fn verify_plugin(&self) -> Result<()> {
        match &self.plugin {
            Some(plugins) => {
                for fa in plugins {
                    let plugin = forge::get(fa);
                    ensure!(plugin.is_installed(), "{fa} is not installed");
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
                .filter(|(p, _, _)| plugins.contains(p.fa()))
                .map(|row| row.into())
                .collect();
            miseprintln!("{}", serde_json::to_string_pretty(&runtimes)?);
            return Ok(());
        }

        let mut plugins = JSONOutput::new();
        for (plugin_name, runtimes) in &runtimes
            .into_iter()
            .group_by(|(p, _, _)| p.id().to_string())
        {
            let runtimes = runtimes.map(|row| row.into()).collect();
            plugins.insert(plugin_name.clone(), runtimes);
        }
        miseprintln!("{}", serde_json::to_string_pretty(&plugins)?);
        Ok(())
    }

    fn display_parseable(&self, runtimes: Vec<RuntimeRow>) -> Result<()> {
        warn!("The parseable output format is deprecated and will be removed in a future release.");
        warn!("Please use the regular output format instead which has been modified to be more easily parseable.");
        let tvs = runtimes
            .into_iter()
            .map(|(p, tv, _)| (p, tv))
            .filter(|(p, tv)| p.is_version_installed(tv))
            .map(|(_, tv)| tv);
        for tv in tvs {
            if self.plugin.is_some() {
                // only displaying 1 plugin so only show the version
                miseprintln!("{}", tv.version);
            } else {
                miseprintln!("{} {}", &tv.forge, tv.version);
            }
        }
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
        miseprintln!("{}", table.to_string());
        Ok(())
    }

    fn get_runtime_list(&self, config: &Config) -> Result<Vec<RuntimeRow>> {
        let tsb = ToolsetBuilder::new().with_global_only(self.global);

        let ts = tsb.build(config)?;
        let mut versions: HashMap<(String, String), (Arc<dyn Forge>, ToolVersion)> = ts
            .list_installed_versions()?
            .into_iter()
            .map(|(p, tv)| ((p.id().into(), tv.version.clone()), (p, tv)))
            .collect();

        let active = ts
            .list_current_versions()
            .into_iter()
            .map(|(p, tv)| ((p.id().into(), tv.version.clone()), (p, tv)))
            .collect::<HashMap<(String, String), (Arc<dyn Forge>, ToolVersion)>>();

        versions.extend(active.clone());

        let rvs: Vec<RuntimeRow> = versions
            .into_iter()
            .filter(|(_, (f, _))| match &self.plugin {
                Some(p) => p.contains(f.fa()),
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
                    Some((_, tv)) => ts.versions.get(&tv.forge).map(|tv| tv.source.clone()),
                    None => None,
                };
                (p, tv, source)
            })
            // if it isn't installed and it's not specified, don't show it
            .filter(|(p, tv, source)| source.is_some() || p.is_version_installed(tv))
            .filter(|(p, _, _)| match &self.plugin {
                Some(forge) => forge.contains(p.fa()),
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

type RuntimeRow = (Arc<dyn Forge>, ToolVersion, Option<ToolSource>);

#[derive(Tabled)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    #[tabled(display_with = "Self::display_plugin")]
    plugin: Arc<dyn Forge>,
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
    fn display_plugin(plugin: &Arc<dyn Forge>) -> String {
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
        let vs: VersionStatus = (p.as_ref(), &tv, &source).into();
        JSONToolVersion {
            symlinked_to: p.symlink_path(&tv),
            install_path: tv.install_path(),
            version: tv.version,
            requested_version: source.as_ref().map(|_| tv.request.version()),
            source: source.map(|source| source.as_json()),
            installed: !matches!(vs, VersionStatus::Missing(_)),
            active: matches!(vs, VersionStatus::Active(_, _)),
        }
    }
}

enum VersionStatus {
    Active(String, bool),
    Inactive(String),
    Missing(String),
    Symlink(String, bool),
}

impl From<(&dyn Forge, &ToolVersion, &Option<ToolSource>)> for VersionStatus {
    fn from((p, tv, source): (&dyn Forge, &ToolVersion, &Option<ToolSource>)) -> Self {
        if p.symlink_path(tv).is_some() {
            VersionStatus::Symlink(tv.version.clone(), source.is_some())
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
            VersionStatus::Symlink(version, active) => {
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
            "type": ".mise.toml",
            "path": "/Users/jdx/.mise.toml"
          }
        }
      ],
      "python": [...]
    }
"#
);

#[cfg(test)]
mod tests {
    use crate::dirs;
    use crate::file::remove_all;
    use crate::test::reset;

    #[test]
    fn test_ls() {
        reset();
        let _ = remove_all(*dirs::INSTALLS);
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
        reset();
        assert_cli_snapshot!("ls", "-c", @r###"
        dummy  ref:master  ~/.test-tool-versions     ref:master
        tiny   3.1.0       ~/cwd/.test-tool-versions 3
        "###);
    }

    #[test]
    fn test_ls_json() {
        reset();
        let _ = remove_all(*dirs::INSTALLS);
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--json");
        assert_cli_snapshot!("ls", "--json", "tiny");
    }

    #[test]
    fn test_ls_parseable() {
        reset();
        let _ = remove_all(*dirs::INSTALLS);
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--parseable", @r###"
        dummy ref:master
        tiny 3.1.0
        mise The parseable output format is deprecated and will be removed in a future release.
        mise Please use the regular output format instead which has been modified to be more easily parseable.
        "###);
        assert_cli_snapshot!("ls", "--parseable", "tiny", @r###"
        3.1.0
        mise The parseable output format is deprecated and will be removed in a future release.
        mise Please use the regular output format instead which has been modified to be more easily parseable.
        "###);
    }

    #[test]
    fn test_ls_missing() {
        reset();
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--missing", @"");
    }

    #[test]
    fn test_ls_missing_plugin() {
        reset();
        let err = assert_cli_err!("ls", "missing-plugin");
        assert_str_eq!(err.to_string(), r#"missing-plugin is not installed"#);
    }

    #[test]
    fn test_ls_prefix() {
        reset();
        assert_cli!("install");
        assert_cli_snapshot!("ls", "--plugin=tiny", "--prefix=3", @"tiny  3.1.0  ~/cwd/.test-tool-versions 3");
    }
}
