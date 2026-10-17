use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use eyre::bail;

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::options::ManagerPackageOptions;
use eyre::Result;
use mise_ui::sudo;

#[derive(Default)]
pub struct AptManager {
    /// `[bootstrap.apt.sources]` changed and the lists were refreshed for
    /// them (or, in a dry run, would be), so this run needs no other refresh.
    lists_refreshed: AtomicBool,
    /// URLs of the declared sources, set only by a run that may change the
    /// machine: packages seen installed from them are recorded in
    /// [`MovedToSources`]. Status queries and dry runs leave it empty and
    /// record nothing.
    recording_uris: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl AptManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// fresh container/VM case: no package lists at all, so any install would
    /// fail with "Unable to locate package"
    fn lists_missing(&self) -> bool {
        let lists = Path::new("/var/lib/apt/lists");
        !mise_util::file::ls(lists)
            .unwrap_or_default()
            .iter()
            .any(|p| {
                p.file_name()
                    .map(|f| f.to_string_lossy().contains("_Packages"))
                    .unwrap_or(false)
            })
    }

    /// Whether `apt-get install` with these arguments would fail against the
    /// current lists.
    ///
    /// Lists can exist yet not cover the configured sources — an offline ISO
    /// install indexes only the install media — and one unknown name or
    /// missing pinned version fails the whole install. A simulation resolves
    /// exactly as the real install does, virtual packages and pins included,
    /// and needs no root. A simulation that cannot run is not a reason to
    /// refresh.
    async fn install_would_fail(&self, args: &[String]) -> bool {
        debug!("$ apt-get --simulate {}", args.join(" "));
        let output = tokio::process::Command::new("apt-get")
            .arg("--simulate")
            .arg("-q")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await;
        match output {
            Ok(output) if !output.status.success() => {
                debug!(
                    "apt-get --simulate failed, refreshing lists: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                true
            }
            Ok(_) => false,
            Err(err) => {
                debug!("could not simulate apt-get install: {err:#}");
                false
            }
        }
    }

    /// Names in `args` that `apt-cache policy` reports with an install
    /// candidate. Keyed by the bare name apt heads each stanza with, so callers
    /// holding an arch-qualified name must query it alone.
    async fn policy_installable(&self, args: &[&str]) -> Result<std::collections::HashSet<String>> {
        debug!("$ apt-cache policy {}", args.join(" "));
        let output = tokio::process::Command::new("apt-cache")
            .arg("policy")
            .args(args)
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
        Ok(parse_apt_cache_policy(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    /// `apt-cache policy` for `names`, with the repositories offering each
    /// version.
    async fn policy_origins(&self, names: &[&str]) -> Result<HashMap<String, PolicyOrigins>> {
        debug!("$ apt-cache policy {}", names.join(" "));
        let output = tokio::process::Command::new("apt-cache")
            .arg("policy")
            .args(names)
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !output.status.success() {
            bail!(
                "apt-cache policy failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(parse_policy_origins(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    /// After an install or upgrade, remember which of `pkgs` now come from a
    /// declared source, whether mise moved them there or installed them fresh.
    async fn record_installed_from_sources(&self, pkgs: &[PackageRequest]) {
        let uris = self.recording_uris.lock().unwrap().clone();
        let names = pkgs
            .iter()
            .filter(|pkg| !pkg.name.contains(':'))
            .map(|pkg| pkg.name.as_str())
            .collect::<Vec<_>>();
        if uris.is_empty() || names.is_empty() {
            return;
        }
        match self.policy_origins(&names).await {
            Ok(policies) => MovedToSources::record(
                policies
                    .iter()
                    .filter_map(|(name, policy)| Some((name, policy.installed_origin(&uris)?))),
            ),
            Err(err) => debug!("apt-cache policy failed, not recording sources: {err:#}"),
        }
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

/// Packages mise moved onto a declared `[bootstrap.apt.sources]` source, as
/// `(package, source URL)`.
///
/// apt cannot say where an installed version came from once its repository
/// stops listing it: after a PPA publishes a newer build, the one installed
/// from it is known only to dpkg, exactly like a distro build an old image
/// shipped. This record tells the two apart, so the move happens once per
/// source. It lives in the state directory of the user running mise, so
/// `sudo mise` keeps its own: the worst case is one more upgrade from the
/// same source the first time the other user applies.
#[derive(Debug, Default)]
struct MovedToSources(std::collections::BTreeSet<(String, String)>);

impl MovedToSources {
    fn path() -> std::path::PathBuf {
        mise_util::dirs::STATE.join("apt-sources-moved")
    }

    fn load() -> Self {
        let content = std::fs::read_to_string(Self::path()).unwrap_or_default();
        Self(
            content
                .lines()
                .filter_map(|line| line.split_once(' '))
                .map(|(name, uri)| (name.to_string(), uri.to_string()))
                .collect(),
        )
    }

    fn save(&self) -> Result<()> {
        let content = self
            .0
            .iter()
            .map(|(name, uri)| format!("{name} {uri}\n"))
            .collect::<String>();
        mise_util::file::write(Self::path(), content)?;
        Ok(())
    }

    /// Add `entries`, writing only when one is new. A failure to record only
    /// means a later run checks those packages again.
    fn record<'a>(entries: impl Iterator<Item = (&'a String, &'a str)>) {
        let mut moved = Self::load();
        let before = moved.0.len();
        moved
            .0
            .extend(entries.map(|(name, uri)| (name.clone(), uri.to_string())));
        if moved.0.len() == before {
            return;
        }
        if let Err(err) = moved.save() {
            debug!("could not record packages installed from apt sources: {err:#}");
        }
    }

    fn contains(&self, name: &str, uri: &str) -> bool {
        self.0.contains(&(name.to_string(), uri.to_string()))
    }
}

/// One package's `apt-cache policy` stanza: its installed and candidate
/// versions, and the repository URLs that offer each version.
#[derive(Debug, Default, PartialEq)]
struct PolicyOrigins {
    installed: Option<String>,
    candidate: Option<String>,
    origins: HashMap<String, Vec<String>>,
}

impl PolicyOrigins {
    /// The declared source offering the installed version.
    fn installed_origin(&self, uris: &std::collections::HashSet<String>) -> Option<&str> {
        self.declared_origin(self.installed.as_deref()?, uris)
    }

    fn declared_origin(
        &self,
        version: &str,
        uris: &std::collections::HashSet<String>,
    ) -> Option<&str> {
        self.origins
            .get(version)
            .into_iter()
            .flatten()
            .find(|origin| uris.contains(*origin))
            .map(String::as_str)
    }

    /// The declared source offering the candidate, when the candidate is a
    /// different version and no declared source offers the installed one.
    fn declared_candidate_origin(&self, uris: &std::collections::HashSet<String>) -> Option<&str> {
        let (Some(installed), Some(candidate)) = (&self.installed, &self.candidate) else {
            return None;
        };
        if installed == candidate || self.declared_origin(installed, uris).is_some() {
            return None;
        }
        self.declared_origin(candidate, uris)
    }
}

/// Parse `LC_ALL=C apt-cache policy` output:
///
/// ```text
/// git:
///   Installed: 1:2.43.0-1ubuntu7.3
///   Candidate: 1:2.55.0-0ppa1~ubuntu24.04.2
///   Version table:
///      1:2.55.0-0ppa1~ubuntu24.04.2 500
///         500 https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble/main amd64 Packages
///  *** 1:2.43.0-1ubuntu7.3 500
///         500 http://archive.ubuntu.com/ubuntu noble-updates/main amd64 Packages
///         100 /var/lib/dpkg/status
/// ```
fn parse_policy_origins(output: &str) -> HashMap<String, PolicyOrigins> {
    let mut policies: HashMap<String, PolicyOrigins> = HashMap::new();
    let mut package: Option<String> = None;
    let mut version: Option<String> = None;
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            package = line.strip_suffix(':').map(str::to_string);
            version = None;
            continue;
        }
        let Some(policy) = package
            .as_ref()
            .map(|p| policies.entry(p.clone()).or_default())
        else {
            continue;
        };
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        let none = |value: &str| (value != "(none)").then(|| value.to_string());
        if let Some(installed) = trimmed.strip_prefix("Installed: ") {
            policy.installed = none(installed);
        } else if let Some(candidate) = trimmed.strip_prefix("Candidate: ") {
            policy.candidate = none(candidate);
        } else if trimmed.ends_with(':') {
            // "Version table:"
        } else if trimmed.starts_with("***") || indent <= 5 {
            version = trimmed
                .trim_start_matches("***")
                .split_whitespace()
                .next()
                .map(str::to_string);
        } else if let Some(version) = &version {
            let mut fields = trimmed.split_whitespace().skip(1);
            if let Some(uri) = fields.next() {
                policy.origins.entry(version.clone()).or_default().push(
                    super::apt_sources::origin_key(uri, fields.next().unwrap_or_default()),
                );
            }
        }
    }
    policies
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
                display_name: None,
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
        cfg!(target_os = "linux") && mise_util::file::which("apt-get").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "apt-get not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn apply_sources(
        &self,
        manager_options: &ManagerPackageOptions,
        opts: &InstallOpts,
    ) -> Result<()> {
        match manager_options {
            ManagerPackageOptions::Apt { sources } => {
                let mut applied =
                    super::apt_sources::apply(sources, opts.dry_run, opts.ask).await?;
                if applied.refreshed {
                    self.lists_refreshed.store(true, Ordering::Relaxed);
                }
                if !opts.dry_run {
                    *self.recording_uris.lock().unwrap() =
                        super::apt_sources::declared_origins(sources);
                }
                match applied.failures.len() {
                    0 => Ok(()),
                    1 => Err(applied.failures.remove(0)),
                    count => {
                        let details = applied
                            .failures
                            .iter()
                            .map(|err| format!("  {err:#}"))
                            .collect::<Vec<_>>();
                        bail!("{count} apt sources failed:\n{}", details.join("\n"))
                    }
                }
            }
            _ => Ok(()),
        }
    }

    /// A declared source should supply the packages it carries, including
    /// ones the machine already had: an image that ships Ubuntu's git must
    /// end up with the PPA's once the PPA is declared. A package counts as out
    /// of date while apt's candidate comes from a declared source, the
    /// installed version does not, and mise has not already moved it to that
    /// source. After the move a newer upload is left to `upgrade`, as for any
    /// `latest` package.
    async fn installed_with_options(
        &self,
        pkgs: &[PackageRequest],
        manager_options: &ManagerPackageOptions,
    ) -> Result<Vec<PackageStatus>> {
        let mut statuses = self.installed(pkgs).await?;
        let ManagerPackageOptions::Apt { sources } = manager_options else {
            return Ok(statuses);
        };
        let uris = super::apt_sources::declared_origins(sources);
        // apt heads policy stanzas with the bare name, so arch-qualified
        // requests cannot be matched and are left as dpkg reports them
        let names = statuses
            .iter()
            .filter(|s| s.request.version.is_none() && !s.request.name.contains(':'))
            .filter(|s| matches!(s.state, PackageState::Installed { .. }))
            .map(|s| s.request.name.as_str())
            .collect::<Vec<_>>();
        if uris.is_empty() || names.is_empty() {
            return Ok(statuses);
        }
        let moved = MovedToSources::load();
        let policies = match self.policy_origins(&names).await {
            Ok(policies) => policies,
            Err(err) => {
                debug!("apt-cache policy failed, not checking declared sources: {err:#}");
                return Ok(statuses);
            }
        };
        // a package already on a declared source is recorded as well, so a
        // later upload there is not mistaken for a move once the installed
        // build drops out of the source's lists
        if !self.recording_uris.lock().unwrap().is_empty() {
            MovedToSources::record(
                policies
                    .iter()
                    .filter_map(|(name, policy)| Some((name, policy.installed_origin(&uris)?))),
            );
        }
        for status in &mut statuses {
            let PackageState::Installed { version } = &status.state else {
                continue;
            };
            if status.request.version.is_some() {
                continue;
            }
            let name = &status.request.name;
            let Some(uri) = policies
                .get(name)
                .and_then(|policy| policy.declared_candidate_origin(&uris))
            else {
                continue;
            };
            if moved.contains(name, uri) {
                continue;
            }
            status.state = PackageState::VersionMismatch {
                installed: version.clone(),
            };
        }
        Ok(statuses)
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
        // apt-cache policy heads every stanza with the *bare* package name, so
        // `bash:amd64` reports as `bash:` — an arch qualifier cannot be
        // recovered from a batched result, and matching one by its bare name
        // would call `bash:arm64` installable whenever any `bash` stanza is
        // present. Bare names go in one batch; each qualified name gets its own
        // query, where apt answers by emitting a stanza or nothing at all.
        let (qualified, bare): (Vec<&String>, Vec<&String>) =
            names.iter().partition(|n| n.contains(':'));
        let mut installable = if bare.is_empty() {
            Default::default()
        } else {
            self.policy_installable(&bare.iter().map(|n| n.as_str()).collect::<Vec<_>>())
                .await?
        };
        for name in qualified {
            if !self.policy_installable(&[name.as_str()]).await?.is_empty() {
                installable.insert(name.clone());
            }
        }
        Ok(names.iter().map(|n| installable.contains(n)).collect())
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // `--` keeps package operands from ever being parsed as apt-get
        // options; pins render to apt's native name=version syntax and
        // name:arch qualifiers pass through in the name
        let mut args = vec!["install".to_string(), "-y".to_string(), "--".to_string()];
        args.extend(pkgs.iter().map(|p| match &p.version {
            Some(v) => format!("{}={v}", p.name),
            None => p.name.clone(),
        }));
        if !self.lists_refreshed.load(Ordering::Relaxed)
            && (opts.update || self.lists_missing() || self.install_would_fail(&args).await)
        {
            self.update(opts)?;
        }
        if opts.dry_run {
            miseprintln!(
                "{}",
                sudo::argv_with_env("apt-get", &args, &debian_frontend()).join(" ")
            );
            return Ok(());
        }
        sudo::run("apt-get", &args, &debian_frontend())?;
        self.record_installed_from_sources(pkgs).await;
        Ok(())
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // upgrading against stale lists is a no-op, so always refresh first
        if !self.lists_refreshed.load(Ordering::Relaxed) {
            self.update(opts)?;
        }
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
        sudo::run("apt-get", &args, &debian_frontend())?;
        self.record_installed_from_sources(pkgs).await;
        Ok(())
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

    const GIT_POLICY: &str = "\
git:
  Installed: 1:2.43.0-1ubuntu7.3
  Candidate: 1:2.55.0-0ppa1~ubuntu24.04.2
  Version table:
     1:2.55.0-0ppa1~ubuntu24.04.2 500
        500 https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble/main amd64 Packages
 *** 1:2.43.0-1ubuntu7.3 500
        500 http://archive.ubuntu.com/ubuntu noble-updates/main amd64 Packages
        100 /var/lib/dpkg/status
     1:2.43.0-1build1 500
        500 http://archive.ubuntu.com/ubuntu noble/main amd64 Packages
curl:
  Installed: 8.5.0-2ubuntu10.6
  Candidate: 8.5.0-2ubuntu10.6
  Version table:
 *** 8.5.0-2ubuntu10.6 500
        500 http://archive.ubuntu.com/ubuntu noble-updates/main amd64 Packages
        100 /var/lib/dpkg/status
";

    fn ppa_uris() -> std::collections::HashSet<String> {
        ["https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble".to_string()].into()
    }

    #[test]
    fn policy_origins_map_versions_to_repositories() {
        let policies = parse_policy_origins(GIT_POLICY);
        let git = &policies["git"];
        assert_eq!(git.installed.as_deref(), Some("1:2.43.0-1ubuntu7.3"));
        assert_eq!(
            git.candidate.as_deref(),
            Some("1:2.55.0-0ppa1~ubuntu24.04.2")
        );
        assert_eq!(
            git.origins["1:2.55.0-0ppa1~ubuntu24.04.2"],
            vec!["https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble"]
        );
        assert_eq!(
            git.origins["1:2.43.0-1ubuntu7.3"],
            vec![
                "http://archive.ubuntu.com/ubuntu noble-updates",
                "/var/lib/dpkg/status"
            ]
        );
        assert_eq!(policies["curl"].origins.len(), 1);
    }

    #[test]
    fn candidate_from_declared_source_replaces_a_distro_install() {
        let policies = parse_policy_origins(GIT_POLICY);
        assert_eq!(
            policies["git"].declared_candidate_origin(&ppa_uris()),
            Some("https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble")
        );
        // a routine distro update is not a declared source's business
        assert_eq!(
            policies["curl"].declared_candidate_origin(&ppa_uris()),
            None
        );
        // nothing declared offers git
        assert_eq!(
            policies["git"].declared_candidate_origin(
                &["https://download.docker.com/linux/ubuntu noble".to_string()].into()
            ),
            None
        );
    }

    #[test]
    fn installed_from_declared_source_stays_current() {
        let policies = parse_policy_origins(
            "\
git:
  Installed: 1:2.55.0-0ppa1~ubuntu24.04.2
  Candidate: 1:2.55.0-0ppa1~ubuntu24.04.2
  Version table:
 *** 1:2.55.0-0ppa1~ubuntu24.04.2 500
        500 https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble/main amd64 Packages
        100 /var/lib/dpkg/status
",
        );
        assert_eq!(policies["git"].declared_candidate_origin(&ppa_uris()), None);
        assert_eq!(
            policies["git"].installed_origin(&ppa_uris()),
            Some("https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble")
        );
        // a distro build is not recorded as coming from the PPA
        assert_eq!(
            parse_policy_origins(GIT_POLICY)["git"].installed_origin(&ppa_uris()),
            None
        );
    }

    #[test]
    fn same_mirror_other_suite_is_not_the_declared_source() {
        // stable and a declared testing suite share deb.debian.org
        let policies = parse_policy_origins(
            "\
curl:
  Installed: 7.88.1-10+deb12u8
  Candidate: 8.14.1-2
  Version table:
     8.14.1-2 500
        500 https://deb.debian.org/debian trixie/main amd64 Packages
 *** 7.88.1-10+deb12u8 500
        500 https://deb.debian.org/debian bookworm/main amd64 Packages
        100 /var/lib/dpkg/status
",
        );
        let testing = ["https://deb.debian.org/debian trixie".to_string()].into();
        assert_eq!(policies["curl"].installed_origin(&testing), None);
        assert_eq!(
            policies["curl"].declared_candidate_origin(&testing),
            Some("https://deb.debian.org/debian trixie")
        );
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
        if !mgr.is_available() || mise_util::file::which("apt-cache").is_none() {
            return;
        }
        let names = vec!["bash".to_string(), "mise-nonexistent-pkg-xyz".to_string()];
        let available = mgr.available(&names).await.unwrap();
        assert_eq!(available, vec![true, false]);

        // An arch qualifier apt cannot satisfy must stay unavailable even when
        // a bare candidate for the same package is installable in the same
        // batch: apt heads both stanzas with the bare name, so a batched result
        // cannot tell them apart.
        let names = vec!["bash:mise-not-an-arch".to_string(), "bash".to_string()];
        let available = mgr.available(&names).await.unwrap();
        assert_eq!(available, vec![false, true]);
    }

    /// The refresh check simulates the real install, so it agrees with apt on
    /// virtual packages and pins that `apt-cache policy` cannot judge.
    #[tokio::test]
    async fn test_install_would_fail_against_real_apt_get() {
        let mgr = AptManager::new();
        if !mgr.is_available() {
            return;
        }
        let install = |names: &[&str]| {
            let mut args = vec!["install".to_string(), "-y".to_string(), "--".to_string()];
            args.extend(names.iter().map(|n| n.to_string()));
            args
        };
        assert!(!mgr.install_would_fail(&install(&["bash"])).await);
        assert!(
            mgr.install_would_fail(&install(&["bash", "mise-nonexistent-pkg-xyz"]))
                .await
        );
        assert!(mgr.install_would_fail(&install(&["bash=0.0.0-mise"])).await);
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
