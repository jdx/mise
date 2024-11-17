use crate::backend::backend_type::BackendType;
use crate::registry::{RegistryTool, REGISTRY};
use crate::ui::table;
use eyre::{bail, Result};
use itertools::Itertools;
use tabled::{Table, Tabled};
use crate::config::SETTINGS;

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
}

impl Registry {
    pub fn run(self) -> Result<()> {
        let filter_backend = |rt: &RegistryTool| {
            if let Some(backend) = self.backend {
                rt.backends()
                    .iter()
                    .filter(|full| full.starts_with(&format!("{backend}:")))
                    .cloned()
                    .collect()
            } else {
                rt.backends()
            }
        };
        if let Some(name) = &self.name {
            if let Some(rt) = REGISTRY.get(name.as_str()) {
                miseprintln!("{}", rt.backends().join(" "));
            } else {
                bail!("tool not found in registry: {name}");
            }
        } else {
            let data = REGISTRY
                .iter()
                .filter(|(short, _)| !SETTINGS.disable_tools.contains(**short))
                .map(|(short, rt)| Row::from((short.to_string(), filter_backend(rt).join(" "))))
                .filter(|row| !row.backends.is_empty())
                .sorted_by(|a, b| a.short.cmp(&b.short));
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
    backends: String,
}

impl From<(String, String)> for Row {
    fn from((short, backends): (String, String)) -> Self {
        Self { short, backends }
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
