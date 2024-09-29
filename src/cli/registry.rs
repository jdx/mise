use std::collections::BTreeMap;

use eyre::Result;
use tabled::{Table, Tabled};

use crate::config::{settings, Config};
use crate::registry::REGISTRY;
use crate::ui::table;

/// [experimental] List available tools to install
///
/// This command lists the tools available in the registry as shorthand names.
///
/// For example, `poetry` is shorthand for `asdf:mise-plugins/mise-poetry`.
#[derive(Debug, clap::Args)]
#[clap(after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct Registry {}

impl Registry {
    pub fn run(self) -> Result<()> {
        settings::ensure_experimental("registry")?;
        let mut tools = BTreeMap::new();

        for (plugin, url) in Config::get().get_shorthands() {
            let re = regex!(r#"^https://github.com/(.+?/.+?)(.git)?$"#);
            let full = if let Some(caps) = re.captures(url) {
                format!("asdf:{}", &caps[1])
            } else {
                format!("asdf:{}", url)
            };
            tools.insert(plugin.to_string(), full);
        }

        for (short, full) in REGISTRY.iter() {
            tools.insert(short.to_string(), full.to_string());
        }

        let data = tools.into_iter().map(|x| x.into()).collect::<Vec<Row>>();
        let mut table = Table::new(data);
        table::default_style(&mut table, false);
        miseprintln!("{table}");
        Ok(())
    }
}

#[derive(Tabled, Eq, PartialEq, Ord, PartialOrd)]
#[tabled(rename_all = "PascalCase")]
struct Row {
    short: String,
    full: String,
}

impl From<(String, String)> for Row {
    fn from((short, full): (String, String)) -> Self {
        Self { short, full }
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise registry</bold>
    node    core:node
    poetry  asdf:mise-plugins/mise-poetry
    ubi     cargo:ubi
"#
);

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::cli::tests::grep;
    use crate::test::reset;

    #[test]
    fn test_registry() {
        reset();
        let out = assert_cli!("registry");
        // TODO: enable this when core plugins are back in the registry
        // assert_snapshot!(grep(out, "node"), @"node                         core:node");
        assert_snapshot!(grep(out, "poetry"), @"poetry                       asdf:mise-plugins/mise-poetry");
    }
}
