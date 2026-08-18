use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::result::Result;
use crate::system::sudo;

pub struct AptManager {}

impl AptManager {
    pub fn new() -> Self {
        Self {}
    }

    /// fresh container/VM case: no package lists at all, so any install would
    /// fail with "Unable to locate package"
    fn lists_missing(&self) -> bool {
        let lists = Path::new("/var/lib/apt/lists");
        !crate::file::ls(lists).unwrap_or_default().iter().any(|p| {
            p.file_name()
                .map(|f| f.to_string_lossy().contains("_Packages"))
                .unwrap_or(false)
        })
    }

    fn update(&self, opts: &InstallOpts) -> Result<()> {
        let args = vec!["update".to_string()];
        if opts.dry_run {
            miseprintln!(
                "{}",
                sudo::argv_with_env("apt-get", &args, &debian_frontend()).join(" ")
            );
            return Ok(());
        }
        sudo::run("apt-get", &args, &debian_frontend())
    }
}

fn debian_frontend() -> Vec<(String, String)> {
    vec![("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())]
}

/// `name` may carry an architecture qualifier (`gcc:arm64`); dpkg-query
/// reports the bare package name
fn dpkg_name(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

/// Names from `apt-cache policy` output that have an installable candidate.
///
/// Output is one stanza per known package: an unindented `<name>:` header
/// followed by indented `Installed:`/`Candidate:` lines. Unknown packages get
/// no stanza at all, and a package apt knows only as a dependency of something
/// else has `Candidate: (none)` — neither is installable, so neither counts.
fn parse_apt_cache_policy(output: &str) -> std::collections::HashSet<String> {
    let mut installable = std::collections::HashSet::new();
    let mut current: Option<&str> = None;
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            current = line.strip_suffix(':');
        } else if let Some(candidate) = line.trim_start().strip_prefix("Candidate:")
            && let Some(name) = current.take()
            && candidate.trim() != "(none)"
        {
            installable.insert(name.to_string());
        }
    }
    installable
}

fn parse_dpkg_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    // keyed by both "name" and "name:arch" so arch-qualified requests
    // (gcc:arm64) only match that architecture. A package can be installed
    // for several architectures (multi-arch) at different versions, so keep
    // every installed version — a version pin must match if *any* arch
    // satisfies it, regardless of dpkg-query's output order
    let mut installed: HashMap<String, Vec<&str>> = HashMap::new();
    for line in output.lines() {
        let mut parts = line.split('\t');
        if let (Some(name), Some(status), Some(version)) =
            (parts.next(), parts.next(), parts.next())
        {
            if status != "installed" {
                continue;
            }
            if let Some(arch) = parts.next() {
                installed
                    .entry(format!("{name}:{arch}"))
                    .or_default()
                    .push(version);
            }
            installed.entry(name.to_string()).or_default().push(version);
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(&req.name) {
                Some(versions) => match &req.version {
                    Some(requested) if !versions.contains(&requested.as_str()) => {
                        PackageState::VersionMismatch {
                            installed: versions[0].to_string(),
                        }
                    }
                    Some(requested) => PackageState::Installed {
                        version: requested.clone(),
                    },
                    None => PackageState::Installed {
                        version: versions[0].to_string(),
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
impl SystemPackageManager for AptManager {
    fn name(&self) -> &str {
        "apt"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("apt-get").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "apt-get not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        if pkgs.is_empty() {
            return Ok(vec![]);
        }
        let mut args = vec![
            "-W".to_string(),
            "-f=${Package}\\t${db:Status-Status}\\t${Version}\\t${Architecture}\\n".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| dpkg_name(&p.name).to_string()));
        debug!("$ dpkg-query {}", args.join(" "));
        let output = tokio::process::Command::new("dpkg-query")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // exit 1 just means some packages are unknown to dpkg — they're Missing
        if !output.status.success() && output.status.code() != Some(1) {
            bail!(
                "dpkg-query failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_dpkg_query(&stdout, pkgs))
    }

    async fn available(&self, names: &[String]) -> Result<Vec<bool>> {
        if names.is_empty() {
            return Ok(vec![]);
        }
        let args = names.iter().map(|n| n.as_str()).collect::<Vec<_>>();
        debug!("$ apt-cache policy {}", args.join(" "));
        let output = tokio::process::Command::new("apt-cache")
            .arg("policy")
            .args(&args)
            // apt translates the stanza labels this parses, so pin the locale
            // rather than reading "Kandidat:" as an unavailable package
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        // unknown names are reported on stderr and do not fail the command,
        // so a nonzero status means apt-cache itself could not run
        if !output.status.success() {
            bail!(
                "apt-cache policy failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let installable = parse_apt_cache_policy(&String::from_utf8_lossy(&output.stdout));
        Ok(names
            .iter()
            .map(|n| installable.contains(n) || installable.contains(dpkg_name(n)))
            .collect())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.update || self.lists_missing() {
            self.update(opts)?;
        }
        // `--` keeps package operands from ever being parsed as apt-get
        // options; pins render to apt's native name=version syntax and
        // name:arch qualifiers pass through in the name
        let mut args = vec!["install".to_string(), "-y".to_string(), "--".to_string()];
        args.extend(pkgs.iter().map(|p| match &p.version {
            Some(v) => format!("{}={v}", p.name),
            None => p.name.clone(),
        }));
        if opts.dry_run {
            miseprintln!(
                "{}",
                sudo::argv_with_env("apt-get", &args, &debian_frontend()).join(" ")
            );
            return Ok(());
        }
        sudo::run("apt-get", &args, &debian_frontend())
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // upgrading against stale lists is a no-op, so always refresh first
        self.update(opts)?;
        // `--only-upgrade` keeps a race (package removed between our status
        // check and this call) from turning an upgrade into a fresh install
        let mut args = vec![
            "install".to_string(),
            "-y".to_string(),
            "--only-upgrade".to_string(),
            "--".to_string(),
        ];
        args.extend(pkgs.iter().map(|p| match &p.version {
            Some(v) => format!("{}={v}", p.name),
            None => p.name.clone(),
        }));
        if opts.dry_run {
            miseprintln!(
                "{}",
                sudo::argv_with_env("apt-get", &args, &debian_frontend()).join(" ")
            );
            return Ok(());
        }
        sudo::run("apt-get", &args, &debian_frontend())
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
    fn test_dpkg_name() {
        assert_eq!(dpkg_name("gcc"), "gcc");
        assert_eq!(dpkg_name("gcc:arm64"), "gcc");
    }

    /// Exercises the real `apt-cache` where there is one (Linux CI), so the
    /// parser cannot drift from apt's actual output format.
    #[tokio::test]
    async fn test_available_against_real_apt_cache() {
        let mgr = AptManager::new();
        if !mgr.is_available() || crate::file::which("apt-cache").is_none() {
            return;
        }
        let names = vec!["bash".to_string(), "mise-nonexistent-pkg-xyz".to_string()];
        let available = mgr.available(&names).await.unwrap();
        assert_eq!(available, vec![true, false]);
    }

    #[test]
    fn test_parse_apt_cache_policy() {
        // libaio1 is the pre-time_t name: apt still knows it as a virtual
        // dependency but has nothing to install, so it is not available.
        // "nonexistent" gets no stanza at all.
        let output = indoc::indoc! {"
            libaio1t64:
              Installed: (none)
              Candidate: 0.3.113-6build1
              Version table:
                 0.3.113-6build1 500
            libaio1:
              Installed: (none)
              Candidate: (none)
              Version table:
            libncurses6:
              Installed: 6.4+20240113-1
              Candidate: 6.4+20240113-1
        "};
        let installable = parse_apt_cache_policy(output);
        assert!(installable.contains("libaio1t64"));
        assert!(installable.contains("libncurses6"));
        assert!(!installable.contains("libaio1"));
        assert!(!installable.contains("nonexistent"));
    }

    #[test]
    fn test_parse_dpkg_query() {
        let requests = vec![
            req("bc", None),
            req("nonexistent", None),
            req("removed-pkg", None),
            req("curl", Some("9.9.9")),
        ];
        let output = "bc\tinstalled\t1.07.1-3\tamd64\nremoved-pkg\tdeinstall\t2.0\tamd64\ncurl\tinstalled\t8.5.0-2\tamd64\n";
        let statuses = parse_dpkg_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.07.1-3".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        assert_eq!(statuses[2].state, PackageState::Missing);
        assert_eq!(
            statuses[3].state,
            PackageState::VersionMismatch {
                installed: "8.5.0-2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_dpkg_query_multiarch_bare_name() {
        let requests = vec![req("libssl3", None)];
        // installed for amd64, deinstalled for i386 — the bare request is
        // satisfied regardless of which line dpkg-query prints last
        for output in [
            "libssl3\tinstalled\t3.0.2\tamd64\nlibssl3\tdeinstall\t3.0.1\ti386\n",
            "libssl3\tdeinstall\t3.0.1\ti386\nlibssl3\tinstalled\t3.0.2\tamd64\n",
        ] {
            let statuses = parse_dpkg_query(output, &requests);
            assert_eq!(
                statuses[0].state,
                PackageState::Installed {
                    version: "3.0.2".to_string()
                }
            );
        }
    }

    #[test]
    fn test_parse_dpkg_query_multiarch_bare_name_versioned() {
        let requests = vec![req("libssl3", Some("3.0.2"))];
        // two arches installed at different versions — the pin matches if
        // any arch satisfies it, regardless of dpkg-query's output order
        for output in [
            "libssl3\tinstalled\t3.0.1\ti386\nlibssl3\tinstalled\t3.0.2\tamd64\n",
            "libssl3\tinstalled\t3.0.2\tamd64\nlibssl3\tinstalled\t3.0.1\ti386\n",
        ] {
            let statuses = parse_dpkg_query(output, &requests);
            assert_eq!(
                statuses[0].state,
                PackageState::Installed {
                    version: "3.0.2".to_string()
                }
            );
        }
    }

    #[test]
    fn test_parse_dpkg_query_arch_qualified() {
        let requests = vec![req("gcc:arm64", None), req("gcc:amd64", None)];
        let output = "gcc\tinstalled\t12.3\tarm64\n";
        let statuses = parse_dpkg_query(output, &requests);
        // gcc:arm64 matches the installed arm64 package
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "12.3".to_string()
            }
        );
        // gcc:amd64 does not match the arm64 install
        assert_eq!(statuses[1].state, PackageState::Missing);
    }
}
