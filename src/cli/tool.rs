use eyre::Result;
use itertools::Itertools;
use serde_derive::Serialize;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::toolset::{ToolSource, ToolVersionOptions, ToolsetBuilder};
use crate::ui::table;

/// Gets information about a tool
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Tool {
    /// Tool name to get information about
    tool: BackendArg,
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    #[clap(flatten)]
    filter: ToolInfoFilter,
}

#[derive(Debug, Clone, clap::Args)]
#[group(multiple = false)]
pub struct ToolInfoFilter {
    /// Only show backend field
    #[clap(long)]
    backend_: bool,

    /// Only show description field
    #[clap(long)]
    description: bool,

    /// Only show installed versions
    #[clap(long)]
    installed: bool,

    /// Only show active versions
    #[clap(long)]
    active: bool,

    /// Only show requested versions
    #[clap(long)]
    requested: bool,

    /// Only show config source
    #[clap(long)]
    config_source: bool,

    /// Only show tool options
    #[clap(long)]
    tool_options: bool,
}

impl Tool {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut ts = ToolsetBuilder::new().build(&config).await?;
        ts.resolve(&config).await?;
        let tvl = ts.versions.get(&self.tool);
        let tv = tvl.map(|tvl| tvl.versions.first().unwrap());
        let ba = tv.map(|tv| tv.ba()).unwrap_or_else(|| &self.tool);
        let backend = ba.backend().ok();
        let description = if let Some(backend) = backend {
            backend.description().await
        } else {
            None
        };
        let info = ToolInfo {
            backend: ba.full(),
            description,
            installed_versions: ts
                .list_installed_versions(&config)
                .await?
                .into_iter()
                .filter(|(b, _)| b.ba().as_ref() == ba)
                .map(|(_, tv)| tv.version)
                .collect::<Vec<_>>(),
            active_versions: tvl.map(|tvl| {
                tvl.versions
                    .iter()
                    .map(|tv| tv.version.clone())
                    .collect::<Vec<_>>()
            }),
            requested_versions: tvl.map(|tvl| {
                tvl.requests
                    .iter()
                    .map(|tr| tr.version())
                    .collect::<Vec<_>>()
            }),
            config_source: tvl.map(|tvl| tvl.source.clone()),
            tool_options: ba.opts(),
        };

        if self.json {
            self.output_json(info)
        } else {
            self.output_user(info)
        }
    }

    fn output_json(&self, info: ToolInfo) -> Result<()> {
        if self.filter.backend_ {
            miseprintln!("{}", serde_json::to_string_pretty(&info.backend)?);
        } else if self.filter.description {
            miseprintln!("{}", serde_json::to_string_pretty(&info.description)?);
        } else if self.filter.installed {
            miseprintln!(
                "{}",
                serde_json::to_string_pretty(&info.installed_versions)?
            );
        } else if self.filter.active {
            miseprintln!("{}", serde_json::to_string_pretty(&info.active_versions)?);
        } else if self.filter.requested {
            miseprintln!(
                "{}",
                serde_json::to_string_pretty(&info.requested_versions)?
            );
        } else if self.filter.config_source {
            miseprintln!("{}", serde_json::to_string_pretty(&info.config_source)?);
        } else if self.filter.tool_options {
            miseprintln!("{}", serde_json::to_string_pretty(&info.tool_options)?);
        } else {
            miseprintln!("{}", serde_json::to_string_pretty(&info)?);
        }
        Ok(())
    }

    fn output_user(&self, info: ToolInfo) -> Result<()> {
        if self.filter.backend_ {
            miseprintln!("{}", info.backend);
        } else if self.filter.description {
            if let Some(description) = info.description {
                miseprintln!("{}", description);
            } else {
                miseprintln!("[none]");
            }
        } else if self.filter.installed {
            miseprintln!("{}", info.installed_versions.join(" "));
        } else if self.filter.active {
            if let Some(active_versions) = info.active_versions {
                miseprintln!("{}", active_versions.join(" "));
            } else {
                miseprintln!("[none]");
            }
        } else if self.filter.requested {
            if let Some(requested_versions) = info.requested_versions {
                miseprintln!("{}", requested_versions.join(" "));
            } else {
                miseprintln!("[none]");
            }
        } else if self.filter.config_source {
            if let Some(config_source) = info.config_source {
                miseprintln!("{}", config_source);
            } else {
                miseprintln!("[none]");
            }
        } else if self.filter.tool_options {
            if info.tool_options.is_empty() {
                miseprintln!("[none]");
            } else {
                for (k, v) in info.tool_options.opts {
                    miseprintln!("{k}={v:?}");
                }
            }
        } else {
            let mut table = vec![];
            table.push(("Backend:", info.backend));
            if let Some(description) = info.description {
                table.push(("Description:", description));
            }
            table.push(("Installed Versions:", info.installed_versions.join(" ")));
            if let Some(active_versions) = info.active_versions {
                table.push(("Active Version:", active_versions.join(" ")));
            }
            if let Some(requested_versions) = info.requested_versions {
                table.push(("Requested Version:", requested_versions.join(" ")));
            }
            if let Some(config_source) = info.config_source {
                table.push(("Config Source:", config_source.to_string()));
            }
            if info.tool_options.is_empty() {
                table.push(("Tool Options:", "[none]".to_string()));
            } else {
                table.push((
                    "Tool Options:",
                    info.tool_options
                        .opts
                        .into_iter()
                        .map(|(k, v)| format!("{k}={v:?}"))
                        .join(","),
                ));
            }
            let mut table = tabled::Table::new(table);
            table::default_style(&mut table, true);
            miseprintln!("{table}");
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct ToolInfo {
    backend: String,
    description: Option<String>,
    installed_versions: Vec<String>,
    requested_versions: Option<Vec<String>>,
    active_versions: Option<Vec<String>>,
    config_source: Option<ToolSource>,
    tool_options: ToolVersionOptions,
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise tool node</bold>
    Backend:            core
    Installed Versions: 20.0.0 22.0.0
    Active Version:     20.0.0
    Requested Version:  20
    Config Source:      ~/.config/mise/mise.toml
    Tool Options:       [none]
"#
);
