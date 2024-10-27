use crate::registry::REGISTRY;
use crate::ui::table;
use eyre::{bail, Result};
use tabled::{Table, Tabled};

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
}

impl Registry {
    pub fn run(self) -> Result<()> {
        if let Some(name) = &self.name {
            if let Some(full) = REGISTRY.get(name.as_str()) {
                miseprintln!("{full}");
            } else {
                bail!("tool not found in registry: {name}");
            }
        } else {
            let data = REGISTRY
                .iter()
                .map(|(short, full)| (short.to_string(), full.to_string()).into())
                .collect::<Vec<Row>>();
            let mut table = Table::new(data);
            table::default_style(&mut table, false);
            miseprintln!("{table}");
        }
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
    ubi     cargo:ubi-cli

    $ <bold>mise registry poetry</bold>
    asdf:mise-plugins/mise-poetry
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
