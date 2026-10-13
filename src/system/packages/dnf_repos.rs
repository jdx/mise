//! `[bootstrap.dnf.repos]`: dnf repositories enabled before dnf packages.
//!
//! ```toml
//! [bootstrap.dnf.repos]
//! ghostty = { copr = "scottames/ghostty" }
//! ```
//!
//! A COPR is enabled the way `dnf copr enable` enables it: mise downloads the
//! repo file COPR publishes for this release and writes it unchanged to
//! `/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:<owner>:<project>.repo`,
//! the file the copr plugin writes. `dnf copr list`, `disable`, and `remove`
//! manage it, a COPR the plugin already enabled counts as enabled, and the
//! plugin itself is not needed, so Fedora Atomic hosts work too. dnf imports
//! the signing key the file names on the first install from the repository.

use std::path::{Path, PathBuf};

use eyre::{bail, eyre};
use serde::Deserialize;

use crate::http::HTTP_FETCH;
use crate::result::Result;
use crate::system::sudo;

const REPOS_DIR: &str = "/etc/yum.repos.d";
const COPR_HOST: &str = "copr.fedorainfracloud.org";

/// One `[bootstrap.dnf.repos.<name>]` entry as written in mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnfRepoTomlConfig {
    /// COPR project, `owner/project` or `@group/project`.
    pub copr: Option<String>,
}

/// A validated `[bootstrap.dnf.repos]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnfRepo {
    pub name: String,
    /// The COPR owner, `@`-prefixed for a group.
    pub owner: String,
    pub project: String,
}

impl DnfRepo {
    pub fn parse(name: &str, config: DnfRepoTomlConfig) -> Result<Self> {
        validate_name(name)?;
        let Some(copr) = config.copr else {
            bail!("{name}: set copr to the COPR project, as in 'scottames/ghostty'");
        };
        let (owner, project) = parse_copr(&copr).map_err(|err| eyre!("{name}: {err}"))?;
        Ok(Self {
            name: name.to_string(),
            owner,
            project,
        })
    }

    /// The repository id COPR gives the project; a group owner is spelled
    /// `group_<name>`.
    fn repo_id(&self) -> String {
        let owner = match self.owner.strip_prefix('@') {
            Some(group) => format!("group_{group}"),
            None => self.owner.clone(),
        };
        format!("copr:{COPR_HOST}:{owner}:{}", self.project)
    }

    fn repo_path(&self) -> PathBuf {
        // dnf paths are POSIX on every host that renders them, including
        // Windows running the unit tests
        PathBuf::from(format!("{REPOS_DIR}/_{}.repo", self.repo_id()))
    }

    fn describe(&self) -> String {
        format!("copr:{}/{}", self.owner, self.project)
    }

    fn repo_file_url(&self, chroot: &str, arch: &str) -> String {
        let project = match self.owner.strip_prefix('@') {
            Some(group) => format!("g/{group}/{}", self.project),
            None => format!("{}/{}", self.owner, self.project),
        };
        format!("https://{COPR_HOST}/coprs/{project}/repo/{chroot}/dnf.repo?arch={arch}")
    }

    /// Download the repo file COPR publishes for `chroot`, checked to define
    /// this project's repository.
    async fn fetch_repo_file(&self, chroot: &str) -> Result<String> {
        let arch = basearch();
        let url = self.repo_file_url(chroot, arch);
        debug!("dnf repo {}: fetching {url}", self.name);
        let resp = HTTP_FETCH
            .get_async_with_headers_allow_error_status(&url, &Default::default())
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            // COPR names the chroots it does build for when the project exists
            let available = resp
                .headers()
                .get("copr-error-data")
                .and_then(|value| value.to_str().ok())
                .and_then(available_chroots);
            match available {
                Some(chroots) if chroots.is_empty() => bail!(
                    "{} has no builds yet, so it cannot be enabled",
                    self.describe()
                ),
                Some(chroots) => bail!(
                    "{} is not built for {chroot}-{arch}; it builds for {}",
                    self.describe(),
                    chroots.join(", ")
                ),
                None => bail!("{} does not exist", self.describe()),
            }
        }
        if !status.is_success() {
            bail!("GET {url} returned {status}");
        }
        let content = resp.text().await?;
        if first_section(&content).as_deref() != Some(self.repo_id().as_str()) {
            bail!("{url} did not return a repo file for {}", self.describe());
        }
        Ok(content)
    }
}

/// The chroots in COPR's `Copr-Error-Data` header, base64-encoded JSON such as
/// `{"available chroots": ["fedora-42-x86_64"]}`.
fn available_chroots(header: &str) -> Option<Vec<String>> {
    use base64::Engine;
    #[derive(Deserialize)]
    struct ErrorData {
        #[serde(rename = "available chroots")]
        available_chroots: Option<Vec<String>>,
    }
    let json = base64::engine::general_purpose::STANDARD
        .decode(header.trim())
        .ok()?;
    let mut chroots = serde_json::from_slice::<ErrorData>(&json)
        .ok()?
        .available_chroots?;
    chroots.sort();
    Some(chroots)
}

/// The first `[section]` of a repo file.
fn first_section(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with(['#', ';']))
        .and_then(|line| line.strip_prefix('['))
        .and_then(|line| line.strip_suffix(']'))
        .map(str::to_string)
}

/// Whether `content` defines `repo_id` without turning it off. dnf reads
/// `enabled` as a boolean that defaults to on.
fn repo_enabled(content: &str, repo_id: &str) -> bool {
    let mut in_section = false;
    let mut found = false;
    let mut enabled = true;
    for line in content.lines().map(str::trim) {
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = section.trim() == repo_id;
            found |= in_section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "enabled"
        {
            enabled = !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            );
        }
    }
    found && enabled
}

/// dnf's `$basearch` for this machine, as COPR names architectures.
fn basearch() -> &'static str {
    match std::env::consts::ARCH {
        "powerpc64" if cfg!(target_endian = "little") => "ppc64le",
        "powerpc64" => "ppc64",
        "x86" => "i386",
        arch => arch,
    }
}

/// COPR project names and owners: letters, digits, `_`, `-`, `.`, and `+`.
fn parse_copr(copr: &str) -> Result<(String, String)> {
    let copr = copr.trim();
    let copr = copr.strip_prefix("copr:").unwrap_or(copr);
    let valid = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
    };
    match copr.split_once('/') {
        Some((owner, project))
            if valid(owner.strip_prefix('@').unwrap_or(owner)) && valid(project) =>
        {
            Ok((owner.to_string(), project.to_string()))
        }
        _ => bail!(
            "copr must be 'owner/project' or '@group/project', as in 'scottames/ghostty', got '{copr}'"
        ),
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        bail!(
            "'{name}' is not a valid repository name; use letters, digits, '_', '-', and '.', not starting with '.'"
        );
    }
    Ok(())
}

/// The parts of `/etc/os-release` that pick a COPR chroot.
#[derive(Debug, Default, Clone)]
struct Release {
    id: String,
    id_like: Vec<String>,
    version_id: Option<String>,
    /// `platform:f42`, `platform:el9`, `platform:al2023`: the base release,
    /// which a derivative's `ID` and `ID_LIKE` do not always reveal
    platform_id: Option<String>,
    redhat_support_product_version: Option<String>,
}

impl Release {
    fn read() -> Self {
        Self::parse(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
    }

    fn parse(content: &str) -> Self {
        let mut release = Self::default();
        for line in content.lines() {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches(['"', '\'']).to_string();
            let value = (!value.is_empty()).then_some(value);
            match key.trim() {
                "ID" => release.id = value.unwrap_or_default(),
                "ID_LIKE" => {
                    release.id_like = value
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect()
                }
                "VERSION_ID" => release.version_id = value,
                "PLATFORM_ID" => release.platform_id = value,
                "REDHAT_SUPPORT_PRODUCT_VERSION" => release.redhat_support_product_version = value,
                _ => {}
            }
        }
        release
    }

    /// The COPR chroot this release installs from, without the architecture,
    /// chosen as the copr plugin chooses it: Fedora and its derivatives by
    /// release, Amazon Linux by version, and RHEL and its rebuilds from EPEL.
    fn chroot(&self) -> Result<String> {
        if self.redhat_support_product_version.as_deref() == Some("rawhide") {
            return Ok("fedora-rawhide".to_string());
        }
        // Oracle Linux sets only ID_LIKE=fedora, so the platform decides
        // first; ID and ID_LIKE are the fallback for releases without one
        let platform = self
            .platform_id
            .as_deref()
            .and_then(|p| p.strip_prefix("platform:"));
        let numbered = |prefix: &str| {
            platform
                .and_then(|p| p.strip_prefix(prefix))
                .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()))
        };
        // `al` is Amazon Linux's platform, and also Alibaba Cloud Linux's,
        // whose `platform:al8` names the EL release it is compatible with
        if let Some(version) = numbered("al")
            && self.id == "amzn"
        {
            return Ok(format!("amazonlinux-{version}"));
        }
        if let Some(major) = numbered("el").or(numbered("al")) {
            return Ok(format!("epel-{major}"));
        }
        if let Some(version) = numbered("f") {
            return Ok(format!("fedora-{version}"));
        }
        let like = |id: &str| self.id == id || self.id_like.iter().any(|l| l == id);
        let major = || {
            self.version_id
                .as_deref()
                .and_then(|v| v.split('.').next())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| eyre!("/etc/os-release has no VERSION_ID"))
        };
        if self.id == "amzn" {
            return Ok(format!("amazonlinux-{}", major()?));
        }
        if like("rhel") || like("centos") || self.id == "ol" {
            return Ok(format!("epel-{}", major()?));
        }
        if like("fedora") {
            return Ok(format!("fedora-{}", major()?));
        }
        bail!(
            "COPR builds for Fedora, EPEL, and Amazon Linux, and this release (ID={}) is none of them",
            self.id
        )
    }
}

/// Enable every declared repository that is not enabled yet. Returns the
/// repositories that could not be enabled, such as a COPR with no build for
/// this release; one failing does not stop the others. Only a failure to ask
/// is returned as an error. With `ask`, mise asks once before enabling any,
/// and a no leaves them all as they are.
pub(crate) async fn apply(
    repos: &[DnfRepo],
    dry_run: bool,
    ask: bool,
) -> Result<Vec<eyre::Report>> {
    let pending = repos
        .iter()
        .filter(|repo| {
            let enabled = std::fs::read_to_string(repo.repo_path())
                .is_ok_and(|content| repo_enabled(&content, &repo.repo_id()));
            if enabled {
                debug!("dnf repo {}: enabled", repo.name);
            }
            !enabled
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(vec![]);
    }
    let chroot = match Release::read().chroot() {
        Ok(chroot) => chroot,
        Err(err) => {
            let names = pending
                .iter()
                .map(|repo| repo.describe())
                .collect::<Vec<_>>();
            return Ok(vec![
                err.wrap_err(format!("cannot enable {}", names.join(", "))),
            ]);
        }
    };
    if dry_run {
        for repo in &pending {
            miseprintln!(
                "fetch the {chroot} repo file for {} into {}",
                repo.describe(),
                repo.repo_path().display()
            );
        }
        return Ok(vec![]);
    }
    if ask && !confirm(&pending)? {
        info!("dnf: repositories left unchanged");
        return Ok(vec![]);
    }
    let mut failures = vec![];
    for repo in pending {
        let result: Result<()> = async {
            let content = repo.fetch_repo_file(&chroot).await?;
            write_root_file(&repo.repo_path(), content.as_bytes())
        }
        .await;
        match result {
            Ok(()) => info!("dnf: enabled {} ({})", repo.name, repo.describe()),
            Err(err) => {
                failures.push(err.wrap_err(format!("dnf repo {} ({})", repo.name, repo.describe())))
            }
        }
    }
    Ok(failures)
}

fn confirm(pending: &[&DnfRepo]) -> Result<bool> {
    let list = pending
        .iter()
        .map(|repo| format!("{} ({})", repo.name, repo.describe()))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(crate::ui::prompt::confirm(format!("dnf: enable repositories {list}?"))?.is_yes())
}

/// Write a world-readable root-owned file, as the copr plugin leaves it.
fn write_root_file(path: &Path, content: &[u8]) -> Result<()> {
    sudo::run_with_input(
        "install",
        &[
            "-m".into(),
            "0644".into(),
            "/dev/stdin".into(),
            path.display().to_string(),
        ],
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copr(name: &str, copr: &str) -> Result<DnfRepo> {
        DnfRepo::parse(
            name,
            DnfRepoTomlConfig {
                copr: Some(copr.into()),
            },
        )
    }

    const GHOSTTY: &str = "[copr:copr.fedorainfracloud.org:scottames:ghostty]\n\
        name=Copr repo for ghostty owned by scottames\n\
        baseurl=https://download.copr.fedorainfracloud.org/results/scottames/ghostty/fedora-$releasever-$basearch/\n\
        type=rpm-md\n\
        skip_if_unavailable=True\n\
        gpgcheck=1\n\
        gpgkey=https://download.copr.fedorainfracloud.org/results/scottames/ghostty/pubkey.gpg\n\
        repo_gpgcheck=0\n\
        enabled=1\n\
        enabled_metadata=1\n";

    #[test]
    fn names_files_as_the_copr_plugin_does() {
        let repo = copr("ghostty", "scottames/ghostty").unwrap();
        assert_eq!(
            repo.repo_path(),
            Path::new("/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:scottames:ghostty.repo")
        );
        assert_eq!(
            repo.repo_file_url("fedora-42", "x86_64"),
            "https://copr.fedorainfracloud.org/coprs/scottames/ghostty/repo/fedora-42/dnf.repo?arch=x86_64"
        );
        assert_eq!(repo.describe(), "copr:scottames/ghostty");

        let group = copr("llvm", "copr:@fedora-llvm-team/llvm-snapshots").unwrap();
        assert_eq!(
            group.repo_id(),
            "copr:copr.fedorainfracloud.org:group_fedora-llvm-team:llvm-snapshots"
        );
        assert_eq!(
            group.repo_file_url("fedora-rawhide", "aarch64"),
            "https://copr.fedorainfracloud.org/coprs/g/fedora-llvm-team/llvm-snapshots/repo/fedora-rawhide/dnf.repo?arch=aarch64"
        );
    }

    #[test]
    fn rejects_invalid_declarations() {
        let err = |name: &str, value: &str| copr(name, value).unwrap_err().to_string();
        assert!(err("bad/name", "a/b").contains("not a valid repository name"));
        assert!(err(".hidden", "a/b").contains("not a valid repository name"));
        for bad in ["ghostty", "a/b/c", "/b", "a/", "@/b", "a b/c", "a/.b"] {
            assert!(err("x", bad).contains("copr must be"), "{bad}");
        }
        assert!(
            DnfRepo::parse("x", DnfRepoTomlConfig::default())
                .unwrap_err()
                .to_string()
                .contains("set copr")
        );
    }

    #[test]
    fn picks_the_chroot_from_os_release() {
        let chroot = |content: &str| Release::parse(content).chroot();
        assert_eq!(chroot("ID=fedora\nVERSION_ID=42\n").unwrap(), "fedora-42");
        assert_eq!(
            chroot("ID=fedora\nVERSION_ID=44\nREDHAT_SUPPORT_PRODUCT_VERSION=rawhide\n").unwrap(),
            "fedora-rawhide"
        );
        // Fedora Atomic derivatives such as Bazzite
        assert_eq!(
            chroot("ID=bazzite\nID_LIKE=\"fedora\"\nVERSION_ID=\"42\"\n").unwrap(),
            "fedora-42"
        );
        assert_eq!(
            chroot("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n").unwrap(),
            "epel-9"
        );
        assert_eq!(
            chroot("ID=\"centos\"\nID_LIKE=\"rhel fedora\"\nVERSION_ID=\"10\"\n").unwrap(),
            "epel-10"
        );
        assert_eq!(
            chroot("ID=\"amzn\"\nID_LIKE=\"fedora\"\nVERSION_ID=\"2023\"\n").unwrap(),
            "amazonlinux-2023"
        );
        // Oracle Linux claims only Fedora ancestry; its platform says EL
        assert_eq!(
            chroot(
                "ID=\"ol\"\nID_LIKE=\"fedora\"\nVERSION_ID=\"9.8\"\nPLATFORM_ID=\"platform:el9\"\n"
            )
            .unwrap(),
            "epel-9"
        );
        assert_eq!(
            chroot("ID=\"ol\"\nID_LIKE=\"fedora\"\nVERSION_ID=\"8.10\"\n").unwrap(),
            "epel-8"
        );
        assert_eq!(
            chroot("ID=bazzite\nID_LIKE=\"fedora\"\nVERSION_ID=\"42.20250101\"\nPLATFORM_ID=\"platform:f42\"\n")
                .unwrap(),
            "fedora-42"
        );
        assert_eq!(
            chroot("ID=\"amzn\"\nID_LIKE=\"fedora\"\nVERSION_ID=\"2023\"\nPLATFORM_ID=\"platform:al2023\"\n")
                .unwrap(),
            "amazonlinux-2023"
        );
        assert_eq!(
            chroot("ID=fedora\nVERSION_ID=44\nPLATFORM_ID=\"platform:f44\"\nREDHAT_SUPPORT_PRODUCT_VERSION=rawhide\n")
                .unwrap(),
            "fedora-rawhide"
        );
        assert_eq!(
            chroot("ID=\"alinux\"\nID_LIKE=\"rhel fedora centos anolis\"\nVERSION_ID=\"3\"\nPLATFORM_ID=\"platform:al8\"\n")
                .unwrap(),
            "epel-8"
        );
        assert!(chroot("ID=ubuntu\nVERSION_ID=24.04\n").is_err());
        assert!(chroot("ID=fedora\n").is_err());
    }

    #[test]
    fn reads_enabled_state_from_the_repo_file() {
        let id = "copr:copr.fedorainfracloud.org:scottames:ghostty";
        assert!(repo_enabled(GHOSTTY, id));
        assert!(!repo_enabled(
            &GHOSTTY.replace("enabled=1", "enabled=0"),
            id
        ));
        assert!(!repo_enabled(
            &GHOSTTY.replace("enabled=1", "enabled = False"),
            id
        ));
        // dnf turns a repository on unless told otherwise
        assert!(repo_enabled(&GHOSTTY.replace("enabled=1\n", ""), id));
        assert!(!repo_enabled(GHOSTTY, "copr:copr.fedorainfracloud.org:x:y"));
        assert!(!repo_enabled("", id));
        // a disabled multilib section does not turn off the main one
        let multilib = format!("{GHOSTTY}\n[{id}:ml]\nenabled=0\n");
        assert!(repo_enabled(&multilib, id));
    }

    #[test]
    fn checks_the_downloaded_file_names_the_project() {
        assert_eq!(
            first_section(&format!("\n# comment\n{GHOSTTY}")).as_deref(),
            Some("copr:copr.fedorainfracloud.org:scottames:ghostty")
        );
        assert_eq!(first_section("<html>"), None);
    }

    #[test]
    fn decodes_available_chroots() {
        use base64::Engine;
        let header = base64::engine::general_purpose::STANDARD
            .encode(r#"{"available chroots": ["fedora-43-x86_64", "fedora-42-x86_64"]}"#);
        assert_eq!(
            available_chroots(&header).unwrap(),
            vec!["fedora-42-x86_64", "fedora-43-x86_64"]
        );
        assert_eq!(available_chroots("not base64!"), None);
    }
}
