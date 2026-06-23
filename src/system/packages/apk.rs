use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

/// Alpine Linux via apk
pub struct ApkManager {}

impl ApkManager {
    pub fn new() -> Self {
        Self {}
    }
}

fn parse_apk_info(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    requests
        .iter()
        .map(|req| {
            let prefix = format!("{}-", req.name);
            let version = output.lines().find_map(|line| {
                line.strip_prefix(&prefix)
                    .filter(|version| version.starts_with(|c: char| c.is_ascii_digit()))
                    .map(str::to_string)
            });
            let state = match version {
                Some(installed) => match &req.version {
                    Some(requested) if requested != &installed => {
                        PackageState::VersionMismatch { installed }
                    }
                    _ => PackageState::Installed { version: installed },
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

fn apk_name(req: &PackageRequest) -> String {
    match &req.version {
        Some(version) => format!("{}={version}", req.name),
        None => req.name.clone(),
    }
}

#[async_trait(?Send)]
impl SystemPackageManager for ApkManager {
    fn name(&self) -> &'static str {
        "apk"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("apk").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "apk not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let mut args = vec!["info".to_string(), "-e".to_string(), "-v".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        debug!("$ apk {}", args.join(" "));
        let output = tokio::process::Command::new("apk")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // apk info exits nonzero when any named package is not installed, but
        // still prints installed package versions. Treat unexpected stderr as
        // real apk failure instead of silently reporting everything missing.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success()
            && !stderr.is_empty()
            && !stderr
                .lines()
                .all(|l| l.trim().is_empty() || l.contains("not found"))
        {
            bail!("apk info failed: {}", stderr.trim());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_apk_info(&stdout, pkgs))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut args = vec!["add".to_string()];
        if opts.update {
            args.push("--update-cache".to_string());
        }
        args.push("--".to_string());
        args.extend(pkgs.iter().map(apk_name));
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("apk", &args).join(" "));
            return Ok(());
        }
        sudo::run("apk", &args, &[])
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let mut args = vec!["upgrade".to_string(), "--available".to_string()];
        args.push("--update-cache".to_string());
        args.push("--".to_string());
        args.extend(pkgs.iter().map(apk_name));
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("apk", &args).join(" "));
            return Ok(());
        }
        sudo::run("apk", &args, &[])
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
    fn test_parse_apk_info() {
        let requests = vec![
            req("bc", None),
            req("zlib", None),
            req("nonexistent", None),
            req("zlib-dev", Some("1.3.1-r2")),
            req("curl", Some("8.14.1-r1")),
        ];
        let output = "bc-1.08.2-r0\nzlib-dev-1.3.1-r2\ncurl-8.14.1-r0\n";
        let statuses = parse_apk_info(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.08.2-r0".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        assert_eq!(statuses[2].state, PackageState::Missing);
        assert_eq!(
            statuses[3].state,
            PackageState::Installed {
                version: "1.3.1-r2".to_string()
            }
        );
        assert_eq!(
            statuses[4].state,
            PackageState::VersionMismatch {
                installed: "8.14.1-r0".to_string()
            }
        );
    }
}
