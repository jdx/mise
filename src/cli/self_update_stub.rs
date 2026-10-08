pub(crate) use crate::upgrade_hint::{upgrade_instructions_or_hint, upgrade_instructions_text};

pub(crate) async fn maybe_auto_update(
    _args: &[String],
    _original_cwd: Option<&std::path::Path>,
    _command_eligible: bool,
) -> crate::Result<()> {
    Ok(())
}

/// Update mise itself
///
/// This build of mise has self-update disabled. Update it with the package
/// manager that installed it.
#[derive(Debug, Default, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct SelfUpdate {
    /// Update to a specific version
    version: Option<String>,

    /// Update even if already up to date
    #[usage(long, short)]
    force: bool,

    /// Skip confirmation prompt
    #[usage(long, short)]
    yes: bool,

    /// Do not update installed plugins
    #[usage(long)]
    no_plugins: bool,
}

impl SelfUpdate {
    pub(crate) async fn run(self) -> crate::Result<()> {
        if let Some(instructions) = upgrade_instructions_text() {
            warn!("{}", instructions);
        }
        eyre::bail!("mise's self-update feature has been disabled at build time, cannot update");
    }
    pub(crate) fn is_available() -> bool {
        false
    }
}
