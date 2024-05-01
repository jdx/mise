use eyre::Result;

use crate::forge::{self, ForgeType};

/// List built-in backends
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct BackendsLs {}

impl BackendsLs {
    pub fn run(self) -> Result<()> {
        let mut forges = forge::list_forge_types();
        forges.retain(|f| *f != ForgeType::Asdf);

        for forge in forges {
            miseprintln!("{}", forge);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

  $ <bold>mise backends ls</bold>
  cargo
  go
  npm
  pipx
  ubi
"#
);

#[cfg(test)]
mod tests {

    #[test]
    fn test_backends_list() {
        assert_cli_snapshot!("backends", "list");
    }
}
