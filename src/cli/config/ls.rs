use console::style;
use eyre::Result;
use itertools::Itertools;
use tabled::settings::object::Columns;
use tabled::settings::{Modify, Width};
use tabled::Tabled;

use crate::config::config_file::ConfigFile;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::ui::table;

/// [experimental] List config files currently in use
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct ConfigLs {
    /// Do not print table header
    #[clap(long, alias = "no-headers", verbatim_doc_comment)]
    pub no_header: bool,
}

impl ConfigLs {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental("`mise config ls`")?;
        let rows = config
            .config_files
            .values()
            .map(|cf| cf.as_ref().into())
            .collect::<Vec<Row>>();
        let mut table = tabled::Table::new(rows);
        table::default_style(&mut table, self.no_header);
        table.with(Modify::new(Columns::last()).with(Width::truncate(40).suffix("…")));
        miseprintln!("{table}");

        Ok(())
    }
}

fn format_plugin_cell(s: String) -> String {
    match s.is_empty() {
        true => style("(none)").italic().dim().to_string(),
        false => style(s).dim().to_string(),
    }
}

#[derive(Tabled)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    path: String,
    plugins: String,
}

impl From<&dyn ConfigFile> for Row {
    fn from(cf: &dyn ConfigFile) -> Self {
        let path = display_path(cf.get_path());
        let ts = cf.to_tool_request_set().unwrap();
        let plugins = ts.list_plugins().into_iter().join(", ");
        let plugins = format_plugin_cell(plugins);
        Self { path, plugins }
    }
}

// TODO: fill this out
static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise config ls</bold>
"#
);

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_config_ls() {
        reset();
        assert_cli_snapshot!("cfg", "--no-headers", @r###"
        ~/cwd/.test-tool-versions tiny       
        ~/.test-tool-versions     tiny, dummy
        ~/config/config.toml      (none)
        "###);
    }
}
