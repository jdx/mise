//! Host requirements describe prerequisites, not dependencies to install.
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::{Result, bail};
use packslip::model::Artifact;

use crate::cmd::CmdLineRunner;
use crate::file;

#[derive(Debug, Default)]
pub(crate) struct Report {
    pub(crate) failures: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

impl Report {
    pub(crate) fn enforce(&self, ignore: bool) -> Result<()> {
        for warning in &self.warnings {
            warn!("packslip requirement: {warning}");
        }
        if self.failures.is_empty() {
            return Ok(());
        }
        let failures = self.failures.join("; ");
        if ignore {
            warn!("overriding packslip host requirements: {failures}");
            Ok(())
        } else {
            bail!(
                "packslip host requirements not met: {failures}; set ignore_requirements=true for this tool to override"
            )
        }
    }
}

/// Compare arbitrarily large numeric components without overflow or semver.
fn numeric_cmp(actual: &str, min: &str) -> Option<Ordering> {
    fn parts(s: &str) -> Option<Vec<&str>> {
        s.split('.')
            .map(|p| {
                if p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                let p = p.trim_start_matches('0');
                Some(if p.is_empty() { "0" } else { p })
            })
            .collect()
    }
    let a = parts(actual)?;
    let b = parts(min)?;
    for i in 0..a.len().max(b.len()) {
        let a = a.get(i).copied().unwrap_or("0");
        let b = b.get(i).copied().unwrap_or("0");
        let cmp = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
        if cmp != Ordering::Equal {
            return Some(cmp);
        }
    }
    Some(Ordering::Equal)
}

fn version_from_output(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|s| s.trim_matches(['"', '\'', '(', ')', '[', ']']))
        .find(|s| numeric_cmp(s, "0").is_some())
        .map(str::to_owned)
}

async fn probe(program: &str, args: &[&str]) -> Option<String> {
    CmdLineRunner::new(program)
        .args(args)
        .read_isolated(64 * 1024)
        .await
        .ok()
}

/// `uname -r` reports a release, not a version: `6.8.0-31-generic` on one
/// distribution and `6.6.87.2-microsoft-standard-WSL2` on another. What
/// follows the dotted numbers is the distribution's own business, and an
/// `os_min` is a lower bound on the numbers, so read those and stop.
///
/// Windows reads its version from a sentence `ver` prints, so nothing there
/// calls this and the compiler would rightly call it dead.
#[cfg(not(windows))]
fn release_version(output: &str) -> Option<String> {
    let word = output.split_whitespace().next()?;
    let end = word
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(word.len());
    let version = word[..end].trim_end_matches('.');
    numeric_cmp(version, "0").map(|_| version.to_owned())
}

async fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    let version = probe("/usr/bin/sw_vers", &["-productVersion"])
        .await
        .as_deref()
        .and_then(release_version);
    // `Microsoft Windows [Version 10.0.19045.5011]`, so read it by the word.
    #[cfg(windows)]
    let version = probe("cmd.exe", &["/D", "/C", "ver"])
        .await
        .as_deref()
        .and_then(version_from_output);
    #[cfg(not(any(target_os = "macos", windows)))]
    let version = probe("uname", &["-r"])
        .await
        .as_deref()
        .and_then(release_version);
    version
}

async fn library_present(name: &str) -> Option<bool> {
    if !file::is_plain_file_name(name) {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let mut paths: Vec<PathBuf> = std::env::var_os("LD_LIBRARY_PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        paths.extend(["/lib", "/lib64", "/usr/lib", "/usr/lib64"].map(PathBuf::from));
        if paths.iter().any(|p| p.join(name).is_file()) {
            return Some(true);
        }
        // Cache failure is unknown, not proof of absence. The cache normally
        // covers distribution-specific multiarch directories as well.
        let cache = probe("/sbin/ldconfig", &["-p"]).await?;
        let arch = std::env::consts::ARCH;
        Some(cache.lines().any(|line| {
            line.split_whitespace().next() == Some(name)
                && match arch {
                    "x86_64" => line.contains("x86-64"),
                    "aarch64" => line.contains("AArch64"),
                    _ => true,
                }
                && line
                    .split("=>")
                    .nth(1)
                    .is_some_and(|p| std::path::Path::new(p.trim()).is_file())
        }))
    }
    #[cfg(target_os = "macos")]
    {
        let mut paths: Vec<PathBuf> = std::env::var_os("DYLD_LIBRARY_PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        paths.extend(["/usr/local/lib", "/usr/lib", "/opt/homebrew/lib"].map(PathBuf::from));
        if paths.iter().any(|p| p.join(name).is_file()) {
            return Some(true);
        }
        // The dyld shared cache can contain libraries without filesystem
        // entries. Without querying it we cannot establish absence.
        None
    }
    #[cfg(windows)]
    {
        let mut paths: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        if let Some(root) = std::env::var_os("SystemRoot") {
            paths.push(PathBuf::from(root).join("System32"));
        }
        Some(paths.iter().any(|p| p.join(name).is_file()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        None
    }
}

fn check_min(report: &mut Report, label: &str, actual: Option<&str>, min: &str, fatal: bool) {
    match actual.and_then(|actual| numeric_cmp(actual, min)) {
        Some(Ordering::Less) => {
            let message = format!("{label} {} is below required {min}", actual.unwrap());
            if fatal {
                report.failures.push(message);
            } else {
                report.warnings.push(message);
            }
        }
        Some(_) => {}
        None => report.warnings.push(format!("cannot check {label}>={min}")),
    }
}

/// `commands` contains paths resolved from the install's active mise toolset.
/// Ambient PATH supplies commands not managed by that toolset.
pub(crate) async fn check(artifact: &Artifact, commands: &BTreeMap<String, PathBuf>) -> Report {
    let mut report = Report::default();
    let Some(req) = &artifact.requires else {
        return report;
    };
    if let Some(min) = &req.os_min {
        check_min(&mut report, "OS", os_version().await.as_deref(), min, true);
    }
    if let Some(min) = &req.glibc_min {
        let glibc = if cfg!(target_os = "linux") {
            probe("getconf", &["GNU_LIBC_VERSION"])
                .await
                .as_deref()
                .and_then(version_from_output)
        } else {
            None
        };
        check_min(&mut report, "glibc", glibc.as_deref(), min, true);
    }
    for lib in req.libs.iter().flatten() {
        match library_present(lib).await {
            Some(true) => {}
            Some(false) => report.failures.push(format!(
                "shared library {lib} was not found by the host loader search"
            )),
            None => report
                .warnings
                .push(format!("cannot check shared library {lib}")),
        }
    }
    for bin in &req.bin {
        let path = commands
            .get(&bin.name)
            .cloned()
            // `which` matches the bare name, so on Windows `git.exe` and
            // `node.cmd` read as missing; `which_spawnable` applies PATHEXT
            // and only offers a path the OS will start.
            .or_else(|| file::which_spawnable(&bin.name));
        let Some(path) = path else {
            report.warnings.push(format!(
                "command {}{} is missing; install it before using features that need it",
                bin.name,
                bin.min
                    .as_deref()
                    .map(|v| format!(">={v}"))
                    .unwrap_or_default()
            ));
            continue;
        };
        if let Some(min) = &bin.min {
            let output = CmdLineRunner::new(path)
                .arg("--version")
                .read_isolated(64 * 1024)
                .await
                .ok();
            let version = output.as_deref().and_then(version_from_output);
            check_min(&mut report, &bin.name, version.as_deref(), min, false);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lower_bounds_are_numeric_and_unknown_is_a_warning() {
        for (a, b, order) in [
            ("21", "17", Ordering::Greater),
            ("2.10", "2.9", Ordering::Greater),
            ("17", "17.0.0", Ordering::Equal),
            ("0002.09", "2.9", Ordering::Equal),
            ("999999999999999999999999999999", "21", Ordering::Greater),
        ] {
            assert_eq!(numeric_cmp(a, b), Some(order));
        }
        for bad in ["", "v2.0", "2.1-rc1", "2..0", "-1"] {
            assert_eq!(numeric_cmp(bad, "1"), None);
        }
        let mut r = Report::default();
        check_min(&mut r, "OS", Some("12"), "13", true);
        check_min(&mut r, "java", Some("11"), "17", false);
        check_min(&mut r, "glibc", None, "2.31", true);
        assert_eq!(r.failures.len(), 1);
        assert_eq!(r.warnings.len(), 2);
        assert!(r.enforce(false).is_err());
        assert!(r.enforce(true).is_ok());
        assert_eq!(
            version_from_output("openjdk version \"21.0.4\""),
            Some("21.0.4".into())
        );
        assert_eq!(
            version_from_output("git version 2.49.0"),
            Some("2.49.0".into())
        );
        // A kernel release carries a suffix; an `os_min` bounds the numbers.
        #[cfg(not(windows))]
        for (release, version) in [
            ("6.8.0-31-generic", Some("6.8.0")),
            ("6.6.87.2-microsoft-standard-WSL2", Some("6.6.87.2")),
            ("5.15", Some("5.15")),
            ("15.5", Some("15.5")),
            ("14.3-RELEASE-p5", Some("14.3")),
            ("-broken", None),
            ("", None),
        ] {
            assert_eq!(
                release_version(release).as_deref(),
                version,
                "reading {release:?}"
            );
        }
    }
}
