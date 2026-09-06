use eyre::Result;

use crate::system::history::sync::machines;
use crate::ui::table::MiseTable;

/// List the machines with recovery refs in the setup repository
///
/// Experimental: enable with `mise settings experimental=true`.
///
/// This machine first, then every machine whose refs the last
/// `mise bootstrap dotfiles sync` fetched. Their checkpoints are addressed as
/// `<machine>/<ref>`: `mise bootstrap dotfiles rollback --to laptop/latest --all`
/// recovers a machine's backed-up files; their journals are data only.
/// Encrypted backups are listed without a key; restoring one needs an
/// identity among its recipients.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesMachines {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

impl DotfilesMachines {
    pub(crate) async fn run(self) -> Result<()> {
        crate::config::Settings::get().ensure_experimental("dotfile tracking")?;
        let (store, _, entries) = super::history::open().await?;
        let Some(repo) = store.repo() else {
            eyre::bail!("listing machines requires git");
        };
        let this_encrypts = crate::system::history::sync::run::origin()
            .map(|origin| origin.encrypt_backups)
            .unwrap_or(false);
        let machines = machines::list(repo, store.machine(), &entries, this_encrypts)?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&machines)?);
            return Ok(());
        }
        let mut table = MiseTable::new(
            false,
            &["Machine", "Id", "Checkpoints", "Encrypted", "Latest"],
        );
        for machine in machines {
            table.add_row(vec![
                if machine.this {
                    format!("{} (this machine)", machine.name)
                } else {
                    machine.name
                },
                machine.id,
                machine.checkpoints.to_string(),
                match machine.encrypted {
                    0 => "no".to_string(),
                    n if n == machine.checkpoints => "yes".to_string(),
                    n => format!("{n} of {}", machine.checkpoints),
                },
                machine
                    .latest
                    .as_deref()
                    .map(super::history::local_time)
                    .unwrap_or_else(|| "-".into()),
            ]);
        }
        table.print()
    }
}
