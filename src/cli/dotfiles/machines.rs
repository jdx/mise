use eyre::Result;

use crate::system::history::sync::machines;
use crate::ui::table::MiseTable;

/// List the machines with recovery refs in the setup repository
///
/// This machine first, then every machine whose refs the last
/// `mise bootstrap dotfiles sync` fetched. Their checkpoints are addressed as
/// `<machine>/<ref>`: `mise bootstrap dotfiles rollback --to laptop/latest --all`
/// recovers a machine's backed-up files; their journals are data only.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct DotfilesMachines {
    /// Output in JSON format
    #[usage(long, short = 'J')]
    json: bool,
}

impl DotfilesMachines {
    pub(crate) async fn run(self) -> Result<()> {
        let (store, _, entries) = super::history::open().await?;
        let Some(repo) = store.repo() else {
            eyre::bail!("listing machines requires git");
        };
        let machines = machines::list(repo, store.machine(), &entries)?;
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&machines)?);
            return Ok(());
        }
        let mut table = MiseTable::new(false, &["Machine", "Id", "Checkpoints", "Latest"]);
        for machine in machines {
            table.add_row(vec![
                if machine.this {
                    format!("{} (this machine)", machine.name)
                } else {
                    machine.name
                },
                machine.id,
                machine.checkpoints.to_string(),
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
