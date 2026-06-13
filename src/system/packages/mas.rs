use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;
use serde_json::Value;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;

/// Mac App Store apps via the `mas` CLI.
pub struct MasManager {}

impl MasManager {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstalledApp {
    adam_id: Option<String>,
    bundle_id: Option<String>,
    version: String,
}

fn value_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        match value {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    })
}

fn parse_mas_json_value(value: &Value) -> Option<InstalledApp> {
    let adam_id = value_string(
        value,
        &[
            "adamID", "adamId", "adam_id", "appID", "appId", "app_id", "id", "trackID", "trackId",
        ],
    );
    let bundle_id = value_string(
        value,
        &[
            "bundleID",
            "bundleId",
            "bundle_id",
            "bundleIdentifier",
            "bundle_identifier",
        ],
    );
    let version = value_string(
        value,
        &[
            "version",
            "currentVersion",
            "current_version",
            "versionString",
        ],
    )?;
    Some(InstalledApp {
        adam_id,
        bundle_id,
        version,
    })
}

fn parse_mas_json(output: &str) -> Vec<InstalledApp> {
    let output = output.trim();
    if output.is_empty() {
        return vec![];
    }
    if let Ok(Value::Array(values)) = serde_json::from_str::<Value>(output) {
        return values.iter().filter_map(parse_mas_json_value).collect();
    }
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| parse_mas_json_value(&value))
        .collect()
}

fn parse_mas_text(output: &str) -> Vec<InstalledApp> {
    output
        .lines()
        .filter_map(|line| {
            let (adam_id, rest) = line.trim().split_once(char::is_whitespace)?;
            if !adam_id.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let version = rest
                .rsplit_once('(')
                .and_then(|(_, v)| v.strip_suffix(')'))
                .unwrap_or("")
                .trim();
            if version.is_empty() {
                return None;
            }
            Some(InstalledApp {
                adam_id: Some(adam_id.to_string()),
                bundle_id: None,
                version: version.to_string(),
            })
        })
        .collect()
}

fn statuses_from_apps(apps: &[InstalledApp], requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let mut installed: HashMap<String, String> = HashMap::new();
    for app in apps {
        if let Some(adam_id) = &app.adam_id {
            installed.insert(adam_id.clone(), app.version.clone());
        }
        if let Some(bundle_id) = &app.bundle_id {
            installed.insert(bundle_id.clone(), app.version.clone());
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(&req.name) {
                Some(version) => match &req.version {
                    Some(requested) if version != requested => PackageState::VersionMismatch {
                        installed: version.clone(),
                    },
                    _ => PackageState::Installed {
                        version: version.clone(),
                    },
                },
                None => PackageState::Missing,
            };
            PackageStatus {
                request: req.clone(),
                state,
            }
        })
        .collect()
}

async fn mas_list() -> Result<Vec<InstalledApp>> {
    debug!("$ mas list --json");
    let json_output = tokio::process::Command::new("mas")
        .args(["list", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if json_output.status.success() {
        let stdout = String::from_utf8_lossy(&json_output.stdout);
        return Ok(parse_mas_json(&stdout));
    }

    debug!("$ mas list");
    let text_output = tokio::process::Command::new("mas")
        .arg("list")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !text_output.status.success() {
        let json_stderr = String::from_utf8_lossy(&json_output.stderr);
        let text_stderr = String::from_utf8_lossy(&text_output.stderr);
        bail!(
            "mas list failed: {}",
            if text_stderr.trim().is_empty() {
                json_stderr.trim()
            } else {
                text_stderr.trim()
            }
        );
    }
    let stdout = String::from_utf8_lossy(&text_output.stdout);
    Ok(parse_mas_text(&stdout))
}

#[async_trait(?Send)]
impl SystemPackageManager for MasManager {
    fn name(&self) -> &'static str {
        "mas"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos") && crate::file::which("mas").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "macos") {
            "mas not found".to_string()
        } else {
            "only available on macos".to_string()
        }
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let apps = mas_list().await?;
        Ok(statuses_from_apps(&apps, pkgs))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if let Some(p) = pkgs.iter().find(|p| p.version.is_some()) {
            bail!("mas cannot install a pinned version ('{p}')");
        }
        let mut args = vec!["install".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        if opts.dry_run {
            miseprintln!("mas {}", args.join(" "));
            return Ok(());
        }
        debug!("$ mas {}", args.join(" "));
        let status = tokio::process::Command::new("mas")
            .args(&args)
            .stdin(Stdio::null())
            .status()
            .await?;
        if !status.success() {
            bail!("mas install failed");
        }
        Ok(())
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut args = vec!["update".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        if opts.dry_run {
            miseprintln!("mas {}", args.join(" "));
            return Ok(());
        }
        debug!("$ mas {}", args.join(" "));
        let status = tokio::process::Command::new("mas")
            .args(&args)
            .stdin(Stdio::null())
            .status()
            .await?;
        if !status.success() {
            bail!("mas update failed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(name: &str, version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
        }
    }

    #[test]
    fn test_parse_mas_json_lines() {
        let apps = parse_mas_json(
            r#"{"adamID":497799835,"bundleID":"com.apple.dt.Xcode","version":"16.2"}
{"adamID":"409203825","bundleID":"com.apple.Numbers","version":"14.4"}"#,
        );
        assert_eq!(apps.len(), 2);
        let statuses = statuses_from_apps(
            &apps,
            &[
                req("497799835", None),
                req("com.apple.Numbers", None),
                req("missing", None),
                req("com.apple.dt.Xcode", Some("15.0")),
            ],
        );
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "16.2".to_string()
            }
        );
        assert_eq!(
            statuses[1].state,
            PackageState::Installed {
                version: "14.4".to_string()
            }
        );
        assert_eq!(statuses[2].state, PackageState::Missing);
        assert_eq!(
            statuses[3].state,
            PackageState::VersionMismatch {
                installed: "16.2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_mas_json_array() {
        let apps = parse_mas_json(
            r#"[{"id":497799835,"bundleIdentifier":"com.apple.dt.Xcode","version":"16.2"}]"#,
        );
        assert_eq!(
            apps,
            vec![InstalledApp {
                adam_id: Some("497799835".to_string()),
                bundle_id: Some("com.apple.dt.Xcode".to_string()),
                version: "16.2".to_string()
            }]
        );
    }

    #[test]
    fn test_parse_mas_text() {
        let apps = parse_mas_text("497799835 Xcode (16.2)\n409203825 Numbers (14.4)\n");
        assert_eq!(
            apps[0],
            InstalledApp {
                adam_id: Some("497799835".to_string()),
                bundle_id: None,
                version: "16.2".to_string()
            }
        );
    }
}
