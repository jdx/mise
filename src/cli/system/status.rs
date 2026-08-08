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
            let (requests, os_skipped): (Vec<_>, Vec<_>) = mp
                .requests
                .iter()
                .cloned()
                .partition(|request| mp.request_matches_platform(request));
            if requests.is_empty() {
                if self.json {
                    json_out.insert(
                        name.to_string(),
                        json!({
                            "available": true,
                            "packages": os_skipped.iter().map(|request| {
                                os_skipped_json(request, mp.os_filter(request).unwrap_or_default())
                            }).collect::<Vec<_>>(),
                        }),
                    );
                } else {
                    rows.extend(os_skipped.iter().map(|request| {
                        os_skipped_row(name, request, mp.os_filter(request).unwrap_or_default())
                    }));
                }
                continue;
            }
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
            let statuses = mp.manager.installed(&requests).await?;
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
                let os = mp.os_filter(request).unwrap_or_default();
                if self.json {
                    json_pkgs.push(os_skipped_json(request, os));
                } else {
                    rows.push(os_skipped_row(name, request, os));
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
fn os_skipped_row(manager: &str, request: &PackageRequest, os: &[String]) -> Vec<String> {
    vec![
        manager.to_string(),
        request.to_string(),
        "".to_string(),
        format!("skipped (os: {})", os.join(", ")),
    ]
}

/// JSON entry for an os-filtered package; mirrors the ordinary package shape
/// with `"state": "skipped"` and the entry's `os` list.
fn os_skipped_json(request: &PackageRequest, os: &[String]) -> serde_json::Value {
    json!({
        "package": request.name,
        "requested_version": request.version.clone().unwrap_or_else(|| "latest".to_string()),
        "state": "skipped",
        "reason": "os mismatch",
        "os": os,
        "installed_version": "",
    })
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

    fn request(name: &str, version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
        }
    }

    #[test]
    fn os_skipped_table_row_shape() {
        let row = os_skipped_row(
            "brew-cask",
            &request("firefox", None),
            &["macos".to_string(), "linux/arm64".to_string()],
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
        let row = os_skipped_row(
            "brew",
            &request("curl", Some("8.5.0-2")),
            &["linux".to_string()],
        );
        assert_eq!(row[1], "curl@8.5.0-2");
        assert_eq!(row[3], "skipped (os: linux)");
    }

    #[test]
    fn os_skipped_json_shape() {
        let value = os_skipped_json(&request("firefox", None), &["macos".to_string()]);
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

        let value = os_skipped_json(
            &request("curl", Some("8.5.0-2")),
            &["linux".to_string(), "macos/arm64".to_string()],
        );
        assert_eq!(value["requested_version"], "8.5.0-2");
        assert_eq!(value["os"], json!(["linux", "macos/arm64"]));
    }
}
