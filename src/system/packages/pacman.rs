use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

/// Arch-family (Arch, Manjaro, EndeavourOS) via pacman
pub struct PacmanManager {}

impl PacmanManager {
    pub fn new() -> Self {
        Self {}
    }

    /// fresh container case: no sync databases, any install would fail
    fn dbs_missing(&self) -> bool {
        let sync = Path::new("/var/lib/pacman/sync");
        !crate::file::ls(sync).unwrap_or_default().iter().any(|p| {
            p.extension()
                .map(|e| e.to_string_lossy() == "db")
                .unwrap_or(false)
        })
    }

    fn refresh(&self, opts: &InstallOpts) -> Result<()> {
        let args = vec!["-Sy".to_string()];
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("pacman", &args).join(" "));
            return Ok(());
        }
        sudo::run("pacman", &args, &[])
    }
}

fn parse_pacman_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let mut installed: HashMap<&str, &str> = HashMap::new();
    for line in output.lines() {
        if let Some((name, version)) = line.split_once(' ') {
            installed.insert(name, version);
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(req.name.as_str()) {
                // a pin matches the full version-pkgrel or just the version
                // part (any pkgrel)
                Some(version) => match &req.version {
                    Some(requested)
                        if *version != requested.as_str()
                            && !version.starts_with(&format!("{requested}-")) =>
                    {
                        PackageState::VersionMismatch {
                            installed: version.to_string(),
                        }
                    }
                    _ => PackageState::Installed {
                        version: version.to_string(),
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

#[async_trait(?Send)]
impl SystemPackageManager for PacmanManager {
    fn name(&self) -> &'static str {
        "pacman"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("pacman").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "pacman not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let mut args = vec!["-Q".to_string()];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        debug!("$ pacman {}", args.join(" "));
        let output = tokio::process::Command::new("pacman")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // pacman -Q exits 1 when any package is missing ("error: package 'x'
        // was not found" on stderr); installed ones still print to stdout.
        // Anything else on stderr (corrupt db, lock file) is a real error.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success()
            && !stderr.is_empty()
            && !stderr
                .lines()
                .all(|l| l.trim().is_empty() || l.contains("was not found"))
        {
            bail!("pacman -Q failed: {}", stderr.trim());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_pacman_query(&stdout, pkgs))
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // Arch repos only carry the latest version — pacman has no syntax to
        // install an older one, so a pin can be checked (status) but not
        // satisfied here; the CLI filters pinned requests out before calling
        if let Some(p) = pkgs.iter().find(|p| p.version.is_some()) {
            bail!(
                "pacman cannot install a pinned version ('{p}'): Arch repositories only \
                 provide the latest version"
            );
        }
        if opts.update || self.dbs_missing() {
            self.refresh(opts)?;
        }
        let mut args = vec![
            "-S".to_string(),
            "--noconfirm".to_string(),
            "--needed".to_string(),
            // `--` keeps package operands from being parsed as pacman options
            "--".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("pacman", &args).join(" "));
            return Ok(());
        }
        sudo::run("pacman", &args, &[])
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // refresh sync DBs, then -S --needed upgrades exactly the named
        // packages that are outdated. Note: Arch officially supports only
        // full-system upgrades (-Syu); upgrading individual packages is a
        // partial upgrade — documented as a caveat in the pacman docs page.
        self.refresh(opts)?;
        let mut args = vec![
            "-S".to_string(),
            "--noconfirm".to_string(),
            "--needed".to_string(),
            "--".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| p.name.clone()));
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("pacman", &args).join(" "));
            return Ok(());
        }
        sudo::run("pacman", &args, &[])
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
    fn test_parse_pacman_query() {
        let requests = vec![
            req("bc", None),
            req("nonexistent", None),
            req("zsh", Some("5.9")),
            req("tmux", Some("3.3")),
        ];
        let output = "bc 1.08.2-1\nzsh 5.9-5\ntmux 3.4-2\n";
        let statuses = parse_pacman_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.08.2-1".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        // a version-only pin matches any pkgrel
        assert_eq!(
            statuses[2].state,
            PackageState::Installed {
                version: "5.9-5".to_string()
            }
        );
        // a different installed version must not satisfy a pin
        assert_eq!(
            statuses[3].state,
            PackageState::VersionMismatch {
                installed: "3.4-2".to_string()
            }
        );
    }
}
