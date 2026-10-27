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
/// warns here. `--json` prints an object of names and values, and
/// `--json-extended` adds each var's source.
#[derive(Debug, usage_rs::Args)]
#[usage(visible_alias = "list", verbatim_doc_comment)]
pub(super) struct VarsLs {
    /// Output in JSON format
    #[usage(long, short = 'J', group = "output")]
    json: bool,

    /// Output in JSON format with sources
    #[usage(long, group = "output")]
    json_extended: bool,

    /// Do not print the table header
    #[usage(long, conflicts = ["json", "json_extended"])]
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
        list(self.no_header, self.json, self.json_extended).await
    }
}

pub(super) async fn list(no_header: bool, json: bool, json_extended: bool) -> Result<()> {
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
    if json || json_extended {
        let map: serde_json::Map<String, serde_json::Value> = rows
            .into_iter()
            .map(|row| {
                let value = if json_extended {
                    serde_json::json!({ "value": row.value, "source": row.source })
                } else {
                    row.value.into()
                };
                (row.name, value)
            })
            .collect();
        miseprintln!("{}", serde_json::to_string_pretty(&map)?);
        return Ok(());
    }
    let mut table = tabled::Table::new(rows);
    table::print(&mut table, no_header)?;
    Ok(())
}
