use eyre::Result;
use serde_json::json;

use crate::config::{Config, Settings};
use crate::system;
use crate::system::packages::PackageState;
use crate::ui::table::MiseTable;

/// Show the status of system packages from `[bootstrap.packages]`
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "ls", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemStatus {
    /// Output in JSON format
    #[clap(long, short = 'J')]
    json: bool,

    /// Exit with code 1 if any configured packages are not in their desired state
    #[clap(long, verbatim_doc_comment)]
    missing: bool,
}

impl SystemStatus {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
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
        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&json_out)?);
        } else {
            if rows.is_empty() {
                info!("nothing configured in [bootstrap.packages]");
            }
            if !rows.is_empty() {
                let mut table =
                    MiseTable::new(false, &["Manager", "Package", "Installed", "State"]);
                for row in rows {
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

    $ <bold>mise bootstrap packages status</bold>
    $ <bold>mise bootstrap packages status --json</bold>
    $ <bold>mise bootstrap packages status --missing</bold> # exit 1 if anything is out of sync
"#
);
