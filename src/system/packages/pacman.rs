use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

/// Arch-family (Arch, Manjaro, EndeavourOS) via pacman
pub(crate) struct PacmanManager {}

impl PacmanManager {
    pub(crate) fn new() -> Self {
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

#[derive(Debug, PartialEq, Eq, Hash)]
struct PacmanProvide {
    name: String,
    version: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct PacmanPackageMetadata {
    name: String,
    version: String,
    provides: HashSet<PacmanProvide>,
}

fn parse_pacman_info(output: &str) -> Vec<PacmanPackageMetadata> {
    let mut packages = Vec::new();
    let mut name = None;
    let mut version = None;
    let mut provides = HashSet::new();
    let mut reading_provides = false;
    for line in output.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if let (Some(name), Some(version)) = (name.take(), version.take()) {
                packages.push(PacmanPackageMetadata {
                    name,
                    version,
                    provides: std::mem::take(&mut provides),
                });
            }
            reading_provides = false;
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            reading_provides = false;
            match key.trim() {
                "Name" => name = Some(value.trim().to_string()),
                "Version" => version = Some(value.trim().to_string()),
                "Provides" => {
                    reading_provides = true;
                    provides.extend(parse_provides(value));
                }
                _ => {}
            }
        } else if reading_provides {
            provides.extend(parse_provides(line));
        }
    }
    packages
}

fn parse_provides(value: &str) -> impl Iterator<Item = PacmanProvide> + '_ {
    value
        .split_whitespace()
        .filter(|provide| *provide != "None")
        .map(|provide| {
            let (name, version) = provide
                .split_once('=')
                .map(|(name, version)| (name, Some(version.to_string())))
                .unwrap_or_else(|| (provide.split(['<', '>']).next().unwrap_or(provide), None));
            PacmanProvide {
                name: name.to_string(),
                version,
            }
        })
}

fn find_provider<'a>(
    packages: &'a [PacmanPackageMetadata],
    request: &PackageRequest,
    constraint_satisfied: bool,
    eligible: impl Fn(&PacmanPackageMetadata) -> bool,
) -> Option<&'a PacmanPackageMetadata> {
    packages
        .iter()
        .filter(|package| eligible(package))
        .find(|package| {
            package.name == request.name
                && (!constraint_satisfied
                    || request
                        .version
                        .as_ref()
                        .is_none_or(|version| version_matches(version, &package.version)))
        })
        .or_else(|| {
            packages
                .iter()
                .filter(|package| eligible(package))
                .find(|package| {
                    package.provides.iter().any(|provide| {
                        provide.name == request.name
                            && (!constraint_satisfied
                                || request.version.as_ref().is_none_or(|version| {
                                    provide
                                        .version
                                        .as_ref()
                                        .is_some_and(|provided| version_matches(version, provided))
                                }))
                    })
                })
        })
}

fn matching_provider_names(packages: &[PacmanPackageMetadata], requested: &str) -> Vec<String> {
    packages
        .iter()
        .filter(|package| {
            package.name == requested
                || package
                    .provides
                    .iter()
                    .any(|provide| provide.name == requested)
        })
        .map(|package| package.name.clone())
        .collect()
}

fn version_matches(requested: &str, installed: &str) -> bool {
    installed == requested || installed.starts_with(&format!("{requested}-"))
}

fn parse_pacman_deptest(output: &str) -> HashSet<&str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn deptest_requirement(req: &PackageRequest) -> String {
    match &req.version {
        Some(version) => format!("{}={version}", req.name),
        None => req.name.clone(),
    }
}

fn remove_args(names: &[String]) -> Vec<String> {
    let mut args = vec![
        "-R".to_string(),
        "--noconfirm".to_string(),
        "--".to_string(),
    ];
    args.extend(names.iter().cloned());
    args
}

async fn concrete_remove_names(pkgs: &[PackageRequest]) -> Result<Vec<String>> {
    let info = pacman_info().await?;
    let packages = parse_pacman_info(&info);
    let mut names = Vec::new();
    for pkg in pkgs {
        let providers = matching_provider_names(&packages, &pkg.name);
        if providers.is_empty() {
            return Err(eyre::eyre!(
                "pacman -Qi returned no provider for satisfied requirement '{}'",
                pkg.name
            ));
        }
        for provider in providers {
            if !names.contains(&provider) {
                names.push(provider);
            }
        }
    }
    Ok(names)
}

fn apply_provider_query<'a>(
    status: &mut PackageStatus,
    packages: &'a [PacmanPackageMetadata],
    constraint_satisfied: bool,
) -> Result<&'a PacmanPackageMetadata> {
    let provider = find_provider(packages, &status.request, constraint_satisfied, |_| true)
        .ok_or_else(|| {
            eyre::eyre!(
                "pacman -Qi returned no provider for satisfied requirement '{}'",
                status.request.name
            )
        })?;
    // The provider's package version is display metadata; pacman -T evaluates
    // the requested version against the version declared in Provides.
    status.state = if constraint_satisfied {
        PackageState::Installed {
            version: provider.version.clone(),
        }
    } else {
        PackageState::VersionMismatch {
            installed: provider.version.clone(),
        }
    };
    Ok(provider)
}

async fn pacman_info() -> Result<String> {
    let args = ["-Qi"];
    debug!("$ pacman {}", args.join(" "));
    let output = tokio::process::Command::new("pacman")
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !output.status.success() {
        bail!(
            "pacman -Qi failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
            .map(|status| deptest_requirement(&status.request))
            .collect::<Vec<_>>();
        if apparent_missing.is_empty() {
            return Ok(statuses);
        }

        // Deptest tells us which apparent misses are genuinely unsatisfied.
        // Package metadata below maps satisfied virtual names back to their
        // concrete installed providers.
        let deptest = pacman_deptest(&apparent_missing).await?;
        let unsatisfied = parse_pacman_deptest(&deptest);
        // A failed version constraint can still have a provider for the bare
        // name. Distinguish that mismatch from a genuinely missing package.
        let versioned_unsatisfied = statuses
            .iter()
            .filter(|status| {
                status.request.version.is_some()
                    && unsatisfied.contains(deptest_requirement(&status.request).as_str())
            })
            .map(|status| status.request.name.clone())
            .collect::<Vec<_>>();
        let bare_deptest = pacman_deptest(&versioned_unsatisfied).await?;
        let bare_missing = parse_pacman_deptest(&bare_deptest);
        let info = pacman_info().await?;
        let packages = parse_pacman_info(&info);
        for status in statuses
            .iter_mut()
            .filter(|status| matches!(status.state, PackageState::Missing))
        {
            let constraint_satisfied =
                !unsatisfied.contains(deptest_requirement(&status.request).as_str());
            if !constraint_satisfied
                && (status.request.version.is_none()
                    || bare_missing.contains(status.request.name.as_str()))
            {
                continue;
            }
            let provider = apply_provider_query(status, &packages, constraint_satisfied)?;
            debug!(
                "pacman: {} is satisfied by installed provider {}",
                status.request.name, provider.name,
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

    fn supports_remove(&self) -> bool {
        true
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // A request may name a virtual capability satisfied through Provides.
        // Resolve each target through the local database immediately before
        // removal so pacman receives the concrete installed package identity.
        let names = concrete_remove_names(pkgs).await?;
        let args = remove_args(&names);
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
            desired: crate::system::packages::PackageDesiredState::Present,
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
            request: req("mariadb-clients", Some("12.3.2")),
            state: PackageState::Missing,
        };

        // pacman -T validated the version declared by Provides even though the
        // provider package has a different version of its own.
        let packages = parse_pacman_info(
            "Name            : percona-server-clients\n\
             Version         : 9.7.1_1-1\n\
             Provides        : mariadb-clients=12.3.2\n\
                               mysql-clients\n\
             Description     : database clients\n",
        );
        let provider = apply_provider_query(&mut status, &packages, true).unwrap();

        assert_eq!(provider.name, "percona-server-clients");
        assert!(
            provider
                .provides
                .iter()
                .any(|provide| provide.name == "mariadb-clients")
        );
        assert!(
            provider
                .provides
                .iter()
                .any(|provide| provide.name == "mysql-clients")
        );
        assert_eq!(
            status.state,
            PackageState::Installed {
                version: "9.7.1_1-1".to_string()
            }
        );
    }

    #[test]
    fn test_find_provider_prefers_exact_package_name() {
        let packages = parse_pacman_info(
            "Name            : alternate-foo\n\
             Version         : 2.0-1\n\
             Provides        : foo\n\
             \n\
             Name            : foo\n\
             Version         : 1.0-1\n\
             Provides        : None\n",
        );

        assert_eq!(
            find_provider(&packages, &req("foo", None), true, |_| true)
                .unwrap()
                .name,
            "foo"
        );
    }

    #[test]
    fn test_matching_provider_names_returns_every_provider() {
        let packages = parse_pacman_info(
            "Name            : foo-one\n\
             Version         : 1.0-1\n\
             Provides        : foo\n\
             \n\
             Name            : foo-two\n\
             Version         : 2.0-1\n\
             Provides        : foo\n",
        );

        assert_eq!(
            matching_provider_names(&packages, "foo"),
            ["foo-one", "foo-two"]
        );
    }

    #[test]
    fn test_remove_args_remove_exact_packages_without_cascading() {
        assert_eq!(
            remove_args(&["omarchy".to_string(), "omarchy-settings".to_string()]),
            ["-R", "--noconfirm", "--", "omarchy", "omarchy-settings"]
        );
    }

    #[test]
    fn test_apply_provider_query_version_mismatch() {
        let mut status = PackageStatus {
            request: req("virtual-package", Some("2.0")),
            state: PackageState::Missing,
        };

        let packages = parse_pacman_info(
            "Name            : provider-package\n\
             Version         : 2.0-1\n\
             Provides        : virtual-package=1.0\n",
        );
        apply_provider_query(&mut status, &packages, false).unwrap();

        assert_eq!(
            status.state,
            PackageState::VersionMismatch {
                installed: "2.0-1".to_string()
            }
        );
    }

    #[test]
    fn test_deptest_requirement_includes_version() {
        assert_eq!(
            deptest_requirement(&req("virtual-package", Some("2.0"))),
            "virtual-package=2.0"
        );
        assert_eq!(
            deptest_requirement(&req("virtual-package", None)),
            "virtual-package"
        );
    }

    #[test]
    fn test_parse_pacman_deptest() {
        let missing = parse_pacman_deptest("missing-one\nmissing-two\n");
        assert_eq!(missing, HashSet::from(["missing-one", "missing-two"]));
    }
}
