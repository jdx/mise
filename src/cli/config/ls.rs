use crate::config::config_file::ConfigFile;
use crate::config::Config;
use crate::file::display_path;
use crate::ui::table::MiseTable;
use comfy_table::{Attribute, Cell};
use eyre::Result;
use itertools::Itertools;

/// List config files currently in use
#[derive(Debug, clap::Args)]
#[clap(visible_alias="list", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ConfigLs {
    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    pub no_header: bool,

    /// Output in JSON format
    #[clap(short = 'J', long, verbatim_doc_comment)]
    pub json: bool,
}

impl ConfigLs {
    pub fn run(self) -> Result<()> {
        if self.json {
            self.display_json()?;
        } else {
            self.display()?;
        }
        Ok(())
    }

    fn display(&self) -> Result<()> {
        let config = Config::get();
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
        table.truncate(true).print()
    }

    fn display_json(&self) -> Result<()> {
        let array_items = Config::get()
            .config_files
            .values()
            .map(|cf| {
                let c: &dyn ConfigFile = cf.as_ref();
                let mut item = serde_json::Map::new();
                item.insert(
                    "path".to_string(),
                    serde_json::Value::String(c.get_path().to_string_lossy().to_string()),
                );
                let plugins = c
                    .to_tool_request_set()
                    .unwrap()
                    .list_tools()
                    .into_iter()
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect::<Vec<serde_json::Value>>();
                item.insert("tools".to_string(), serde_json::Value::Array(plugins));

                item
            })
            .collect::<serde_json::Value>();
        miseprintln!("{}", serde_json::to_string_pretty(&array_items)?);
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
