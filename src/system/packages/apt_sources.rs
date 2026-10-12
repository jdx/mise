//! `[bootstrap.apt.sources]`: apt repositories added before apt packages.
//!
//! ```toml
//! [bootstrap.apt.sources]
//! git-core = { ppa = "git-core/ppa" }
//!
//! [bootstrap.apt.sources.docker]
//! uris = "https://download.docker.com/linux/ubuntu"
//! components = "stable"
//! key = "https://download.docker.com/linux/ubuntu/gpg"
//! fingerprint = "9DC858229FC7DD38854AE2D88D81803C0EBFCD88"
//! ```
//!
//! Each entry becomes `/etc/apt/sources.list.d/mise-<name>.sources`, a deb822
//! file whose `Signed-By` embeds the repository's armored signing key, so a key
//! mise adds is trusted only for its own repository and removing the file
//! removes the key with it. Inline keys need apt 2.4 or newer. A key is kept
//! once it is in the file with the declared fingerprints; mise fetches one only
//! when it is missing or does not match, and embeds only the declared keys
//! from what it fetched.

use std::path::{Path, PathBuf};

use eyre::{bail, eyre};
use serde::Deserialize;

use crate::http::HTTP_FETCH;
use crate::result::Result;
use crate::system::sudo;

const SOURCES_DIR: &str = "/etc/apt/sources.list.d";
/// Where earlier mise versions kept a source's key; removed once the key is
/// embedded in the source file.
const KEYRINGS_DIR: &str = "/etc/apt/keyrings";
crate::remove_by!("2027.10.0", "apt-legacy-keyring-cleanup");

/// One `[bootstrap.apt.sources.<name>]` entry as written in mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AptSourceTomlConfig {
    /// Launchpad PPA, `owner/archive`; the other fields are then derived.
    pub ppa: Option<String>,
    pub uris: Option<String>,
    pub suites: Option<String>,
    pub components: Option<String>,
    pub architectures: Option<String>,
    /// URL of the repository's signing key, armored or binary.
    pub key: Option<String>,
    /// Primary key fingerprint(s) the signing key must have.
    pub fingerprint: Option<Fingerprints>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Fingerprints {
    One(String),
    Many(Vec<String>),
}

impl Fingerprints {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(fingerprint) => vec![fingerprint],
            Self::Many(fingerprints) => fingerprints,
        }
    }
}

/// A validated `[bootstrap.apt.sources]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptSource {
    pub name: String,
    pub kind: AptSourceKind,
    /// Normalized (uppercase hex, no spaces), sorted, and deduplicated. Empty
    /// only for a PPA whose key mise looks up on Launchpad.
    pub fingerprints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AptSourceKind {
    Ppa {
        owner: String,
        archive: String,
    },
    Repository {
        uris: String,
        /// `None` takes the release codename at apply time.
        suites: Option<String>,
        components: Option<String>,
        architectures: Option<String>,
        key: String,
    },
}

impl AptSource {
    pub fn parse(name: &str, config: AptSourceTomlConfig) -> Result<Self> {
        validate_name(name)?;
        let fingerprints = normalize_fingerprints(
            config
                .fingerprint
                .map(Fingerprints::into_vec)
                .unwrap_or_default(),
        )
        .map_err(|err| eyre!("{name}: {err}"))?;
        for (field, value) in [
            ("ppa", &config.ppa),
            ("uris", &config.uris),
            ("suites", &config.suites),
            ("components", &config.components),
            ("architectures", &config.architectures),
            ("key", &config.key),
        ] {
            if let Some(value) = value
                && (value.trim().is_empty() || value.contains(['\n', '\r']))
            {
                bail!("{name}: {field} must be a non-empty single line");
            }
        }
        let kind = if let Some(ppa) = config.ppa {
            let given = [
                ("uris", config.uris.is_some()),
                ("suites", config.suites.is_some()),
                ("components", config.components.is_some()),
                ("architectures", config.architectures.is_some()),
                ("key", config.key.is_some()),
            ]
            .into_iter()
            .filter_map(|(field, set)| set.then_some(field))
            .collect::<Vec<_>>();
            if !given.is_empty() {
                bail!(
                    "{name}: a ppa entry cannot also set {}; mise derives them from the PPA",
                    given.join(", ")
                );
            }
            let (owner, archive) = parse_ppa(&ppa).map_err(|err| eyre!("{name}: {err}"))?;
            if fingerprints.len() > 1 {
                bail!("{name}: a PPA has a single signing key; give one fingerprint");
            }
            AptSourceKind::Ppa { owner, archive }
        } else {
            let Some(uris) = config.uris else {
                bail!("{name}: set either ppa or uris");
            };
            let Some(key) = config.key else {
                bail!("{name}: key is required, the URL of the repository's signing key");
            };
            if !key.starts_with("https://") && !key.starts_with("http://") {
                bail!("{name}: key must be an http(s) URL, got '{key}'");
            }
            if fingerprints.is_empty() {
                bail!(
                    "{name}: fingerprint is required with key, so a changed key cannot be trusted silently"
                );
            }
            AptSourceKind::Repository {
                uris: uris.trim().to_string(),
                suites: config.suites.map(|s| s.trim().to_string()),
                components: config.components.map(|s| s.trim().to_string()),
                architectures: config.architectures.map(|s| s.trim().to_string()),
                key,
            }
        };
        Ok(Self {
            name: name.to_string(),
            kind,
            fingerprints,
        })
    }

    fn sources_path(&self) -> PathBuf {
        // apt paths are POSIX on every host that renders them, including
        // Windows running the unit tests
        PathBuf::from(format!("{SOURCES_DIR}/mise-{}.sources", self.name))
    }

    fn legacy_keyring_path(&self) -> PathBuf {
        PathBuf::from(format!("{KEYRINGS_DIR}/mise-{}.gpg", self.name))
    }

    /// Each repository and suite this source adds on `release`, as
    /// [`origin_key`] spells them. A URL alone is not enough: a declared
    /// testing or backports suite shares its mirror with the release's own.
    fn origins(&self, release: &Release) -> Vec<String> {
        let Ok(Some(fields)) = self.fields(release) else {
            return vec![];
        };
        fields
            .uris
            .split_whitespace()
            .flat_map(|uri| {
                fields
                    .suites
                    .split_whitespace()
                    .map(move |suite| origin_key(uri, suite))
            })
            .collect()
    }

    fn describe(&self) -> String {
        match &self.kind {
            AptSourceKind::Ppa { owner, archive } => format!("ppa:{owner}/{archive}"),
            AptSourceKind::Repository { uris, .. } => uris.clone(),
        }
    }

    /// The deb822 stanza for this source on a release, or `None` when the
    /// source does not apply there (a PPA on a release not based on Ubuntu).
    fn render(&self, fields: &SourceFields, key: &str) -> String {
        let SourceFields {
            uris,
            suites,
            components,
            architectures,
        } = fields;
        let mut out = format!(
            "# Written by mise for [bootstrap.apt.sources.{}]; changes here are overwritten.\n\
             Types: deb\n\
             URIs: {uris}\n\
             Suites: {suites}\n",
            self.name
        );
        if let Some(components) = components {
            out.push_str(&format!("Components: {components}\n"));
        }
        if let Some(architectures) = architectures {
            out.push_str(&format!("Architectures: {architectures}\n"));
        }
        out.push_str(&inline_signed_by(key));
        out
    }

    /// The deb822 fields for this source on `release`, defaults resolved, or
    /// `None` when the source does not apply there.
    fn fields(&self, release: &Release) -> Result<Option<SourceFields>> {
        let (uris, suites, components, architectures) = match &self.kind {
            AptSourceKind::Ppa { owner, archive } => {
                let Some(codename) = release.ubuntu_codename() else {
                    return Ok(None);
                };
                (
                    ppa_uri(owner, archive),
                    codename.to_string(),
                    Some("main".to_string()),
                    None,
                )
            }
            AptSourceKind::Repository {
                uris,
                suites,
                components,
                architectures,
                ..
            } => {
                // vendors publish for Ubuntu releases, not for derivatives
                // such as Linux Mint, which name their Ubuntu base separately
                let suites = match suites {
                    Some(suites) => suites.clone(),
                    None => match release
                        .ubuntu_codename()
                        .or(release.version_codename.as_deref())
                    {
                        Some(codename) => codename.to_string(),
                        None => bail!(
                            "{}: /etc/os-release has no VERSION_CODENAME; set suites",
                            self.name
                        ),
                    },
                };
                // a flat repository names an exact path and takes no components
                let components = match components {
                    Some(components) => Some(components.clone()),
                    None if suites.ends_with('/') => None,
                    None => Some("main".to_string()),
                };
                (uris.clone(), suites, components, architectures.clone())
            }
        };
        Ok(Some(SourceFields {
            uris,
            suites,
            components,
            architectures,
        }))
    }

    /// Whether the keyring on disk is what this source declares: exactly the
    /// declared fingerprints, or for a PPA without one, any readable key.
    fn keyring_current(&self, keyring: Option<&[u8]>) -> bool {
        let Some(keys) = keyring.and_then(|data| crate::gpg::parse_keyring(data).ok()) else {
            return false;
        };
        let mut found = keys.into_iter().map(|k| k.fingerprint).collect::<Vec<_>>();
        found.sort();
        found.dedup();
        if self.fingerprints.is_empty() {
            !found.is_empty()
        } else {
            found == self.fingerprints
        }
    }

    /// Download the signing key and keep only the declared certificates,
    /// armored for a `Signed-By` field.
    async fn fetch_keyring(&self) -> Result<String> {
        let (url, fingerprints) = match &self.kind {
            AptSourceKind::Ppa { owner, archive } => {
                let fingerprints = if self.fingerprints.is_empty() {
                    vec![launchpad_fingerprint(owner, archive).await?]
                } else {
                    self.fingerprints.clone()
                };
                // a PPA has a single signing key
                let url = format!(
                    "https://keyserver.ubuntu.com/pks/lookup?op=get&options=mr&search=0x{}",
                    fingerprints[0]
                );
                (url, fingerprints)
            }
            AptSourceKind::Repository { key, .. } => (key.clone(), self.fingerprints.clone()),
        };
        debug!("apt source {}: fetching signing key {url}", self.name);
        let data = HTTP_FETCH.get_bytes(&url).await?;
        let keys = crate::gpg::parse_keyring(data.as_ref())
            .map_err(|err| eyre!("{}: reading the key from {url}: {err}", self.name))?;
        Ok(armor(&select_keys(&self.name, &url, keys, &fingerprints)?))
    }
}

/// The installed apt's major and minor version, from `apt-get --version`.
fn apt_version() -> Option<(u32, u32)> {
    let output = std::process::Command::new("apt-get")
        .arg("--version")
        .output()
        .ok()?;
    parse_apt_version(&String::from_utf8_lossy(&output.stdout))
}

fn parse_apt_version(output: &str) -> Option<(u32, u32)> {
    let version = output.lines().next()?.split_whitespace().nth(1)?;
    let mut parts = version.split(['.', '~', '+']);
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// A `Signed-By` field holding `key` inline: deb822 continuation lines with one
/// leading space, and `.` standing for a blank line.
fn inline_signed_by(key: &str) -> String {
    let mut out = String::from("Signed-By:\n");
    for line in key.lines() {
        out.push(' ');
        out.push_str(if line.is_empty() { "." } else { line });
        out.push('\n');
    }
    out
}

/// The armored key embedded in a source file's `Signed-By`, if any.
fn inline_key(stanza: &str) -> Option<String> {
    let mut lines = stanza
        .lines()
        .skip_while(|line| line.trim_end() != "Signed-By:");
    lines.next()?;
    let key = lines
        .map_while(|line| line.strip_prefix(' '))
        .map(|line| if line == "." { "" } else { line })
        .collect::<Vec<_>>();
    (!key.is_empty()).then(|| key.join("\n") + "\n")
}

/// ASCII-armor binary OpenPGP packets as a public key block.
fn armor(data: &[u8]) -> String {
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD.encode(data);
    let mut out = String::from("-----BEGIN PGP PUBLIC KEY BLOCK-----\n\n");
    for chunk in body.as_bytes().chunks(64) {
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    let crc = crc24(data).to_be_bytes();
    out.push_str(&format!(
        "={}\n-----END PGP PUBLIC KEY BLOCK-----\n",
        base64::engine::general_purpose::STANDARD.encode(&crc[1..])
    ));
    out
}

/// The OpenPGP armor checksum (RFC 4880 section 6.1).
fn crc24(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xB704CE;
    for &byte in data {
        crc ^= u32::from(byte) << 16;
        for _ in 0..8 {
            crc <<= 1;
            if crc & 0x1000000 != 0 {
                crc ^= 0x1864CFB;
            }
        }
    }
    crc & 0xFFFFFF
}

struct SourceFields {
    uris: String,
    suites: String,
    components: Option<String>,
    architectures: Option<String>,
}

/// Every repository and suite the declared sources add on this machine.
pub(crate) fn declared_origins(sources: &[AptSource]) -> std::collections::HashSet<String> {
    let release = Release::read();
    sources
        .iter()
        .flat_map(|source| source.origins(&release))
        .collect()
}

/// A repository as `apt-cache policy` names it on an origin line,
/// `500 <uri> <suite>[/<component>] <arch> Packages`: the URI without a
/// trailing slash, then the suite without its component.
pub(crate) fn origin_key(uri: &str, suite: &str) -> String {
    let uri = uri.trim_end_matches('/');
    match suite.split('/').next().filter(|suite| !suite.is_empty()) {
        Some(suite) => format!("{uri} {suite}"),
        None => uri.to_string(),
    }
}

fn ppa_uri(owner: &str, archive: &str) -> String {
    format!("https://ppa.launchpadcontent.net/{owner}/{archive}/ubuntu/")
}

/// The declared certificates from a downloaded keyring, in declaration order.
/// Anything else the download carried is dropped rather than trusted.
fn select_keys(
    name: &str,
    url: &str,
    keys: Vec<crate::gpg::KeyringKey>,
    fingerprints: &[String],
) -> Result<Vec<u8>> {
    let mut out = vec![];
    for fingerprint in fingerprints {
        let Some(key) = keys.iter().find(|k| &k.fingerprint == fingerprint) else {
            let found = keys
                .iter()
                .map(|k| k.fingerprint.as_str())
                .collect::<Vec<_>>();
            bail!(
                "{name}: the key at {url} does not have fingerprint {fingerprint} (found: {})",
                if found.is_empty() {
                    "no keys".to_string()
                } else {
                    found.join(", ")
                }
            );
        };
        out.extend_from_slice(&key.bytes);
    }
    Ok(out)
}

/// The signing key fingerprint Launchpad publishes for a PPA.
async fn launchpad_fingerprint(owner: &str, archive: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Archive {
        signing_key_fingerprint: Option<String>,
    }
    let url = format!("https://api.launchpad.net/1.0/~{owner}/+archive/ubuntu/{archive}");
    let archive_info: Archive = HTTP_FETCH
        .json(&url)
        .await
        .map_err(|err| eyre!("looking up ppa:{owner}/{archive} on Launchpad: {err}"))?;
    let Some(fingerprint) = archive_info.signing_key_fingerprint else {
        bail!(
            "ppa:{owner}/{archive} has no signing key yet; Launchpad creates one after its first build"
        );
    };
    normalize_fingerprints(vec![fingerprint])?
        .pop()
        .ok_or_else(|| eyre!("ppa:{owner}/{archive}: Launchpad returned an empty fingerprint"))
}

/// apt reads only files in `sources.list.d` whose names are made of letters,
/// digits, `_`, `-`, and `.`; anything else would be silently ignored.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        bail!(
            "'{name}' is not a valid source name; use letters, digits, '_', '-', and '.', not starting with '.'"
        );
    }
    Ok(())
}

fn parse_ppa(ppa: &str) -> Result<(String, String)> {
    let ppa = ppa.trim();
    let ppa = ppa.strip_prefix("ppa:").unwrap_or(ppa);
    let valid = |part: &str| {
        part.chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && part.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.')
            })
    };
    match ppa.split_once('/') {
        Some((owner, archive)) if valid(owner) && valid(archive) => {
            Ok((owner.to_string(), archive.to_string()))
        }
        _ => bail!("ppa must be 'owner/archive', as in 'git-core/ppa', got '{ppa}'"),
    }
}

fn normalize_fingerprints(fingerprints: Vec<String>) -> Result<Vec<String>> {
    let mut out = fingerprints
        .into_iter()
        .map(|fingerprint| {
            let normalized = fingerprint
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .to_ascii_uppercase();
            let normalized = normalized.strip_prefix("0X").unwrap_or(&normalized);
            if !matches!(normalized.len(), 40 | 64)
                || !normalized.chars().all(|c| c.is_ascii_hexdigit())
            {
                bail!(
                    "'{fingerprint}' is not a full key fingerprint (40 or 64 hex digits); short key IDs are not accepted"
                );
            }
            Ok(normalized.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    out.sort();
    out.dedup();
    Ok(out)
}

/// The parts of `/etc/os-release` apt sources depend on.
#[derive(Debug, Default, Clone)]
struct Release {
    id: String,
    version_codename: Option<String>,
    ubuntu_codename: Option<String>,
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
                "VERSION_CODENAME" => release.version_codename = value,
                "UBUNTU_CODENAME" => release.ubuntu_codename = value,
                _ => {}
            }
        }
        release
    }

    /// PPAs publish per Ubuntu release. Derivatives such as Linux Mint name
    /// their Ubuntu base in `UBUNTU_CODENAME`.
    fn ubuntu_codename(&self) -> Option<&str> {
        self.ubuntu_codename.as_deref().or_else(|| {
            (self.id == "ubuntu")
                .then_some(self.version_codename.as_deref())
                .flatten()
        })
    }
}

/// What [`apply`] did.
pub(crate) struct Applied {
    /// The package lists were refreshed (or, in a dry run, would be).
    pub(crate) refreshed: bool,
    /// The sources that could not be brought to their declared state, such
    /// as an unreachable key URL. One failing source does not stop the others
    /// or the refresh.
    pub(crate) failures: Vec<eyre::Report>,
}

/// Bring every source to its declared state, then refresh package lists if
/// anything changed so packages from the new sources can be installed.
/// Failures, including a failed refresh, are collected in [`Applied`] rather
/// than returned, so none of them hides another; only a failure to print a
/// dry run's plan, or to ask, is returned. With `ask`, mise asks once before
/// changing any source, and a no leaves every source as it is.
pub(crate) async fn apply(sources: &[AptSource], dry_run: bool, ask: bool) -> Result<Applied> {
    let mut applied = Applied {
        refreshed: false,
        failures: vec![],
    };
    if sources.is_empty() {
        return Ok(applied);
    }
    let release = Release::read();
    let mut apt_checked = false;
    let mut declared = vec![];
    let mut pending = vec![];
    for source in sources {
        let fields = match source.fields(&release) {
            Ok(Some(fields)) => fields,
            Ok(None) => {
                debug!(
                    "apt source {}: skipping {}, this release is not based on Ubuntu",
                    source.name,
                    source.describe()
                );
                continue;
            }
            Err(err) => {
                applied.failures.push(err);
                continue;
            }
        };
        // only a source that applies here needs an inline key; a PPA skipped
        // on Debian must not fail on an old apt
        if !apt_checked {
            apt_checked = true;
            match apt_version() {
                Some(version) if version >= (2, 4) => {}
                found => {
                    let found = found
                        .map_or("an unknown version of apt".to_string(), |(major, minor)| {
                            format!("apt {major}.{minor}")
                        });
                    applied.failures.push(eyre!(
                        "{found} cannot be relied on to read signing keys embedded in a source \
                         file; [bootstrap.apt.sources] needs apt 2.4 or newer"
                    ));
                    return Ok(applied);
                }
            }
        }
        let sources_path = source.sources_path();
        declared.push((source, sources_path.clone()));
        let existing = std::fs::read_to_string(&sources_path).ok();
        let existing_key = existing.as_deref().and_then(inline_key);
        let has_key = source.keyring_current(existing_key.as_deref().map(str::as_bytes));
        let stanza_current = has_key
            && existing_key.as_deref().is_some_and(|key| {
                existing.as_deref() == Some(source.render(&fields, key).as_str())
            });
        // without a declared fingerprint, any key in the file passes the check,
        // so a source that points somewhere new (another PPA under the same
        // name) must fetch the key that goes with it
        let key_current = has_key && (stanza_current || !source.fingerprints.is_empty());
        if key_current && stanza_current && !source.legacy_keyring_path().exists() {
            debug!("apt source {}: up to date", source.name);
            continue;
        }
        pending.push(PendingSource {
            source,
            fields,
            existed: existing.is_some(),
            key: key_current.then_some(existing_key).flatten(),
            stanza_current,
        });
    }
    let mut changed = false;
    if dry_run {
        for p in &pending {
            if p.key.is_none() {
                miseprintln!("fetch the signing key for {}", p.source.describe());
            }
            if !p.stanza_current {
                miseprintln!("write {}", p.source.sources_path().display());
            }
            if p.source.legacy_keyring_path().exists() {
                miseprintln!("remove {}", p.source.legacy_keyring_path().display());
            }
        }
        changed = pending.iter().any(PendingSource::writes);
    } else if ask && pending.iter().any(PendingSource::writes) && !confirm(&pending)? {
        info!("apt: sources left unchanged");
        // a source left as it was must not be warned about for its lists
        declared.retain(|(source, _)| !pending.iter().any(|p| p.source.name == source.name));
    } else {
        for p in &pending {
            let result: Result<()> = async {
                let key = match &p.key {
                    Some(key) => key.clone(),
                    None => p.source.fetch_keyring().await?,
                };
                if p.writes() {
                    let stanza = p.source.render(&p.fields, &key);
                    write_root_file(&p.source.sources_path(), stanza.as_bytes())?;
                    changed = true;
                }
                let legacy = p.source.legacy_keyring_path();
                if legacy.exists() {
                    sudo::run("rm", &["-f".into(), legacy.display().to_string()], &[])?;
                }
                Ok(())
            }
            .await;
            match result {
                Ok(()) => info!(
                    "apt: {} source {} ({})",
                    if p.existed { "updated" } else { "added" },
                    p.source.name,
                    p.source.describe()
                ),
                Err(err) => {
                    // its error explains it; a missing-lists warning would
                    // only repeat that
                    declared.retain(|(source, _)| source.name != p.source.name);
                    applied.failures.push(err.wrap_err(format!(
                        "apt source {} ({})",
                        p.source.name,
                        p.source.describe()
                    )));
                }
            }
        }
    }
    // A source whose lists were never fetched needs a refresh even though its
    // files are current: the run that wrote them may have failed, or stopped,
    // before its `apt-get update` finished.
    if changed || !sources_without_lists(&declared).is_empty() {
        let args = vec!["update".to_string()];
        let env = vec![("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())];
        if dry_run {
            miseprintln!("{}", sudo::argv_with_env("apt-get", &args, &env).join(" "));
            applied.refreshed = true;
        } else if let Err(err) = sudo::run("apt-get", &args, &env) {
            // the lists are not fresh, so the install's own refresh check
            // still runs
            applied.failures.push(err);
        } else {
            applied.refreshed = true;
            // apt reports a repository it could not fetch as a warning and
            // still exits 0
            for name in sources_without_lists(&declared) {
                warn!(
                    "apt: could not fetch the package lists for source {name}; see the apt-get update output above"
                );
            }
        }
    }
    Ok(applied)
}

/// A source whose files differ from its declaration.
struct PendingSource<'a> {
    source: &'a AptSource,
    fields: SourceFields,
    existed: bool,
    /// The armored key already in the source file, when it can stay.
    key: Option<String>,
    stanza_current: bool,
}

impl PendingSource<'_> {
    /// Whether the source file changes, as opposed to only a legacy keyring
    /// being removed.
    fn writes(&self) -> bool {
        self.key.is_none() || !self.stanza_current
    }
}

fn confirm(pending: &[PendingSource<'_>]) -> Result<bool> {
    let list = pending
        .iter()
        .filter(|p| p.writes())
        .map(|p| format!("{} ({})", p.source.name, p.source.describe()))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(crate::ui::prompt::confirm(format!("apt: add or update sources {list}?"))?.is_yes())
}

/// The declared sources apt has not fetched lists for. `apt-get indextargets
/// --no-release-info` lists every configured index; without the flag it lists
/// only those whose Release file has been fetched. Neither needs root.
fn sources_without_lists<'a>(declared: &[(&'a AptSource, PathBuf)]) -> Vec<&'a str> {
    let entries = |release_info: bool| -> Option<std::collections::HashSet<PathBuf>> {
        let mut cmd = std::process::Command::new("apt-get");
        cmd.arg("indextargets");
        if !release_info {
            cmd.arg("--no-release-info");
        }
        let output = cmd
            .args(["--format", "$(SOURCESENTRY)"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        Some(parse_sources_entries(&String::from_utf8_lossy(
            &output.stdout,
        )))
    };
    // when apt cannot answer, assume the lists are there rather than
    // refreshing on every run
    let (Some(configured), Some(fetched)) = (entries(false), entries(true)) else {
        return vec![];
    };
    declared
        .iter()
        .filter(|(_, path)| configured.contains(path) && !fetched.contains(path))
        .map(|(source, _)| source.name.as_str())
        .collect()
}

/// The files named by `$(SOURCESENTRY)` lines, which read `<path>:<stanza>`.
fn parse_sources_entries(output: &str) -> std::collections::HashSet<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.trim().rsplit_once(':'))
        .map(|(path, _)| PathBuf::from(path))
        .collect()
}

/// Write a world-readable root-owned file. The content goes over stdin so a
/// key never appears in argv or logs.
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

    fn ppa(name: &str, ppa: &str) -> AptSource {
        AptSource::parse(
            name,
            AptSourceTomlConfig {
                ppa: Some(ppa.into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn docker() -> AptSourceTomlConfig {
        AptSourceTomlConfig {
            uris: Some("https://download.docker.com/linux/ubuntu".into()),
            components: Some("stable".into()),
            key: Some("https://download.docker.com/linux/ubuntu/gpg".into()),
            fingerprint: Some(Fingerprints::One(
                "9DC8 5822 9FC7 DD38 854A  E2D8 8D81 803C 0EBF CD88".into(),
            )),
            ..Default::default()
        }
    }

    const NOBLE: &str = "ID=ubuntu\nVERSION_CODENAME=noble\nUBUNTU_CODENAME=noble\n";
    const MINT: &str =
        "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_CODENAME=wilma\nUBUNTU_CODENAME=noble\n";
    const BOOKWORM: &str = "ID=debian\nVERSION_CODENAME=bookworm\n";

    const KEY: &str =
        "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nAAAA\n-----END PGP PUBLIC KEY BLOCK-----\n";

    fn render(source: &AptSource, release: &Release) -> Result<Option<String>> {
        Ok(source
            .fields(release)?
            .map(|fields| source.render(&fields, KEY)))
    }

    #[test]
    fn parses_apt_versions() {
        assert_eq!(
            parse_apt_version("apt 2.7.14 (amd64)\nLanguage: en\n"),
            Some((2, 7))
        );
        assert_eq!(parse_apt_version("apt 2.2.4 (amd64)\n"), Some((2, 2)));
        assert_eq!(parse_apt_version("garbage"), None);
    }

    #[test]
    fn inline_key_round_trips_through_the_stanza() {
        let source = ppa("git-core", "ppa:git-core/ppa");
        let stanza = render(&source, &Release::parse(NOBLE)).unwrap().unwrap();
        assert_eq!(inline_key(&stanza).as_deref(), Some(KEY));
        assert_eq!(inline_key("Types: deb\nSigned-By: /x.gpg\n"), None);
    }

    #[test]
    fn armor_is_readable_as_a_keyring() {
        let keys = crate::gpg::parse_keyring(include_bytes!(
            "../../../crates/mise-util/src/assets/gpg/swift.asc"
        ))
        .unwrap();
        let bytes = keys
            .iter()
            .flat_map(|k| k.bytes.clone())
            .collect::<Vec<_>>();
        let reparsed = crate::gpg::parse_keyring(armor(&bytes).as_bytes()).unwrap();
        assert_eq!(reparsed.len(), keys.len());
    }

    #[test]
    fn ppa_renders_launchpad_stanza_for_ubuntu_release() {
        let source = ppa("git-core", "ppa:git-core/ppa");
        assert_eq!(
            render(&source, &Release::parse(NOBLE)).unwrap().unwrap(),
            "# Written by mise for [bootstrap.apt.sources.git-core]; changes here are overwritten.\n\
             Types: deb\n\
             URIs: https://ppa.launchpadcontent.net/git-core/ppa/ubuntu/\n\
             Suites: noble\n\
             Components: main\n\
             Signed-By:\n \
             -----BEGIN PGP PUBLIC KEY BLOCK-----\n .\n AAAA\n -----END PGP PUBLIC KEY BLOCK-----\n"
        );
    }

    #[test]
    fn ppa_uses_ubuntu_base_on_derivatives_and_skips_debian() {
        let source = ppa("fish", "fish-shell/release-4");
        let mint = render(&source, &Release::parse(MINT)).unwrap().unwrap();
        assert!(mint.contains("Suites: noble\n"), "{mint}");
        assert_eq!(render(&source, &Release::parse(BOOKWORM)).unwrap(), None);
    }

    #[test]
    fn repository_defaults_suite_to_codename() {
        let source = AptSource::parse("docker", docker()).unwrap();
        assert_eq!(
            source.fingerprints,
            vec!["9DC858229FC7DD38854AE2D88D81803C0EBFCD88"]
        );
        let stanza = render(&source, &Release::parse(BOOKWORM)).unwrap().unwrap();
        assert!(
            stanza.contains("Suites: bookworm\nComponents: stable\n"),
            "{stanza}"
        );
        assert!(stanza.contains("Signed-By:\n -----BEGIN PGP PUBLIC KEY BLOCK-----\n"));
        assert!(render(&source, &Release::default()).is_err());
        let mint = render(&source, &Release::parse(MINT)).unwrap().unwrap();
        assert!(mint.contains("Suites: noble\n"), "{mint}");
    }

    #[test]
    fn sources_entries_name_the_file() {
        let entries = parse_sources_entries(
            "/etc/apt/sources.list.d/ubuntu.sources:1\n/etc/apt/sources.list.d/mise-x.sources:1\n",
        );
        assert!(entries.contains(Path::new("/etc/apt/sources.list.d/mise-x.sources")));
        assert!(entries.contains(Path::new("/etc/apt/sources.list.d/ubuntu.sources")));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn origins_pair_each_uri_with_each_suite() {
        let git = ppa("git-core", "git-core/ppa");
        assert_eq!(
            git.origins(&Release::parse(NOBLE)),
            vec!["https://ppa.launchpadcontent.net/git-core/ppa/ubuntu noble"]
        );
        assert!(git.origins(&Release::parse(BOOKWORM)).is_empty());
        let backports = AptSource::parse(
            "backports",
            AptSourceTomlConfig {
                uris: Some("https://deb.debian.org/debian/".into()),
                suites: Some("bookworm-backports".into()),
                ..docker()
            },
        )
        .unwrap();
        assert_eq!(
            backports.origins(&Release::parse(BOOKWORM)),
            vec!["https://deb.debian.org/debian bookworm-backports"]
        );
        assert_eq!(
            origin_key("https://example.com/apt/", "./"),
            "https://example.com/apt ."
        );
        assert_eq!(
            origin_key("/var/lib/dpkg/status", ""),
            "/var/lib/dpkg/status"
        );
    }

    #[test]
    fn flat_repository_takes_no_components() {
        let source = AptSource::parse(
            "flat",
            AptSourceTomlConfig {
                suites: Some("./".into()),
                components: None,
                ..docker()
            },
        )
        .unwrap();
        let stanza = render(&source, &Release::parse(NOBLE)).unwrap().unwrap();
        assert!(!stanza.contains("Components:"), "{stanza}");
    }

    #[test]
    fn rejects_invalid_declarations() {
        let err = |name: &str, config: AptSourceTomlConfig| {
            AptSource::parse(name, config).unwrap_err().to_string()
        };
        assert!(err("bad/name", docker()).contains("not a valid source name"));
        assert!(err(".hidden", docker()).contains("not a valid source name"));
        assert!(
            err(
                "x",
                AptSourceTomlConfig {
                    fingerprint: None,
                    ..docker()
                }
            )
            .contains("fingerprint is required")
        );
        assert!(
            err(
                "x",
                AptSourceTomlConfig {
                    fingerprint: Some(Fingerprints::One("0EBFCD88".into())),
                    ..docker()
                }
            )
            .contains("short key IDs")
        );
        assert!(
            err(
                "x",
                AptSourceTomlConfig {
                    key: None,
                    ..docker()
                }
            )
            .contains("key is required")
        );
        assert!(
            err(
                "x",
                AptSourceTomlConfig {
                    components: Some("main\nSigned-By: /tmp/evil.gpg".into()),
                    ..docker()
                }
            )
            .contains("single line")
        );
        assert!(
            err(
                "x",
                AptSourceTomlConfig {
                    ppa: Some("git-core/ppa".into()),
                    ..docker()
                }
            )
            .contains("cannot also set uris, components, key")
        );
        assert!(err("x", AptSourceTomlConfig::default()).contains("set either ppa or uris"));
        assert!(
            err(
                "x",
                AptSourceTomlConfig {
                    ppa: Some("git-core/ppa".into()),
                    fingerprint: Some(Fingerprints::Many(vec!["A".repeat(40), "B".repeat(40)])),
                    ..Default::default()
                }
            )
            .contains("single signing key")
        );
        for bad in ["git-core", "Git-Core/ppa", "a/b/c", "/ppa"] {
            assert!(
                AptSource::parse(
                    "x",
                    AptSourceTomlConfig {
                        ppa: Some(bad.into()),
                        ..Default::default()
                    }
                )
                .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn keyring_must_hold_exactly_the_declared_keys() {
        let keys = crate::gpg::parse_keyring(include_bytes!(
            "../../../crates/mise-util/src/assets/gpg/swift.asc"
        ))
        .unwrap();
        assert!(keys.len() >= 2, "need two keys for this test");
        let fingerprint = keys[0].fingerprint.clone();
        let source = AptSource::parse(
            "x",
            AptSourceTomlConfig {
                fingerprint: Some(Fingerprints::One(fingerprint.clone())),
                ..docker()
            },
        )
        .unwrap();
        let selected = select_keys("x", "url", keys.clone(), &source.fingerprints).unwrap();
        assert!(source.keyring_current(Some(&selected)));
        let everything = keys
            .iter()
            .flat_map(|k| k.bytes.clone())
            .collect::<Vec<_>>();
        assert!(!source.keyring_current(Some(&everything)));
        assert!(!source.keyring_current(None));
        assert!(!source.keyring_current(Some(b"not a key")));

        let missing = select_keys("x", "url", keys[1..].to_vec(), &source.fingerprints)
            .unwrap_err()
            .to_string();
        assert!(missing.contains(&format!("does not have fingerprint {fingerprint}")));

        let any_key = ppa("p", "owner/ppa");
        assert!(any_key.keyring_current(Some(&everything)));
    }
}
