use crate::backend::backend_type::BackendType;
use eyre::Result;
use strum::IntoEnumIterator;

/// List built-in backends
///
/// Backends that plugins add are listed by `mise plugins ls`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    visible_alias = "list",
    example("mise backends ls", help = "List the backends built into mise"),
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
