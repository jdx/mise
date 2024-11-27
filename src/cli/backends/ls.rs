use crate::backend::backend_type::BackendType;
use eyre::Result;
use strum::IntoEnumIterator;

/// List built-in backends
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "list", after_long_help = AFTER_LONG_HELP, verbatim_doc_comment)]
pub struct BackendsLs {}

impl BackendsLs {
    pub fn run(self) -> Result<()> {
        let mut backends = BackendType::iter().collect::<Vec<BackendType>>();
        backends.retain(|f| !matches!(f, BackendType::Unknown));

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
