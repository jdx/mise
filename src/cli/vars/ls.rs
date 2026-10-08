use eyre::Result;
use tabled::Tabled;

use crate::config::Config;
use crate::config::env_directive::prompt;
use crate::file::display_path;
use crate::ui::table;

/// List every var, with the file it comes from
///
/// Lists the `[vars]` of every config file mise loads here, plus the answers
/// saved on this machine (source: `$MISE_STATE_DIR/vars.toml`). Values hidden
/// with `redact` show as `[redacted]`. A `required` var that is still unset only
/// warns here.
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
    value: String,
    source: String,
}

impl VarsLs {
    pub(super) async fn run(self) -> Result<()> {
        list(self.no_header).await
    }
}

pub(super) async fn list(no_header: bool) -> Result<()> {
    // A `required` var that is not set yet should not hide the others.
    prompt::tolerate_missing();
    let config = Config::get().await?;
    let rows = config
        .vars_results_cached()
        .map(|results| {
            results
                .vars
                .iter()
                .map(|(name, (value, source))| Row {
                    name: name.clone(),
                    value: if results.redactions.contains(name) {
                        "[redacted]".to_string()
                    } else {
                        value.clone()
                    },
                    source: display_path(source),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut table = tabled::Table::new(rows);
    table::print(&mut table, no_header)?;
    Ok(())
}
