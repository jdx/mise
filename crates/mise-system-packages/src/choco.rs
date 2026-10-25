use std::process::Stdio;

use async_trait::async_trait;
use eyre::{Result, bail};

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};

// Chocolatey exits 1641 or 3010 when a package installed but asks for a reboot.
const REBOOT_INITIATED: i32 = 1641;
const REBOOT_REQUIRED: i32 = 3010;

/// Windows packages via Chocolatey.
pub(crate) struct ChocoManager {}

impl ChocoManager {
    /// Creates a Chocolatey package manager.
    pub(crate) fn new() -> Self {
        Self {}
    }
}

/// Builds a side-effect-free exact-name installed-state query. Chocolatey 1.x
/// searches the remote feed unless `--local-only` is given; 2.x lists only
/// local packages and may reject the flag, so it is optional.
fn list_args(request: &PackageRequest, local_only: bool) -> Vec<String> {
    let mut args = vec!["list".to_string()];
    if local_only {
        args.push("--local-only".to_string());
    }
    args.extend(["--exact", "--limit-output", request.name.as_str()].map(str::to_string));
    args
}

/// Builds non-interactive install or upgrade arguments for a package request.
fn package_args(command: &str, request: &PackageRequest) -> Vec<String> {
    let mut args = vec![command.to_string(), request.name.clone()];
    if let Some(version) = &request.version {
        args.extend(["--version".to_string(), version.clone()]);
    }
    if request.version.is_some() {
        // a pin below the installed version is a downgrade
        args.push("--allow-downgrade".to_string());
    }
    args.extend(["--yes".to_string(), "--no-progress".to_string()]);
    args
}

/// Extracts the installed version of `name` from `--limit-output` rows of the
/// form `name|version`. Chocolatey package IDs are case-insensitive.
fn parse_list_rows(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (id, version) = line.trim().split_once('|')?;
        (id.eq_ignore_ascii_case(name) && !version.is_empty()).then(|| version.to_string())
    })
}

/// Compares the installed version with an optional opaque version pin.
fn package_state(request: &PackageRequest, installed: String) -> PackageState {
    match &request.version {
        Some(requested) if *requested != installed => PackageState::VersionMismatch { installed },
        _ => PackageState::Installed { version: installed },
    }
}

/// Queries one package without changing Chocolatey state.
async fn query_package(request: &PackageRequest) -> Result<PackageStatus> {
    let mut output = run_list(request, true).await?;
    if rejected_local_only(&output) {
        // Chocolatey 2.x may reject --local-only
        output = run_list(request, false).await?;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // with useEnhancedExitCodes, a query with no results exits 2
    if output.status.code() == Some(2) {
        return Ok(PackageStatus {
            request: request.clone(),
            state: PackageState::Missing,
            display_name: None,
        });
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = [stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        bail!("choco list failed for '{}': {}", request.name, detail);
    }
    let state = match parse_list_rows(&stdout, &request.name) {
        Some(installed) => package_state(request, installed),
        None => PackageState::Missing,
    };
    Ok(PackageStatus {
        request: request.clone(),
        state,
        display_name: None,
    })
}

/// Whether a `--local-only` query failed because Chocolatey does not accept
/// the option. Any other failure must not be retried without it: on 1.x that
/// would search the remote feed, and a remote match would look installed.
fn rejected_local_only(output: &std::process::Output) -> bool {
    if output.status.success() || output.status.code() == Some(2) {
        return false;
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    text.contains("local-only") || text.contains("localonly") || text.contains("unknown option")
}

/// Runs one `choco list` query.
async fn run_list(request: &PackageRequest, local_only: bool) -> Result<std::process::Output> {
    let args = list_args(request, local_only);
    debug!("$ choco {}", args.join(" "));
    Ok(tokio::process::Command::new("choco")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?)
}

/// Returns whether an exit code means the command did its work.
fn command_succeeded(code: Option<i32>) -> bool {
    matches!(code, Some(0 | REBOOT_INITIATED | REBOOT_REQUIRED))
}

/// Applies a Chocolatey command to each requested package in order.
async fn run_packages(command: &str, pkgs: &[PackageRequest], dry_run: bool) -> Result<()> {
    for pkg in pkgs {
        let args = package_args(command, pkg);
        if dry_run {
            miseprintln!("choco {}", args.join(" "));
            continue;
        }
        debug!("$ choco {}", args.join(" "));
        let status = tokio::process::Command::new("choco")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await?;
        if !command_succeeded(status.code()) {
            bail!("choco {command} {} failed with {status}", pkg.name);
        }
        if matches!(status.code(), Some(REBOOT_INITIATED | REBOOT_REQUIRED)) {
            warn!("choco {command} {} needs a reboot to finish", pkg.name);
        }
    }
    Ok(())
}

#[async_trait(?Send)]
impl SystemPackageManager for ChocoManager {
    fn name(&self) -> &str {
        "choco"
    }

    fn is_available(&self) -> bool {
        cfg!(windows) && mise_util::file::which_spawnable("choco").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(windows) {
            "choco not found".to_string()
        } else {
            "only available on windows".to_string()
        }
    }

    /// Chocolatey package IDs are case-insensitive, so two spellings of one
    /// ID are one package.
    fn package_identity(&self, name: &str) -> Option<String> {
        Some(name.to_ascii_lowercase())
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let mut statuses = Vec::with_capacity(pkgs.len());
        for pkg in pkgs {
            statuses.push(query_package(pkg).await?);
        }
        Ok(statuses)
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        run_packages("install", pkgs, opts.dry_run).await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        run_packages("upgrade", pkgs, opts.dry_run).await
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
    fn parses_version_from_limit_output() {
        assert_eq!(
            parse_list_rows("git|2.47.1\n", "git"),
            Some("2.47.1".to_string())
        );
    }

    #[test]
    fn matches_the_id_case_insensitively_and_exactly() {
        let output = "Git.Install|2.47.1\r\ngit|2.40.0\r\n";
        assert_eq!(parse_list_rows(output, "GIT"), Some("2.40.0".to_string()));
        assert_eq!(parse_list_rows("git-lfs|3.6.0\n", "git"), None);
    }

    #[test]
    fn no_rows_is_missing() {
        assert_eq!(parse_list_rows("", "git"), None);
    }

    #[test]
    fn compares_pins_as_opaque_strings() {
        let request = req("git", Some("2.47.1.20241201"));
        assert_eq!(
            package_state(&request, "2.47.1.20241201".to_string()),
            PackageState::Installed {
                version: "2.47.1.20241201".to_string()
            }
        );
        assert_eq!(
            package_state(&request, "2.47.1".to_string()),
            PackageState::VersionMismatch {
                installed: "2.47.1".to_string()
            }
        );
        assert_eq!(
            package_state(&req("git", None), "2.47.1".to_string()),
            PackageState::Installed {
                version: "2.47.1".to_string()
            }
        );
    }

    #[test]
    fn list_queries_only_local_packages() {
        assert_eq!(
            list_args(&req("git", None), true),
            vec!["list", "--local-only", "--exact", "--limit-output", "git"]
        );
        assert_eq!(
            list_args(&req("git", None), false),
            vec!["list", "--exact", "--limit-output", "git"]
        );
    }

    #[test]
    fn builds_non_interactive_install_args_with_pin() {
        assert_eq!(
            package_args("install", &req("git", Some("2.47.1"))),
            vec![
                "install",
                "git",
                "--version",
                "2.47.1",
                "--allow-downgrade",
                "--yes",
                "--no-progress"
            ]
        );
    }

    #[test]
    fn builds_upgrade_args_without_latest_as_a_literal_version() {
        assert_eq!(
            package_args("upgrade", &req("git", None)),
            vec!["upgrade", "git", "--yes", "--no-progress"]
        );
    }

    #[test]
    fn reboot_exit_codes_are_successes() {
        assert!(command_succeeded(Some(0)));
        assert!(command_succeeded(Some(1641)));
        assert!(command_succeeded(Some(3010)));
        assert!(!command_succeeded(Some(1)));
        assert!(!command_succeeded(None));
    }

    #[cfg(unix)]
    fn output(code: i32, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn retries_only_when_local_only_is_rejected() {
        assert!(rejected_local_only(&output(
            1,
            "Unknown option: --local-only"
        )));
        assert!(!rejected_local_only(&output(
            1,
            "The source is unreachable"
        )));
        assert!(!rejected_local_only(&output(2, "no results")));
        assert!(!rejected_local_only(&output(0, "")));
    }
}
