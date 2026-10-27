use eyre::{Result, bail};
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
    /// `--json` and `--json-extended` are also accepted before the subcommand
    /// (`mise vars --json ls`), so the parent's flags count here too.
    pub(super) async fn run(self, parent_json: bool, parent_json_extended: bool) -> Result<()> {
        list(
            self.no_header,
            self.json || parent_json,
            self.json_extended || parent_json_extended,
        )
        .await
    }
}

pub(super) async fn list(no_header: bool, json: bool, json_extended: bool) -> Result<()> {
    if no_header && (json || json_extended) {
        bail!("--no-header cannot be combined with --json or --json-extended");
    }
    // A `required` var that is not set yet should not hide the others.
    prompt::tolerate_missing();
    let config = Config::get().await?;
    let vars = config
        .vars_results_cached()
        .map(|results| {
            results
                .vars
                .iter()
                .map(|(name, (value, source))| {
                    let value = if results.redactions.contains(name) {
                        "[redacted]".to_string()
                    } else {
                        value.clone()
                    };
                    (name.clone(), value, source.clone())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if json || json_extended {
        // JSON keeps the full source path; only the table abbreviates the home directory.
        let map: serde_json::Map<String, serde_json::Value> = vars
            .into_iter()
            .map(|(name, value, source)| {
                let value = if json_extended {
                    serde_json::json!({ "value": value, "source": source })
                } else {
                    value.into()
                };
                (name, value)
            })
            .collect();
        miseprintln!("{}", serde_json::to_string_pretty(&map)?);
        return Ok(());
    }
    let rows = vars
        .into_iter()
        .map(|(name, value, source)| Row {
            name,
            value,
            source: display_path(source),
        })
        .collect::<Vec<_>>();
    let mut table = tabled::Table::new(rows);
    table::print(&mut table, no_header)?;
    Ok(())
}
