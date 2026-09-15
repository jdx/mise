use crate::backend::backend_type::BackendType;
use eyre::Result;
use strum::IntoEnumIterator;

/// List built-in backends
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example(
        r###"mise backends ls
mise plugins ls"###,
        help = r###"Installed plugin availability and built-in backends are separate lists"###
    ),
    verbatim_doc_comment
)]
pub(super) struct BackendsLs {}

impl BackendsLs {
    pub(super) fn run(self) -> Result<()> {
        let mut backends = BackendType::iter().collect::<Vec<BackendType>>();
        backends.retain(|f| !matches!(f, BackendType::Unknown));

        for backend in backends {
            miseprintln!("{}", backend);
        }
        Ok(())
    }
}
