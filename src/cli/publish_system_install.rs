use eyre::Result;

/// [internal] publish a staged system installation as root
///
/// `mise install --system` prepares a tool as the invoking user and streams
/// it into this command through sudo. Reads one request and its archive from
/// stdin; see `system_install`.
#[derive(Debug, usage_rs::Args)]
#[usage(hide = true)]
pub(crate) struct PublishSystemInstall {}

impl PublishSystemInstall {
    pub(crate) fn run(self) -> Result<()> {
        crate::system_install::apply_from_stdin()
    }
}
