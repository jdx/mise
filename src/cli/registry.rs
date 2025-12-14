use crate::backend::backend_type::BackendType;
use crate::config::Settings;
use crate::registry::{REGISTRY, RegistryTool, tool_enabled};
use crate::ui::table::MiseTable;
use eyre::{Result, bail};
use itertools::Itertools;
use serde_derive::Serialize;

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
}

#[derive(Serialize)]
struct RegistryToolOutput {
    short: String,
    backends: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
}

impl Registry {
    pub async fn run(self) -> Result<()> {
        if let Some(name) = &self.name {
            if let Some(rt) = REGISTRY.get(name.as_str()) {
                if self.json {
                    self.display_single_json(name, rt)?;
                } else {
                    miseprintln!("{}", rt.backends().join(" "));
                }
            } else {
                bail!("tool not found in registry: {name}");
            }
        } else if self.complete {
            self.complete()?;
        } else if self.json {
            self.display_json()?;
        } else {
            self.display_table()?;
        }
        Ok(())
    }

    fn display_table(&self) -> Result<()> {
        let filter_backend = |rt: &RegistryTool| {
            if let Some(backend) = &self.backend {
                rt.backends()
                    .iter()
                    .filter(|full| full.starts_with(&format!("{backend}:")))
                    .cloned()
                    .collect()
            } else {
                rt.backends()
            }
        };
        let mut table = MiseTable::new(false, &["Tool", "Backends"]);
        let data = REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .filter(|(short, rt)| !self.hide_aliased || **short == rt.short)
            .map(|(short, rt)| (short.to_string(), filter_backend(rt).join(" ")))
            .filter(|(_, backends)| !backends.is_empty())
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(short, backends)| vec![short, backends])
            .collect_vec();
        for row in data {
            table.add_row(row);
        }
        table.print()
    }

    fn complete(&self) -> Result<()> {
        REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .filter(|(short, rt)| !self.hide_aliased || **short == rt.short)
            .map(|(short, rt)| {
                (
                    short.to_string(),
                    rt.description
                        .or(rt.backends().first().cloned())
                        .unwrap_or_default(),
                )
            })
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .for_each(|(short, description)| {
                println!(
                    "{}:{}",
                    short.replace(":", "\\:"),
                    description.replace(":", "\\:")
                );
            });
        Ok(())
    }

    fn display_json(&self) -> Result<()> {
        let filter_backend = |rt: &RegistryTool| {
            if let Some(backend) = &self.backend {
                rt.backends()
                    .iter()
                    .filter(|full| full.starts_with(&format!("{backend}:")))
                    .cloned()
                    .collect()
            } else {
                rt.backends()
            }
        };
        let tools: Vec<RegistryToolOutput> = REGISTRY
            .iter()
            .filter(|(short, _)| filter_enabled(short))
            .filter(|(short, rt)| !self.hide_aliased || **short == rt.short)
            .map(|(short, rt)| {
                let backends = filter_backend(rt);
                RegistryToolOutput {
                    short: short.to_string(),
                    backends: backends.iter().map(|s| s.to_string()).collect(),
                    description: rt.description.map(|s| s.to_string()),
                    aliases: rt.aliases.iter().map(|s| s.to_string()).collect(),
                }
            })
            .filter(|tool| !tool.backends.is_empty())
            .sorted_by(|a, b| a.short.cmp(&b.short))
            .collect();
        miseprintln!("{}", serde_json::to_string_pretty(&tools)?);
        Ok(())
    }

    fn display_single_json(&self, name: &str, rt: &RegistryTool) -> Result<()> {
        let filter_backend = |rt: &RegistryTool| {
            if let Some(backend) = &self.backend {
                rt.backends()
                    .iter()
                    .filter(|full| full.starts_with(&format!("{backend}:")))
                    .cloned()
                    .collect()
            } else {
                rt.backends()
            }
        };
        let tool = RegistryToolOutput {
            short: name.to_string(),
            backends: filter_backend(rt).iter().map(|s| s.to_string()).collect(),
            description: rt.description.map(|s| s.to_string()),
            aliases: rt.aliases.iter().map(|s| s.to_string()).collect(),
        };
        miseprintln!("{}", serde_json::to_string_pretty(&tool)?);
        Ok(())
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
    tool_enabled(
        &Settings::get().enable_tools,
        &Settings::get().disable_tools,
        &short.to_string(),
    )
}
