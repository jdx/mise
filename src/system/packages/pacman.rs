use std::collections::{HashMap, HashSet};
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
                Some(version) => package_state(req, version),
                None => PackageState::Missing,
            };
            PackageStatus {
                request: req.clone(),
                state,
            }
        })
        .collect()
}

fn package_state(req: &PackageRequest, version: &str) -> PackageState {
    // a pin matches the full version-pkgrel or just the version part (any
    // pkgrel)
    match &req.version {
        Some(requested)
            if version != requested && !version.starts_with(&format!("{requested}-")) =>
        {
            PackageState::VersionMismatch {
                installed: version.to_string(),
            }
        }
        _ => PackageState::Installed {
            version: version.to_string(),
        },
    }
}

fn parse_pacman_package(output: &str) -> Option<(&str, &str)> {
    output.lines().find_map(|line| line.split_once(' '))
}

fn parse_pacman_deptest(output: &str) -> HashSet<&str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn apply_provider_query<'a>(status: &mut PackageStatus, output: &'a str) -> Result<&'a str> {
    let Some((provider, version)) = parse_pacman_package(output) else {
        bail!(
            "pacman -Q returned no package for satisfied requirement '{}'",
            status.request.name
        );
    };
    status.state = package_state(&status.request, version);
    Ok(provider)
}

async fn pacman_query(names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args = vec!["-Q", "--"];
    args.extend(names.iter().map(String::as_str));
    debug!("$ pacman {}", args.join(" "));
    let output = tokio::process::Command::new("pacman")
        .args(&args)
        // pacman localizes its messages via gettext, so the "was not found"
        // check below only works against the untranslated output.
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    // pacman -Q exits 1 when any package is missing ("error: package 'x'
    // was not found" on stderr); installed ones still print to stdout.
    // Anything else on stderr (corrupt db, lock file) is a real error.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let only_missing = !stderr.is_empty()
        && stderr
            .lines()
            .all(|line| line.trim().is_empty() || line.contains("was not found"));
    if !output.status.success() && (output.status.code() != Some(1) || !only_missing) {
        bail!("pacman -Q failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn pacman_deptest(names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut args = vec!["-T", "--"];
    args.extend(names.iter().map(String::as_str));
    debug!("$ pacman {}", args.join(" "));
    let output = tokio::process::Command::new("pacman")
        .args(&args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    // deptest returns 127 when at least one requirement is unsatisfied and
    // prints those requirements to stdout. Both 0 and 127 are normal.
    if !matches!(output.status.code(), Some(0 | 127)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pacman -T failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[async_trait(?Send)]
impl SystemPackageManager for PacmanManager {
    fn name(&self) -> &str {
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
        let names = pkgs.iter().map(|pkg| pkg.name.clone()).collect::<Vec<_>>();
        let stdout = pacman_query(&names).await?;
        let mut statuses = parse_pacman_query(&stdout, pkgs);
        let apparent_missing = statuses
            .iter()
            .filter(|status| matches!(status.state, PackageState::Missing))
            .map(|status| status.request.name.clone())
            .collect::<Vec<_>>();
        if apparent_missing.is_empty() {
            return Ok(statuses);
        }

        // A bare query may answer a virtual package target under the installed
        // provider's real name. Deptest tells us which apparent misses are
        // genuinely unsatisfied; query the provider-satisfied names one at a
        // time so their returned versions can be associated positionally.
        let deptest = pacman_deptest(&apparent_missing).await?;
        let missing = parse_pacman_deptest(&deptest);
        for status in statuses
            .iter_mut()
            .filter(|status| matches!(status.state, PackageState::Missing))
        {
            if missing.contains(status.request.name.as_str()) {
                continue;
            }
            let output = pacman_query(std::slice::from_ref(&status.request.name)).await?;
            let provider = apply_provider_query(status, &output)?;
            debug!(
                "pacman: {} is satisfied by installed provider {provider}",
                status.request.name
            );
        }
        Ok(statuses)
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
        let names = pkgs.iter().map(|pkg| pkg.name.clone()).collect::<Vec<_>>();
        let stdout = pacman_query(&names).await?;
        let installed_names = stdout
            .lines()
            .filter_map(|line| line.split_once(' ').map(|(name, _)| name))
            .collect::<HashSet<_>>();
        let pkgs = pkgs
            .iter()
            .filter(|pkg| installed_names.contains(pkg.name.as_str()))
            .collect::<Vec<_>>();
        let skipped = names.len() - pkgs.len();
        if skipped > 0 {
            warn!(
                "pacman: {skipped} package(s) satisfied by an installed provider; skipping targeted upgrade"
            );
        }
        if pkgs.is_empty() {
            return Ok(());
        }
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

    #[test]
    fn test_apply_provider_query() {
        let mut status = PackageStatus {
            request: req("mariadb-clients", None),
            state: PackageState::Missing,
        };

        let provider =
            apply_provider_query(&mut status, "percona-server-clients 9.7.1_1-1\n").unwrap();

        assert_eq!(provider, "percona-server-clients");
        assert_eq!(
            status.state,
            PackageState::Installed {
                version: "9.7.1_1-1".to_string()
            }
        );
    }

    #[test]
    fn test_parse_pacman_deptest() {
        let missing = parse_pacman_deptest("missing-one\nmissing-two\n");
        assert_eq!(missing, HashSet::from(["missing-one", "missing-two"]));
    }
}
