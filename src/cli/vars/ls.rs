use eyre::Result;
use tabled::Tabled;

use crate::config::env_directive::prompt;
use crate::ui::table;

/// List the saved answers
///
/// Reads `$MISE_STATE_DIR/vars.toml`; no config is loaded.
#[derive(Debug, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment)]
pub(super) struct VarsLs {
    /// Do not print the table header
    #[usage(long)]
    no_header: bool,
}

#[derive(Tabled)]
struct Row {
    name: String,
    answer: String,
}

impl VarsLs {
    pub(super) fn run(self) -> Result<()> {
        let rows = prompt::saved_all()
            .into_iter()
            .map(|(name, answer)| Row { name, answer })
            .collect::<Vec<_>>();
        let mut table = tabled::Table::new(rows);
        table::print(&mut table, self.no_header)?;
        Ok(())
    }
}
