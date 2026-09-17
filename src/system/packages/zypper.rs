use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::errors::Error;
use crate::result::Result;
use crate::system::sudo;

/// openSUSE and SUSE Linux Enterprise packages via zypper.
pub(crate) struct ZypperManager;

fn package_args(command: &str, pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec!["--non-interactive".to_string(), command.to_string()];
    if command == "install" && pkgs.iter().any(|pkg| pkg.version.is_some()) {
        // An explicit pin may require a downgrade, but never a forced reinstall.
        args.push("--oldpackage".to_string());
    }
    args.push("--".to_string());
    args.extend(pkgs.iter().map(|pkg| match &pkg.version {
        Some(version) if command != "remove" => format!("{}={version}", pkg.name),
        _ => pkg.name.clone(),
    }));
    args
}

fn run(args: &[String], opts: &InstallOpts) -> Result<()> {
    if opts.dry_run {
        miseprintln!("{}", sudo::argv("zypper", args).join(" "));
        return Ok(());
    }
    match sudo::run("zypper", args, &[]) {
        Err(err) => {
            // These are successful transactions with an informational status.
            // Do not swallow missing packages (104), skipped repositories (106),
            // or failed RPM scripts (107).
            match err.downcast_ref::<Error>() {
                Some(Error::ScriptFailed(_, Some(status), _)) => match status.code() {
                    Some(102) => warn!("zypper: a system reboot is required"),
                    Some(103) => warn!("zypper: restart the package manager and rerun the command"),
                    _ => return Err(err),
                },
                _ => return Err(err),
            }
            Ok(())
        }
        result => result,
    }
}

fn refresh(opts: &InstallOpts) -> Result<()> {
    run(
        &["--non-interactive".to_string(), "refresh".to_string()],
        opts,
    )
}

fn parse_rpm_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    // RPM can report multiple installed versions of a package (e.g. kernels).
    // Any matching version satisfies a pin, independently of output order.
    let mut installed: HashMap<&str, Vec<&str>> = HashMap::new();
    for line in output.lines() {
        if let Some((name, version)) = line.split_once('\t') {
            installed.entry(name).or_default().push(version);
        }
    }
    requests
        .iter()
        .map(|request| {
            let state = match installed.get(request.name.as_str()) {
                Some(versions) => {
                    let matching = versions.iter().find(|version| {
                        request.version.as_ref().is_none_or(|requested| {
                            **version == requested || version.starts_with(&format!("{requested}-"))
                        })
                    });
                    match matching {
                        Some(version) => PackageState::Installed {
                            version: version.to_string(),
                        },
                        None => PackageState::VersionMismatch {
                            installed: versions[0].to_string(),
                        },
                    }
                }
                None => PackageState::Missing,
            };
            PackageStatus {
                request: request.clone(),
                state,
            }
        })
        .collect()
}

#[async_trait(?Send)]
impl SystemPackageManager for ZypperManager {
    fn name(&self) -> &str {
        "zypper"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux")
            && crate::file::which("zypper").is_some()
            && crate::file::which("rpm").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if !cfg!(target_os = "linux") {
            "only available on linux".to_string()
        } else if crate::file::which("zypper").is_none() {
            "zypper not found".to_string()
        } else {
            "rpm not found".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        // Query the local database without refreshing repositories, taking the
        // zypp lock, or elevating. Missing names produce unformatted lines.
        let output = tokio::process::Command::new("rpm")
            .args(["-q", "--qf", "%{NAME}\\t%{VERSION}-%{RELEASE}\\n", "--"])
            .args(pkgs.iter().map(|pkg| &pkg.name))
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .stdin(Stdio::null())
            .output()
            .await?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success()
            && (output.status.code().is_none()
                || stderr.lines().any(|line| {
                    !line.trim().is_empty()
                        && !line.contains("is not installed")
                        && !line.contains("no packages")
                }))
        {
            bail!("rpm -q failed ({}): {}", output.status, stderr.trim());
        }
        Ok(parse_rpm_query(
            &String::from_utf8_lossy(&output.stdout),
            pkgs,
        ))
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        if opts.update {
            refresh(opts)?;
        }
        run(&package_args("install", pkgs), opts)
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        refresh(opts)?;
        // The driver has already filtered to installed packages. `install`
        // updates them and also honors pins that require a downgrade.
        run(&package_args("install", pkgs), opts)
    }

    fn supports_remove(&self) -> bool {
        true
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if pkgs.is_empty() {
            return Ok(());
        }
        run(&package_args("remove", pkgs), opts)
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
            desired: super::super::PackageDesiredState::Present,
        }
    }

    #[test]
    fn rpm_versions_and_missing_packages() {
        let requests = [
            req("git", None),
            req("missing", None),
            req("git-core", None),
            req("bash", Some("5.2.37-2.1")),
            req("bash", Some("5.2.37")),
            req("bash", Some("5.2")),
            req("bash", Some("5.2.37-1.1")),
            req("kernel-default", Some("6.12.1")),
        ];
        let output = "git\t2.49.0-1.1\npackage missing is not installed\nbash\t5.2.37-2.1\nkernel-default\t6.12.1-1.1\nkernel-default\t6.13.0-1.1\n";
        let statuses = parse_rpm_query(output, &requests);
        for index in [0, 3, 4, 7] {
            assert!(statuses[index].state.is_installed());
        }
        for index in [1, 2] {
            assert_eq!(statuses[index].state, PackageState::Missing);
        }
        for index in [5, 6] {
            assert_eq!(
                statuses[index].state,
                PackageState::VersionMismatch {
                    installed: "5.2.37-2.1".to_string(),
                }
            );
        }
    }
}
