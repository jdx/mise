use crate::config::tracking::Tracker;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::ui::table::MiseTable;
use comfy_table::{Attribute, Cell};
use eyre::Result;
use itertools::Itertools;

/// List config files currently in use
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ConfigLs {
    /// Output in JSON format
    #[clap(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,

    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    pub no_header: bool,

    /// List all tracked config files
    #[clap(long, verbatim_doc_comment)]
    pub tracked_configs: bool,
}

impl ConfigLs {
    pub async fn run(self) -> Result<()> {
        if self.tracked_configs {
            self.display_tracked_configs().await?;
        } else if self.json {
            self.display_json().await?;
        } else {
            self.display().await?;
        }
        Ok(())
    }

    async fn display(&self) -> Result<()> {
        let config = Config::get().await?;
        let env_results = config.env_results().await?;
        let configs = config
            .config_files
            .values()
            .rev()
            .map(|cf| cf.as_ref())
            .collect_vec();
        let mut table = MiseTable::new(self.no_header, &["Path", "Tools"]);
        for cfg in configs {
            let ts = cfg.to_tool_request_set().unwrap();
            let tools = ts.list_tools().into_iter().join(", ");
            let tools = if tools.is_empty() {
                Cell::new("(none)")
                    .add_attribute(Attribute::Italic)
                    .add_attribute(Attribute::Dim)
            } else {
                Cell::new(tools).add_attribute(Attribute::Dim)
            };
            table.add_row(vec![Cell::new(display_path(cfg.get_path())), tools]);
        }
        let verbose = Settings::get().verbose;
        for f in &env_results.env_files {
            let description = if verbose {
                env_results
                    .env
                    .iter()
                    .filter(|(_, (_, src))| src == f)
                    .map(|(k, _)| k.as_str())
                    .join(", ")
            } else {
                String::new()
            };
            let tools = if description.is_empty() {
                Cell::new("(none)")
                    .add_attribute(Attribute::Italic)
                    .add_attribute(Attribute::Dim)
            } else {
                Cell::new(description).add_attribute(Attribute::Dim)
            };
            table.add_row(vec![Cell::new(display_path(f)), tools]);
        }
        table.truncate(true).print()
    }

    async fn display_json(&self) -> Result<()> {
        let config = Config::get().await?;
        let env_results = config.env_results().await?;
        let array_items: Vec<serde_json::Value> = config
            .config_files
            .values()
            .map(|cf| {
                let tools: Vec<String> = cf
                    .to_tool_request_set()
                    .unwrap()
                    .list_tools()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                serde_json::json!({
                    "path": cf.get_path().to_string_lossy(),
                    "tools": tools,
                })
            })
            .chain(env_results.env_files.iter().map(|f| {
                serde_json::json!({
                    "path": f.to_string_lossy(),
                    "tools": [],
                })
            }))
            .collect();
        miseprintln!("{}", serde_json::to_string_pretty(&array_items)?);
        Ok(())
    }

    async fn display_tracked_configs(&self) -> Result<()> {
        let tracked_configs = Tracker::list_all()?.into_iter().unique().sorted();
        for path in tracked_configs {
            println!("{}", path.display());
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise config ls</bold>
    Path                        Tools
    ~/.config/mise/config.toml  pitchfork
    ~/src/mise/mise.toml        actionlint, bun, cargo-binstall, cargo:cargo-edit, cargo:cargo-insta
"#
);
