use crate::backend::backend_type::BackendType;
use crate::backend::{self, SecurityFeature};
use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::registry::{REGISTRY, RegistryTool, tool_enabled};
use crate::ui::table::MiseTable;
use eyre::{Result, bail};
use itertools::Itertools;
use serde::Serialize;
use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinSet};

/// List available tools to install
///
/// This command lists the tools available in the registry as shorthand names.
///
/// For example, `poetry` is shorthand for `asdf:mise-plugins/mise-poetry`.
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct Registry {
    /// Show only the specified tool's full name
    name: Option<String>,

    /// Show only tools for this backend
    #[clap(short, long)]
    backend: Option<BackendType>,

    /// Print all tools with descriptions for shell completions
    #[clap(long, hide = true)]
    complete: bool,

    /// Hide aliased tools
    #[clap(long)]
    hide_aliased: bool,

    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Include security features for each tool's backends in JSON output.
    ///
    /// Requires --json. Security info is de-duplicated across
    /// all of a tool's backends. This can add noticeable time for large
    /// listings since each backend's security info is resolved individually.
    #[clap(long, requires = "json")]
    security: bool,
}

#[derive(Serialize)]
struct RegistryToolOutput {
    short: String,
    backends: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    security: Vec<SecurityFeature>,
}

struct RegistryToolOutputArgs {
    short: String,
    backends: Vec<String>,
    description: Option<String>,
    aliases: Vec<String>,
}

impl Registry {
    pub async fn run(self) -> Result<()> {
        if let Some(name) = &self.name {
            if let Some(rt) = REGISTRY.get(name.as_str()) {
                if self.json {
                    let tool = to_output(self.output_args(name, rt), self.security).await;
                    miseprintln!("{}", serde_json::to_string_pretty(&tool)?);
                } else {
                    miseprintln!("{}", self.filter_backends(rt).join(" "));
                }
            } else {
                bail!("tool not found in registry: {name}");
            }
        } else if self.complete {
            self.complete()?;
        } else if self.json {
            self.display_json().await?;
        } else {
            self.display_table()?;
        }
        Ok(())
    }

    fn filter_backends(&self, rt: &RegistryTool) -> Vec<&'static str> {
        if let Some(backend) = &self.backend {
            rt.backends()
                .into_iter()
                .filter(|full| full.starts_with(&format!("{backend}:")))
                .collect()
        } else {
            rt.backends()
        }
    }

    fn output_args(&self, short: &str, rt: &RegistryTool) -> RegistryToolOutputArgs {
        let backends: Vec<String> = self
            .filter_backends(rt)
            .iter()
            .map(|s| s.to_string())
            .collect();
        RegistryToolOutputArgs {
            short: short.to_string(),
            backends,
            description: rt.description.map(|s| s.to_string()),
            aliases: rt.aliases.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn filtered_tools(&self) -> impl Iterator<Item = (&'static str, &'static RegistryTool)> {
        let hide_aliased = self.hide_aliased;
        REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .filter(move |(short, rt)| !hide_aliased || *short == rt.short)
    }

    fn display_table(&self) -> Result<()> {
        let mut table = MiseTable::new(false, &["Tool", "Backends"]);
        let data = self
            .filtered_tools()
            .map(|(short, rt)| (short.to_string(), self.filter_backends(rt).join(" ")))
            .filter(|(_, backends)| !backends.is_empty())
            .map(|(short, backends)| vec![short, backends])
            .collect_vec();
        for row in data {
            table.add_row(row);
        }
        table.print()
    }

    fn complete(&self) -> Result<()> {
        self.filtered_tools()
            .map(|(short, rt)| {
                (
                    short.to_string(),
                    rt.description
                        .or(rt.backends().first().cloned())
                        .unwrap_or_default(),
                )
            })
            .for_each(|(short, description)| {
                println!(
                    "{}:{}",
                    short.replace(":", "\\:"),
                    description.replace(":", "\\:")
                );
            });
        Ok(())
    }

    async fn display_json(&self) -> Result<()> {
        // Collect owned output args before async work so parallel tasks do not
        // borrow from the static registry map.
        let tools: Vec<RegistryToolOutputArgs> = self
            .filtered_tools()
            .filter(|(_, rt)| !self.filter_backends(rt).is_empty())
            .map(|(short, rt)| self.output_args(short, rt))
            .collect();

        let mut outputs: Vec<RegistryToolOutput> = Vec::with_capacity(tools.len());
        if self.security {
            let semaphore = Arc::new(Semaphore::new(Settings::get().jobs));
            let mut jset: JoinSet<RegistryToolOutput> = JoinSet::new();
            for tool in tools {
                let permit = semaphore.clone().acquire_owned().await?;
                jset.spawn(async move {
                    let _permit = permit;
                    to_output(tool, true).await
                });
            }
            while let Some(result) = jset.join_next().await {
                outputs.push(result?);
            }
            outputs.sort_by(|a, b| a.short.cmp(&b.short));
        } else {
            for tool in tools {
                outputs.push(to_output(tool, false).await);
            }
        }
        miseprintln!("{}", serde_json::to_string_pretty(&outputs)?);
        Ok(())
    }
}

/// Resolve and merge security features across every backend for a tool.
/// Duplicate features (same variant + payload) are collapsed.
async fn collect_security(backends: &[String]) -> Vec<SecurityFeature> {
    let mut features: Vec<SecurityFeature> = Vec::new();
    for full in backends {
        let ba = BackendArg::from(full.as_str());
        let Some(backend) = backend::arg_to_backend(ba) else {
            continue;
        };
        for feature in backend.security_info().await {
            if !features.contains(&feature) {
                features.push(feature);
            }
        }
    }
    features
}

async fn to_output(tool: RegistryToolOutputArgs, security: bool) -> RegistryToolOutput {
    let security = if security {
        collect_security(&tool.backends).await
    } else {
        vec![]
    };

    RegistryToolOutput {
        short: tool.short,
        backends: tool.backends,
        description: tool.description,
        aliases: tool.aliases,
        security,
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise registry</bold>
    node    core:node
    poetry  asdf:mise-plugins/mise-poetry
    ubi     cargo:ubi-cli

    $ <bold>mise registry poetry</bold>
    asdf:mise-plugins/mise-poetry
"#
);

fn filter_enabled(short: &str) -> bool {
    let settings = Settings::get();
    let enable_tools = settings.enable_tools();
    let disable_tools = settings.disable_tools();
    tool_enabled(enable_tools.as_ref(), &disable_tools, &short.to_string())
}
