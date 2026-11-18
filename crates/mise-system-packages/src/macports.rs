use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::{Result, bail};

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use mise_ui::sudo;

/// Where the MacPorts installer puts `port`. It only adds `/opt/local/bin` to
/// PATH in shell profiles, so a non-login shell, or a mise process started
/// before a `pre-packages` hook installed MacPorts, will not find it on PATH.
const DEFAULT_PORT: &str = "/opt/local/bin/port";

/// Words `port` treats as pseudo-ports (sets of ports) or logical operators
/// when they appear where a port name is expected.
const RESERVED_NAMES: &[&str] = &[
    "all",
    "and",
    "active",
    "actinact",
    "current",
    "inactive",
    "installed",
    "leaves",
    "not",
    "obsolete",
    "or",
    "outdated",
    "requested",
    "rleaves",
    "uninstalled",
    "unrequested",
];

/// macOS MacPorts via `port`
pub(crate) struct MacportsManager;

/// The `port` executable: the one on PATH, else the default prefix's. Resolved
/// on each call so a `pre-packages` hook that installs MacPorts is noticed.
fn port_bin() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    // Not `file::which`: it caches a miss for the whole process, which would
    // hide a `port` that a `pre-packages` hook installs after an earlier check,
    // and it accepts a directory named `port`.
    let usable = |path: &PathBuf| path.is_file() && mise_util::file::is_executable(path);
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("port"))
            .find(usable)
    });
    on_path.or_else(|| Some(PathBuf::from(DEFAULT_PORT)).filter(usable))
}

fn port_path() -> Result<String> {
    match port_bin() {
        Some(path) => Ok(path.to_string_lossy().into_owned()),
        None => bail!("port not found on PATH or at {DEFAULT_PORT}"),
    }
}

/// `port` has no `--` separator and parses its arguments for options, variants
/// (`+name`), version selectors (`@1.0`), globs and `field:value` selectors, so
/// a name that is not a plain port name must never reach it. Port names are
/// letters, digits, and `.`, `_`, `+`, `-`, and cannot start with a symbol.
pub fn validate_name(name: &str) -> Result<()> {
    let plain = name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'));
    if !plain {
        bail!(
            "'macports:{name}' is not a valid port name; use letters, digits, '.', '_', '+', and '-', starting with a letter or digit"
        );
    }
    if RESERVED_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
        bail!("'macports:{name}' is a port pseudo-name or operator, not a port");
    }
    Ok(())
}

fn validate_names(pkgs: &[PackageRequest]) -> Result<()> {
    pkgs.iter().try_for_each(|pkg| validate_name(&pkg.name))
}

/// `port -N <action> <names>`; `-N` makes port fail instead of prompting.
fn args(action: &str, pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec!["-N".to_string(), action.to_string()];
    args.extend(pkgs.iter().map(|pkg| pkg.name.clone()));
    args
}

/// Run `port` through the configured elevator, or print the command for a
/// dry run.
fn run(port: &str, args: Vec<String>, opts: &InstallOpts) -> Result<()> {
    if opts.dry_run {
        miseprintln!("{}", sudo::argv(port, &args).join(" "));
        return Ok(());
    }
    sudo::run(port, &args, &[])
}

/// Refresh the ports tree. Unlike `port selfupdate`, this leaves MacPorts
/// itself alone, so an update never changes the tool that is running it.
fn sync(port: &str, opts: &InstallOpts) -> Result<()> {
    run(port, vec!["-N".to_string(), "sync".to_string()], opts)
}

/// One installed port from `port -q installed`:
/// `  git @2.47.0_0+credential_osxkeychain (active)`.
struct InstalledPort<'a> {
    name: &'a str,
    /// everything after `@`: `version_revision+variants`
    spec: &'a str,
    version: &'a str,
    revision: &'a str,
    active: bool,
}

impl InstalledPort<'_> {
    /// Whether a pin names this installation: the bare version, the
    /// `version_revision`, or the whole spec with variants.
    fn matches(&self, pin: &str) -> bool {
        pin == self.spec
            || pin == self.version
            || pin == format!("{}_{}", self.version, self.revision)
    }
}

fn parse_line(line: &str) -> Option<InstalledPort<'_>> {
    let (name, rest) = line.strip_prefix("  ")?.split_once(" @")?;
    let (spec, flags) = rest.split_once(' ').unwrap_or((rest, ""));
    // Variants (`+name`, or `-name` for a negated one) start at the first `+`
    // or `-`; their names may contain `_`. The revision is the last `_<digits>`
    // before them, and a version may itself contain `_`.
    let release = spec.split(['+', '-']).next()?;
    let (version, revision) = release.rsplit_once('_')?;
    if revision.is_empty() || !revision.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(InstalledPort {
        name,
        spec,
        version,
        revision,
        active: flags.starts_with("(active)"),
    })
}

/// Match `port installed` output to the requests. Only the active version
/// counts: an inactive one is on disk but unused, and `port install`
/// re-activates it. Versions are compared as strings, never ordered.
fn parse_installed(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let active = output
        .lines()
        .filter_map(parse_line)
        .filter(|port| port.active)
        .collect::<Vec<_>>();
    requests
        .iter()
        .map(|request| {
            // port names are case-insensitive
            let found = active
                .iter()
                .find(|port| port.name.eq_ignore_ascii_case(&request.name));
            let state = match (found, &request.version) {
                (None, _) => PackageState::Missing,
                (Some(port), Some(pin)) if !port.matches(pin) => PackageState::VersionMismatch {
                    installed: port.spec.to_string(),
                },
                (Some(port), _) => PackageState::Installed {
                    version: port.spec.to_string(),
                },
            };
            PackageStatus {
                request: request.clone(),
                state,
                display_name: None,
            }
        })
        .collect()
}

#[async_trait(?Send)]
impl SystemPackageManager for MacportsManager {
    fn name(&self) -> &str {
        "macports"
    }

    fn is_available(&self) -> bool {
        port_bin().is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "macos") {
            format!("port not found on PATH or at {DEFAULT_PORT}")
        } else {
            "only available on macos".to_string()
        }
    }

    /// A ports tree holds only each port's current Portfile, so a pin can be
    /// checked against what is installed but never installed.
    fn supports_version_pins(&self) -> bool {
        false
    }

    fn package_identity(&self, name: &str) -> Option<String> {
        Some(name.to_ascii_lowercase())
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        validate_names(pkgs)?;
        let port = port_path()?;
        let names = pkgs.iter().map(|pkg| pkg.name.clone()).collect::<Vec<_>>();
        debug!("$ {port} -q installed {}", names.join(" "));
        // reads the registry: no root, network, or ports tree needed
        let output = tokio::process::Command::new(&port)
            .args(["-q", "installed"])
            .args(&names)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .await?;
        if !output.status.success() {
            bail!(
                "port installed failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(parse_installed(
            &String::from_utf8_lossy(&output.stdout),
            pkgs,
        ))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        validate_names(pkgs)?;
        let port = port_path()?;
        if opts.update {
            sync(&port, opts)?;
        }
        run(&port, args("install", pkgs), opts)
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        validate_names(pkgs)?;
        let port = port_path()?;
        sync(&port, opts)?;
        run(&port, args("upgrade", pkgs), opts)
    }

    fn supports_remove(&self) -> bool {
        true
    }

    /// port refuses to uninstall a port that other installed ports depend on;
    /// mise surfaces that error rather than passing `--follow-dependents`.
    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        validate_names(pkgs)?;
        let port = port_path()?;
        // After an upgrade an older version may stay installed and inactive;
        // `port -N uninstall git` then refuses the ambiguous name. Name the
        // active version so exactly that registry entry is removed.
        let installed = self.installed(pkgs).await?;
        let mut args = vec!["-N".to_string(), "uninstall".to_string()];
        for (pkg, status) in pkgs.iter().zip(&installed) {
            args.push(pkg.name.clone());
            if let PackageState::Installed { version } = &status.state {
                args.push(format!("@{version}"));
            }
        }
        run(&port, args, opts)
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
            desired: crate::PackageDesiredState::Present,
        }
    }

    #[test]
    fn parses_version_revision_and_variants() {
        let port = parse_line("  git @2.47.0_1+credential_osxkeychain+perl5_34 (active)").unwrap();
        assert_eq!(port.name, "git");
        assert_eq!(port.version, "2.47.0");
        assert_eq!(port.revision, "1");
        assert_eq!(port.spec, "2.47.0_1+credential_osxkeychain+perl5_34");
        assert!(port.active);

        let inactive = parse_line("  bc @1.07.1_0").unwrap();
        assert!(!inactive.active);
        assert_eq!(inactive.version, "1.07.1");

        // underscores inside the version stay in the version
        assert_eq!(
            parse_line("  foo @2024_beta_3").unwrap().version,
            "2024_beta"
        );
        assert!(parse_line("The following ports are currently installed:").is_none());
        assert!(parse_line("  foo @1.0").is_none());

        // negated variants print with `-`
        let negated = parse_line("  git @2.47.0_0-x11 (active)").unwrap();
        assert_eq!((negated.version, negated.revision), ("2.47.0", "0"));
        let mixed = parse_line("  git @2.47.0_1-x11+perl5_34 (active)").unwrap();
        assert_eq!((mixed.version, mixed.revision), ("2.47.0", "1"));
        // a version that itself contains `_<digits>` keeps it
        let underscored = parse_line("  tool @1_2_3+x_4 (active)").unwrap();
        assert_eq!((underscored.version, underscored.revision), ("1_2", "3"));
    }

    #[test]
    fn only_active_ports_count_as_installed() {
        let output = "  git @2.46.0_0 \n  git @2.47.0_0+perl5_34 (active)\n  bc @1.07.1_0\n";
        let statuses = parse_installed(
            output,
            &[req("git", None), req("bc", None), req("ripgrep", None)],
        );
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "2.47.0_0+perl5_34".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        assert_eq!(statuses[2].state, PackageState::Missing);
    }

    #[test]
    fn pins_match_version_revision_or_full_spec() {
        let output = "  git @2.47.0_1+perl5_34 (active)\n";
        for pin in ["2.47.0", "2.47.0_1", "2.47.0_1+perl5_34"] {
            assert!(
                matches!(
                    parse_installed(output, &[req("git", Some(pin))])[0].state,
                    PackageState::Installed { .. }
                ),
                "{pin}"
            );
        }
        for pin in ["2.47", "2.47.0_0", "2.46.0"] {
            assert!(
                matches!(
                    parse_installed(output, &[req("git", Some(pin))])[0].state,
                    PackageState::VersionMismatch { .. }
                ),
                "{pin}"
            );
        }
    }

    #[test]
    fn names_match_case_insensitively() {
        let statuses = parse_installed("  Git @2.47.0_0 (active)\n", &[req("git", None)]);
        assert!(matches!(statuses[0].state, PackageState::Installed { .. }));
    }

    #[test]
    fn rejects_names_port_would_misread() {
        for bad in [
            "",
            "-f",
            "+universal",
            "@1.0",
            "git@2",
            "git*",
            "name:git",
            "outdated",
            "Installed",
            "a b",
            "git;ls",
        ] {
            assert!(validate_names(&[req(bad, None)]).is_err(), "{bad:?}");
        }
        for good in [
            "git",
            "py312-numpy",
            "gcc14",
            "libc++",
            "p5.34-xml_parser",
            "7zip",
        ] {
            assert!(validate_names(&[req(good, None)]).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn builds_noninteractive_args() {
        assert_eq!(
            args("install", &[req("git", None), req("bc", None)]),
            ["-N", "install", "git", "bc"]
        );
    }
}
