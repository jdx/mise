use eyre::Result;
use serde_json::json;

use crate::config::{Config, Settings};
use crate::path::PathExt;
use crate::system;
use crate::system::defaults::DefaultsState;
use crate::system::files::FileState;
use crate::system::packages::PackageState;
use crate::ui::table::MiseTable;

/// Show the status of system packages from `[system.packages]`, files from
/// `[system.files]`, and macOS defaults from `[system.defaults]`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ls", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured packages or files are missing
    #[clap(long)]
    missing: bool,
}

impl SystemStatus {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise system")?;
        let config = Config::get().await?;
        let mgrs = system::packages_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        for mp in mgrs {
            let name = mp.manager.name();
            if mp.disabled || !mp.manager.is_available() {
                let reason = if mp.disabled {
                    "excluded by the system_packages.managers setting".to_string()
                } else {
                    mp.manager.unavailable_reason()
                };
                if self.json {
                    json_out.insert(
                        name.to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &mp.requests {
                        rows.push(vec![
                            name.to_string(),
                            req.to_string(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
                continue;
            }
            let statuses = mp.manager.installed(&mp.requests).await?;
            let mut json_pkgs = vec![];
            for s in statuses {
                let (installed_version, state) = match &s.state {
                    PackageState::Installed { version } => (version.clone(), "installed"),
                    PackageState::Missing => {
                        any_missing = true;
                        ("".to_string(), "missing")
                    }
                    PackageState::VersionMismatch { installed } => {
                        any_missing = true;
                        (installed.clone(), "version mismatch")
                    }
                };
                if self.json {
                    json_pkgs.push(json!({
                        "package": s.request.name,
                        "requested_version": s.request.version.clone().unwrap_or_else(|| "latest".to_string()),
                        "state": state.replace(' ', "_"),
                        "installed_version": installed_version,
                    }));
                } else {
                    rows.push(vec![
                        name.to_string(),
                        s.request.to_string(),
                        installed_version,
                        state.to_string(),
                    ]);
                }
            }
            if self.json {
                json_out.insert(
                    name.to_string(),
                    json!({ "available": true, "packages": json_pkgs }),
                );
            }
        }
        let files = system::files::files_from_config(&config);
        let mut file_rows: Vec<Vec<String>> = vec![];
        let mut json_files = vec![];
        for req in &files {
            let state = match system::files::check(&config, req) {
                Ok(state) => state,
                // e.g. a template that fails to render — visible, not fatal
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
        let defaults = system::defaults_from_config(&config);
        let mut defaults_rows: Vec<Vec<String>> = vec![];
        if !defaults.is_empty() {
            if !system::defaults::is_available() {
                let reason = system::defaults::unavailable_reason();
                if self.json {
                    json_out.insert(
                        "defaults".to_string(),
                        json!({ "available": false, "reason": reason }),
                    );
                } else {
                    for req in &defaults {
                        defaults_rows.push(vec![
                            req.domain.clone(),
                            req.key.clone(),
                            req.value.to_string(),
                            "".to_string(),
                            format!("skipped ({reason})"),
                        ]);
                    }
                }
            } else {
                let statuses = system::defaults::status(&defaults).await?;
                let mut json_entries = vec![];
                for s in statuses {
                    let (current, state) = match &s.state {
                        DefaultsState::Set => (s.request.value.to_string(), "set"),
                        DefaultsState::Differs { current } => {
                            any_missing = true;
                            (current.clone(), "differs")
                        }
                        DefaultsState::Unset => {
                            any_missing = true;
                            ("".to_string(), "unset")
                        }
                    };
                    if self.json {
                        json_entries.push(json!({
                            "domain": s.request.domain,
                            "key": s.request.key,
                            "value": s.request.value.to_json(),
                            "current": current,
                            "state": state,
                        }));
                    } else {
                        defaults_rows.push(vec![
                            s.request.domain.clone(),
                            s.request.key.clone(),
                            s.request.value.to_string(),
                            current,
                            state.to_string(),
                        ]);
                    }
                }
                if self.json {
                    json_out.insert(
                        "defaults".to_string(),
                        json!({ "available": true, "entries": json_entries }),
                    );
                }
            }
        }
        if self.json {
            json_out.insert("files".to_string(), json!(json_files));
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else {
            if rows.is_empty() && file_rows.is_empty() && defaults_rows.is_empty() {
                info!(
                    "nothing configured in [system.packages], [system.files], or [system.defaults]"
                );
            }
            if !rows.is_empty() {
                let mut table =
                    MiseTable::new(false, &["Manager", "Package", "Installed", "State"]);
                for row in rows {
                    table.add_row(row);
                }
                table.print()?;
            }
            if !file_rows.is_empty() {
                let mut table = MiseTable::new(false, &["Target", "Mode", "Source", "State"]);
                for row in file_rows {
                    table.add_row(row);
                }
                table.print()?;
            }
            if !defaults_rows.is_empty() {
                let mut table =
                    MiseTable::new(false, &["Domain", "Key", "Value", "Current", "State"]);
                for row in defaults_rows {
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

    $ <bold>mise system status</bold>
    $ <bold>mise system status --json</bold>
    $ <bold>mise system status --missing</bold> # exit 1 if anything is missing
"#
);
