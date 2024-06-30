use eyre::Result;

use crate::backend::{self, BackendType};

/// List built-in backends
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct BackendsLs {}

impl BackendsLs {
    pub fn run(self) -> Result<()> {
        let mut backends = backend::list_backend_types();
        backends.retain(|f| *f != BackendType::Asdf);

        for backend in backends {
            miseprintln!("{}", backend);
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
  spm
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
