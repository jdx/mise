use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;

// APPINSTALLER_CLI_ERROR_NO_APPLICATIONS_FOUND. WinGet returns the HRESULT as
// a signed process exit code on Windows.
const NO_APPLICATIONS_FOUND: i32 = -1_978_335_212;
const UPDATE_NOT_APPLICABLE: i32 = -1_978_335_189;

/// Windows Package Manager via winget.
pub(crate) struct WingetManager {}

impl WingetManager {
    /// Creates a WinGet package manager.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Refreshes configured WinGet sources before a mutating operation.
    async fn refresh(&self, dry_run: bool) -> Result<()> {
        let args = source_update_args();
        if dry_run {
            miseprintln!("winget {}", args.join(" "));
            return Ok(());
        }
        run_winget(&args, "source update", &[]).await
    }
}

/// Builds the non-interactive source refresh arguments.
fn source_update_args() -> Vec<String> {
    ["source", "update", "--disable-interactivity"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// Builds a query that accepts source agreements before a mutating operation.
fn accept_source_agreements_args(request: &PackageRequest) -> Vec<String> {
    let mut args = list_args(request);
    args.insert(args.len() - 1, "--accept-source-agreements".to_string());
    args
}

/// Builds a side-effect-free exact-ID installed-state query.
fn list_args(request: &PackageRequest) -> Vec<String> {
    [
        "list",
        "--id",
        request.name.as_str(),
        "--exact",
        "--disable-interactivity",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Builds exact-ID install or upgrade arguments for a package request.
fn package_args(command: &str, request: &PackageRequest) -> Vec<String> {
    let mut args = vec![
        command.to_string(),
        "--id".to_string(),
        request.name.clone(),
        "--exact".to_string(),
    ];
    if let Some(version) = &request.version {
        args.extend(["--version".to_string(), version.clone()]);
    }
    args.extend([
        "--silent".to_string(),
        "--accept-source-agreements".to_string(),
        "--accept-package-agreements".to_string(),
        "--disable-interactivity".to_string(),
    ]);
    args
}

/// Compares installed versions with an optional opaque version pin.
fn package_state(request: &PackageRequest, installed: &[String]) -> PackageState {
    if let Some(requested) = &request.version
        && installed.iter().any(|version| version == requested)
    {
        return PackageState::Installed {
            version: requested.clone(),
        };
    }

    let installed = installed[0].clone();
    match &request.version {
        Some(_) => PackageState::VersionMismatch { installed },
        None => PackageState::Installed { version: installed },
    }
}

/// Extracts every installed version from exact-ID WinGet list rows.
fn parse_list_rows(output: &str, package_id: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            line.match_indices(package_id)
                .filter_map(|(offset, _)| {
                    let before = &line[..offset];
                    let after = &line[offset + package_id.len()..];
                    let bounded_before = before.chars().next_back().is_none_or(char::is_whitespace);
                    let bounded_after = after.chars().next().is_none_or(char::is_whitespace);
                    if bounded_before && bounded_after {
                        after.split_whitespace().next().map(str::to_string)
                    } else {
                        None
                    }
                })
                .last()
        })
        .collect()
}

/// Classifies WinGet list output as installed, missing, or failed.
fn query_state(
    code: Option<i32>,
    stdout: &str,
    stderr: &str,
    request: &PackageRequest,
) -> Result<PackageState> {
    match code {
        Some(0) => {
            let installed = parse_list_rows(stdout, &request.name);
            if installed.is_empty() {
                return Err(eyre::eyre!(
                    "winget list succeeded but returned no parseable row for '{}'",
                    request.name
                ));
            }
            Ok(package_state(request, &installed))
        }
        Some(NO_APPLICATIONS_FOUND) => Ok(PackageState::Missing),
        _ => {
            let detail = [stdout.trim(), stderr.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            bail!("winget list failed for '{}': {}", request.name, detail);
        }
    }
}

/// Queries one package without accepting agreements or changing WinGet state.
async fn query_package(request: &PackageRequest) -> Result<PackageStatus> {
    let args = list_args(request);
    debug!("$ winget {}", args.join(" "));
    let output = tokio::process::Command::new("winget")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let state = query_state(output.status.code(), &stdout, &stderr, request)?;
    Ok(PackageStatus {
        request: request.clone(),
        state,
    })
}

/// Returns whether an exit code is successful for the current operation.
fn command_succeeded(code: Option<i32>, accepted_exit_codes: &[i32]) -> bool {
    code == Some(0) || code.is_some_and(|code| accepted_exit_codes.contains(&code))
}

/// Runs WinGet and accepts only zero plus explicitly allowed no-op codes.
async fn run_winget(args: &[String], action: &str, accepted_exit_codes: &[i32]) -> Result<()> {
    debug!("$ winget {}", args.join(" "));
    let status = tokio::process::Command::new("winget")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    if !command_succeeded(status.code(), accepted_exit_codes) {
        bail!("winget {action} failed with {status}");
    }
    Ok(())
}

/// Applies a WinGet command to each requested package in order.
async fn run_packages(
    command: &str,
    pkgs: &[PackageRequest],
    dry_run: bool,
    accepted_exit_codes: &[i32],
) -> Result<()> {
    for pkg in pkgs {
        let args = package_args(command, pkg);
        if dry_run {
            miseprintln!("winget {}", args.join(" "));
        } else {
            run_winget(
                &args,
                &format!("{command} {}", pkg.name),
                accepted_exit_codes,
            )
            .await?;
        }
    }
    Ok(())
}

#[async_trait(?Send)]
impl SystemPackageManager for WingetManager {
    fn name(&self) -> &str {
        "winget"
    }

    fn is_available(&self) -> bool {
        cfg!(windows) && crate::file::which_spawnable("winget").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(windows) {
            "winget not found".to_string()
        } else {
            "only available on windows".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let mut statuses = Vec::with_capacity(pkgs.len());
        for pkg in pkgs {
            statuses.push(query_package(pkg).await?);
        }
        Ok(statuses)
    }

    async fn prepare_mutation(&self, pkgs: &[PackageRequest]) -> Result<()> {
        let Some(pkg) = pkgs.first() else {
            return Ok(());
        };
        let args = accept_source_agreements_args(pkg);
        debug!("$ winget {}", args.join(" "));
        let output = tokio::process::Command::new("winget")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .await?;
        if !command_succeeded(output.code(), &[NO_APPLICATIONS_FOUND]) {
            bail!("winget source agreement preparation failed with {output}");
        }
        Ok(())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.update && !pkgs.is_empty() {
            self.refresh(opts.dry_run).await?;
        }
        run_packages("install", pkgs, opts.dry_run, &[]).await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if !pkgs.is_empty() {
            self.refresh(opts.dry_run).await?;
        }
        run_packages("upgrade", pkgs, opts.dry_run, &[UPDATE_NOT_APPLICABLE]).await
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
    fn parses_version_after_exact_id() {
        let output = "Name                      Id                  Version Available Source\n\
                      ------------------------------------------------------------------------\n\
                      PowerToys (Preview) ARM64 Microsoft.PowerToys 0.86.0  0.101.0   winget\n";
        assert_eq!(
            parse_list_rows(output, "Microsoft.PowerToys"),
            vec!["0.86.0"]
        );
    }

    #[test]
    fn parses_row_when_name_contains_the_id() {
        let output = "Example.Tool Example.Tool 1.2.3 winget\n";
        assert_eq!(parse_list_rows(output, "Example.Tool"), vec!["1.2.3"]);
    }

    #[test]
    fn parses_id_column_when_source_name_matches_the_id() {
        let output = "Example Example.Tool 1.2.3 Example.Tool\n";
        assert_eq!(parse_list_rows(output, "Example.Tool"), vec!["1.2.3"]);
    }

    #[test]
    fn compares_pins_as_opaque_strings() {
        let request = req("Example.Tool", Some("2026-preview.1"));
        assert_eq!(
            package_state(&request, &["2026-preview.1".to_string()]),
            PackageState::Installed {
                version: "2026-preview.1".to_string()
            }
        );
        assert_eq!(
            package_state(&request, &["2026-preview.2".to_string()]),
            PackageState::VersionMismatch {
                installed: "2026-preview.2".to_string()
            }
        );
    }

    #[test]
    fn finds_requested_version_in_a_later_duplicate_row() {
        let output = "Example Example.Tool 1.0.0 winget\n\
                      Example Example.Tool 2.0.0 winget\n";
        assert_eq!(
            query_state(Some(0), output, "", &req("Example.Tool", Some("2.0.0")),).unwrap(),
            PackageState::Installed {
                version: "2.0.0".to_string()
            }
        );
    }

    #[test]
    fn conflicting_duplicate_rows_fall_back_to_the_first_version() {
        let output = "Example Example.Tool 2.0.0 winget\n\
                      Example Example.Tool 1.0.0 winget\n";
        assert_eq!(
            query_state(Some(0), output, "", &req("Example.Tool", Some("3.0.0"))).unwrap(),
            PackageState::VersionMismatch {
                installed: "2.0.0".to_string()
            }
        );
    }

    #[test]
    fn builds_exact_non_interactive_install_args_with_pin() {
        assert_eq!(
            package_args("install", &req("Example.Tool", Some("1.2.3"))),
            vec![
                "install",
                "--id",
                "Example.Tool",
                "--exact",
                "--version",
                "1.2.3",
                "--silent",
                "--accept-source-agreements",
                "--accept-package-agreements",
                "--disable-interactivity",
            ]
        );
    }

    #[test]
    fn list_is_non_interactive_without_accepting_agreements() {
        assert_eq!(
            list_args(&req("Example.Tool", None)),
            vec![
                "list",
                "--id",
                "Example.Tool",
                "--exact",
                "--disable-interactivity",
            ]
        );
    }

    #[test]
    fn source_update_uses_only_supported_noninteractive_flags() {
        assert_eq!(
            source_update_args(),
            vec!["source", "update", "--disable-interactivity"]
        );
    }

    #[test]
    fn mutation_preparation_accepts_source_agreements_via_list() {
        assert_eq!(
            accept_source_agreements_args(&req("Example.Tool", None)),
            vec![
                "list",
                "--id",
                "Example.Tool",
                "--exact",
                "--accept-source-agreements",
                "--disable-interactivity",
            ]
        );
    }

    #[test]
    fn builds_upgrade_args_without_latest_as_a_literal_version() {
        let args = package_args("upgrade", &req("Example.Tool", None));
        assert_eq!(
            args,
            vec![
                "upgrade",
                "--id",
                "Example.Tool",
                "--exact",
                "--silent",
                "--accept-source-agreements",
                "--accept-package-agreements",
                "--disable-interactivity",
            ]
        );
    }

    #[test]
    fn documents_the_no_applications_hresult() {
        assert_eq!(NO_APPLICATIONS_FOUND, -1_978_335_212);
        assert_eq!(NO_APPLICATIONS_FOUND as u32, 0x8A15_0014);
    }

    #[test]
    fn upgrade_not_applicable_is_an_accepted_no_op() {
        assert_eq!(UPDATE_NOT_APPLICABLE as u32, 0x8A15_002B);
        assert!(command_succeeded(
            Some(UPDATE_NOT_APPLICABLE),
            &[UPDATE_NOT_APPLICABLE]
        ));
        assert!(!command_succeeded(Some(UPDATE_NOT_APPLICABLE), &[]));
        assert!(!command_succeeded(Some(-1_978_335_188), &[]));
    }

    #[test]
    fn missing_hresult_is_a_missing_package() {
        assert_eq!(
            query_state(
                Some(NO_APPLICATIONS_FOUND),
                "No installed package found matching input criteria.",
                "",
                &req("Example.Missing", None),
            )
            .unwrap(),
            PackageState::Missing
        );
    }

    #[test]
    fn other_query_failures_are_propagated() {
        let err = query_state(
            Some(-1_978_335_211),
            "",
            "No sources are configured.",
            &req("Example.Tool", None),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "winget list failed for 'Example.Tool': No sources are configured."
        );
    }
}
