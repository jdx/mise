//! Per-manager options that travel with a manager's package requests, and the
//! inline `macos-app` declaration type.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::collections::BTreeSet;
#[cfg(unix)]
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub enum ManagerPackageOptions {
    #[default]
    None,
    /// `[bootstrap.apt.sources]`, applied before apt queries its packages.
    Apt {
        sources: Vec<super::apt_sources::AptSource>,
    },
    /// `[bootstrap.dnf.repos]`, enabled before dnf installs its packages.
    Dnf {
        repos: Vec<super::dnf_repos::DnfRepo>,
    },
    #[cfg(unix)]
    BrewCask {
        adopt: BTreeSet<String>,
        appdirs: BTreeMap<String, PathBuf>,
    },
    /// Inline app declarations, keyed by package name. `macos-app` resolves no
    /// metadata of its own, so the declaration reaches the installer here.
    #[cfg(unix)]
    MacosApp {
        specs: BTreeMap<String, AppSpec>,
        adopt: BTreeSet<String>,
    },
    /// `channel` and `classic`, keyed by snap name. Snaps without either are
    /// absent.
    Snap {
        packages: BTreeMap<String, super::snap::SnapOptions>,
    },
}

impl ManagerPackageOptions {
    #[cfg(unix)]
    pub fn brew_cask_adopt(&self, name: &str) -> bool {
        match self {
            Self::BrewCask { adopt, .. } | Self::MacosApp { adopt, .. } => adopt.contains(name),
            Self::None | Self::Apt { .. } | Self::Dnf { .. } | Self::Snap { .. } => false,
        }
    }

    #[cfg(unix)]
    pub fn brew_cask_appdir(&self, name: &str) -> Option<&Path> {
        match self {
            Self::BrewCask { appdirs, .. } => appdirs.get(name).map(PathBuf::as_path),
            _ => None,
        }
    }

    #[cfg(unix)]
    pub fn macos_app_spec(&self, name: &str) -> Option<&AppSpec> {
        match self {
            Self::MacosApp { specs, .. } => specs.get(name),
            _ => None,
        }
    }
}

/// An app bundle declared inline in `[bootstrap.packages]`, standing in for the
/// cask definition that `brew-cask` would have fetched.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSpec {
    pub url: String,
    pub sha256: String,
    pub artifact: String,
    pub version: String,
}

/// Validate an inline app declaration, returning its checksum and artifact.
///
/// Every field is required: without a checksum there is nothing to verify the
/// download against, and without an artifact name mise cannot tell which bundle
/// in the archive to install.
///
/// Platform-independent on purpose. A config shared across machines should
/// report a malformed declaration wherever it is read — otherwise a bad
/// checksum surfaces on Windows as a generic unknown-manager warning, which
/// diagnoses the wrong thing.
pub fn validate_app_declaration<'a>(
    name: &str,
    url: &str,
    sha256: Option<&'a str>,
    artifact: Option<&'a str>,
    version: &str,
) -> eyre::Result<(&'a str, &'a str)> {
    // A plain http download is still integrity-checked, because `sha256` is
    // mandatory and verified, so this warns rather than refuses — an internal
    // mirror over http is a real case for this manager. It does not cover a
    // redirect from https to http: mise's download client applies no redirect
    // policy, which is shared behaviour well outside this manager.
    if !url.starts_with("https://") {
        warn!(
            "macos-app:{name}: '{url}' is not https; the download is verified by its sha256, but prefer https where the host offers it"
        );
    }
    let Some(sha256) = sha256 else {
        eyre::bail!("macos-app:{name}: 'url' requires 'sha256'");
    };
    // A URL ending in `.git` is cloned rather than downloaded, and a clone has
    // no archive to hash, so `fetch_and_stage` never reaches the checksum step.
    // Accepting one would record a `sha256` that is never verified and install
    // an unchecked bundle from a declaration that looks pinned — this manager's
    // entire integrity story is that digest. Checked against the interpolated
    // URL because that is the string the fetcher dispatches on.
    if url.replace("{{version}}", version).ends_with(".git") {
        eyre::bail!(
            "macos-app:{name}: 'url' must be a direct download of the artifact; a '.git' URL is cloned and cannot be verified against 'sha256'"
        );
    }
    // `no_check` is a Homebrew sentinel that makes the shared fetcher skip
    // verification. It exists for casks whose URL serves a moving target, which
    // an inline declaration never is — accepting it here would silently drop
    // the verification this manager requires. Demanding a well-formed digest
    // also catches a truncated or mistyped one before the download, not after.
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        eyre::bail!("macos-app:{name}: 'sha256' must be a 64-character hex digest; got '{sha256}'");
    }
    let Some(artifact) = artifact.map(str::trim).filter(|a| !a.is_empty()) else {
        eyre::bail!("macos-app:{name}: 'url' requires 'artifact' naming the bundle to install");
    };
    if version.trim().is_empty() {
        eyre::bail!("macos-app:{name}: 'version' must not be blank");
    }
    // Rejected rather than trimmed: the version is interpolated into `url` and
    // joined into install-record paths, so a value that validates must be
    // exactly the value that gets used. Trimming here would let " latest "
    // past the check below and put the padded form on disk.
    if version != version.trim() {
        eyre::bail!(
            "macos-app:{name}: 'version' must not have leading or trailing whitespace; got '{version}'"
        );
    }
    if version == "latest" {
        eyre::bail!(
            "macos-app:{name}: 'url' requires an explicit 'version'; mise cannot discover versions for a direct download"
        );
    }
    Ok((sha256, artifact))
}

#[cfg(unix)]
impl AppSpec {
    /// Build a validated declaration. See [`validate_app_declaration`] for the
    /// rules and why every field is required.
    pub fn parse(
        name: &str,
        url: &str,
        sha256: Option<&str>,
        artifact: Option<&str>,
        version: &str,
    ) -> eyre::Result<Self> {
        let (sha256, artifact) = validate_app_declaration(name, url, sha256, artifact, version)?;
        Ok(Self {
            url: url.replace("{{version}}", version),
            sha256: sha256.to_string(),
            artifact: artifact.to_string(),
            version: version.to_string(),
        })
    }
}

pub fn brew_tap_name(name: &str) -> Option<&str> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    let formula = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() || formula.is_empty() {
        return None;
    }
    if owner == "homebrew" && (tap == "core" || tap == "cask") {
        None
    } else {
        name.rsplit_once('/').map(|(tap, _)| tap)
    }
}
