use eyre::Result;
use serde_json::json;

use crate::config::Config;
use crate::system;
use crate::system::packages::{PackageRequest, PackageState};
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
        let config = Config::get().await?;
        let mgrs = system::packages_from_config(&config);
        let mut any_missing = false;
        let mut rows: Vec<Vec<String>> = vec![];
        let mut json_out = serde_json::Map::new();
        for mp in mgrs {
            let name = mp.manager.name();
            let reason = if mp.disabled {
                Some("excluded by the system_packages.managers setting".to_string())
            } else {
                mp.manager.unavailable_reason_async().await
            };
            if let Some(reason) = reason {
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
            let (requests, os_skipped): (Vec<_>, Vec<_>) = mp
                .requests
                .iter()
                .cloned()
                .partition(|request| request.is_os_supported());
            let statuses = if requests.is_empty() {
                vec![]
            } else {
                mp.manager.installed(&requests).await?
            };
            let mut json_pkgs = vec![];
            for s in statuses {
                let (installed_version, state) = match &s.state {
                    PackageState::Installed { version } => (version.clone(), "installed"),
                    PackageState::Missing => {
                        any_missing = true;
                        ("".to_string(), "missing")
                    }
                    PackageState::NeedsRepair { installed } => {
                        any_missing = true;
                        (installed.clone(), "needs repair")
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
            for request in &os_skipped {
                if self.json {
                    json_pkgs.push(os_skipped_json(request));
                } else {
                    rows.push(os_skipped_row(name, request));
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
            return Err(crate::request_exit(1));
        }
        Ok(())
    }
}

/// Table row for an entry whose `os` list does not match the current platform,
/// rendered without querying the manager. The list stays as written in config.
fn os_skipped_row(manager: &str, request: &PackageRequest) -> Vec<String> {
    vec![
        manager.to_string(),
        request.to_string(),
        "".to_string(),
        format!("skipped (os: {})", os_list(request)),
    ]
}

/// JSON entry for an os-filtered package; mirrors the ordinary package shape
/// with `"state": "skipped"` and the entry's `os` list.
fn os_skipped_json(request: &PackageRequest) -> serde_json::Value {
    json!({
        "package": request.name,
        "requested_version": request.version.clone().unwrap_or_else(|| "latest".to_string()),
        "state": "skipped",
        "reason": "os mismatch",
        "os": request.os.clone().unwrap_or_default(),
        "installed_version": "",
    })
}

fn os_list(request: &PackageRequest) -> String {
    request.os.as_deref().unwrap_or_default().join(", ")
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages status</bold>
    $ <bold>mise bootstrap packages status --json</bold>
    $ <bold>mise bootstrap packages status --missing</bold> # exit 1 if anything is out of sync
"#
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::packages::PackageRequest;

    fn request(name: &str, version: Option<&str>, os: &[&str]) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
            os: Some(os.iter().map(|s| s.to_string()).collect()),
        }
    }

    #[test]
    fn os_skipped_table_row_shape() {
        let row = os_skipped_row(
            "brew-cask",
            &request("firefox", None, &["macos", "linux/arm64"]),
        );
        assert_eq!(
            row,
            vec![
                "brew-cask".to_string(),
                "firefox".to_string(),
                "".to_string(),
                "skipped (os: macos, linux/arm64)".to_string(),
            ]
        );

        // pinned versions keep the spec rendering of ordinary rows
        let row = os_skipped_row("apt", &request("curl", Some("8.5.0-2"), &["linux"]));
        assert_eq!(row[1], "curl@8.5.0-2");
        assert_eq!(row[3], "skipped (os: linux)");
    }

    #[test]
    fn os_skipped_json_shape() {
        let value = os_skipped_json(&request("firefox", None, &["macos"]));
        assert_eq!(
            value,
            json!({
                "package": "firefox",
                "requested_version": "latest",
                "state": "skipped",
                "reason": "os mismatch",
                "os": ["macos"],
                "installed_version": "",
            })
        );

        let value = os_skipped_json(&request("curl", Some("8.5.0-2"), &["linux", "macos/arm64"]));
        assert_eq!(value["requested_version"], "8.5.0-2");
        assert_eq!(value["os"], json!(["linux", "macos/arm64"]));
    }
}
