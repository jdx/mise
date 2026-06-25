use eyre::Result;
use serde_json::json;

use crate::config::{Config, Settings};
use crate::path::PathExt;
use crate::system;
use crate::system::files::FileState;
use crate::ui::table::MiseTable;

/// Show the status of dotfiles from `[dotfiles]`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ls", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct DotfilesStatus {
    /// Only show these targets
    #[clap(value_name = "TARGET")]
    targets: Vec<String>,

    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured dotfiles are not in their desired
    /// state (missing, source missing, differs)
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

impl DotfilesStatus {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise dotfiles")?;
        let config = Config::get().await?;
        let mut any_missing = false;

        let all_files = system::files::files_from_config(&config);
        let files = all_files
            .iter()
            .filter(|req| {
                system::files::matches_target(&req.target, &req.target_raw, &self.targets)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut file_rows: Vec<Vec<String>> = vec![];
        let mut json_files = vec![];
        for req in &files {
            let state = match system::files::check(&config, req) {
                Ok(state) => state,
                Err(err) => FileState::Differs(format!("{err}")),
            };
            let state_str = match &state {
                FileState::Applied => "applied".to_string(),
                FileState::Missing => "missing".to_string(),
                FileState::SourceMissing => "source missing".to_string(),
                FileState::Differs(reason) => format!("differs ({reason})"),
            };
            any_missing |= state != FileState::Applied;
            if self.json {
                json_files.push(json!({
                    "target": req.target_raw,
                    "source": req.source.display_user(),
                    "mode": req.mode.name(),
                    "state": match &state {
                        FileState::Applied => "applied",
                        FileState::Missing => "missing",
                        FileState::SourceMissing => "source_missing",
                        FileState::Differs(_) => "differs",
                    },
                }));
            } else {
                file_rows.push(vec![
                    req.target_raw.clone(),
                    req.mode.name().to_string(),
                    req.source.display_user(),
                    state_str,
                ]);
            }
        }

        let all_edits = system::edits::edits_from_config(&config);
        let edits = all_edits
            .iter()
            .filter(|req| system::edits::matches_target(req, &self.targets))
            .cloned()
            .collect::<Vec<_>>();
        if files.is_empty()
            && edits.is_empty()
            && !self.targets.is_empty()
            && (!all_files.is_empty() || !all_edits.is_empty())
        {
            eyre::bail!(
                "no dotfiles matched target filter: {}",
                self.targets.join(", ")
            );
        }
        let mut edit_rows: Vec<Vec<String>> = vec![];
        let mut json_edits = vec![];
        for req in &edits {
            let state = match system::edits::check(&config, req) {
                Ok(state) => state,
                Err(err) => FileState::Differs(format!("{err}")),
            };
            let state_str = match &state {
                FileState::Applied => "applied".to_string(),
                FileState::Missing => "missing".to_string(),
                FileState::SourceMissing => "source missing".to_string(),
                FileState::Differs(reason) => format!("differs ({reason})"),
            };
            any_missing |= state != FileState::Applied;
            if self.json {
                json_edits.push(json!({
                    "path": req.path_raw,
                    "edit": req.describe_op(),
                    "state": match &state {
                        FileState::Applied => "applied",
                        FileState::Missing => "missing",
                        FileState::SourceMissing => "source_missing",
                        FileState::Differs(_) => "differs",
                    },
                }));
            } else {
                edit_rows.push(vec![req.path_raw.clone(), req.describe_op(), state_str]);
            }
        }

        if self.json {
            miseprintln!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "files": json_files,
                    "edits": json_edits,
                }))?
            );
        } else {
            if file_rows.is_empty() && edit_rows.is_empty() {
                info!("nothing configured in [dotfiles]");
            }
            if !file_rows.is_empty() {
                let mut table = MiseTable::new(false, &["Target", "Mode", "Source", "State"]);
                for row in file_rows {
                    table.add_row(row);
                }
                table.print()?;
            }
            if !edit_rows.is_empty() {
                let mut table = MiseTable::new(false, &["File", "Edit", "State"]);
                for row in edit_rows {
                    table.add_row(row);
                }
                table.print()?;
            }
        }
        if self.missing && any_missing {
            crate::exit(1);
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise dotfiles status</bold>
    $ <bold>mise bootstrap dotfiles status</bold>
    $ <bold>mise dotfiles status ~/.zshrc</bold>
    $ <bold>mise dotfiles status --json</bold>
    $ <bold>mise dotfiles status --missing</bold> # exit 1 if anything is out of sync
"#
);
