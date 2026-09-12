//! Stamps: which packslip releases a trusted host says may be installed.
//!
//! A stamping host, a registry, a mirror, or a scanning service, publishes
//! a signed `releases/v1` list per vendor project at
//! `https://<host>/.well-known/packslip/<project>.json`. Each entry pins
//! the digest of the packslip the host checked and carries what it checked.
//! When the `packslip.stampers` setting names such hosts, a version none of
//! them lists is not released as far as mise is concerned: it is not
//! offered and not installed, however valid the vendor's own document is.
//! Any one trusted host's stamp suffices. A tool whose options say
//! `trust = "vendor"` takes the vendor's document alone, with no stamp.
//!
//! The stamp never replaces the vendor's signature: the bundle a stamp
//! points at is still verified against the pin the project name implies.
//! The stamp says who selected the release and what they checked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr, bail, eyre};
use itertools::Itertools;
use packslip::model::{ReleaseListStatement, ReleaseRef};
use packslip::sigstore::Policy;
use serde::{Deserialize, Serialize};

use crate::backend::packslip::{Pin, verify_release_list};
use crate::config::Settings;
use crate::dirs;
use crate::file;
use crate::http::HTTP_FETCH;
use crate::toolset::ToolVersionOptions;

/// GitHub's OIDC issuer, for a stamper pinned by a workflow identity.
const GITHUB_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// A host whose stamps mise trusts, and how its lists are pinned.
pub(crate) struct Stamper {
    pub(crate) host: String,
    pin: Pin,
}

impl Stamper {
    /// `host=PIN`, where PIN is a minisign public key line, the path of a
    /// `.pub` file, or an `https://` identity prefix for a keyless signer
    /// on GitHub such as `https://github.com/org/registry/`.
    pub(crate) fn parse(spec: &str) -> Result<Stamper> {
        let spec = spec.trim();
        let Some((host, pin)) = spec.split_once('=') else {
            bail!(
                "packslip.stampers entry {spec:?} has no pin; write host=PUBKEY, host=path/to/key.pub, or host=https://github.com/org/repo/"
            );
        };
        let host = host.trim();
        let pin = pin.trim();
        if host.is_empty() || !host.contains('.') || host.contains('/') {
            bail!("packslip.stampers entry {spec:?}: {host:?} is not a host name");
        }
        let pin = if pin.starts_with("https://") {
            Pin::Identity(Policy {
                issuer: Some(GITHUB_ISSUER.into()),
                identity: None,
                identity_prefix: Some(pin.to_string()),
            })
        } else {
            let text = if std::path::Path::new(pin).is_file() {
                file::read_to_string(pin)?
            } else {
                pin.to_string()
            };
            let key = packslip::minisign::PublicKey::parse(&text)
                .map_err(|e| eyre!("packslip.stampers entry for {host}: pubkey: {e}"))?;
            Pin::Key(key)
        };
        Ok(Stamper {
            host: host.to_string(),
            pin,
        })
    }

    /// Where this host publishes its list for a vendor project.
    pub(crate) fn url(&self, project: &str) -> String {
        format!("https://{}/.well-known/packslip/{project}.json", self.host)
    }
}

/// The stampers the settings name, or none when stamping is off.
pub(crate) fn stampers() -> Result<Vec<Stamper>> {
    Settings::get()
        .packslip
        .stampers
        .iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(|s| Stamper::parse(s))
        .collect()
}

/// Whether a tool's options take the vendor's document alone.
pub(crate) fn trusts_vendor(opts: &ToolVersionOptions) -> bool {
    matches!(opts.get("trust"), Some("vendor"))
}

/// One stamp: a trusted host's entry for a version.
#[derive(Debug, Clone)]
pub(crate) struct Stamp {
    pub(crate) host: String,
    pub(crate) entry: ReleaseRef,
    /// The sha256 the host recorded for the packslip it points at.
    pub(crate) digest: Option<String>,
}

/// Every stamp the trusted hosts gave a project, by version.
#[derive(Debug, Default)]
pub(crate) struct Stamps {
    hosts: Vec<String>,
    stamps: BTreeMap<String, Stamp>,
    /// Versions a trusted host withdrew, with the host and reason.
    yanked: BTreeMap<String, String>,
}

impl Stamps {
    /// Fold in one host's verified list. The first host to stamp a version
    /// is the one followed; a yank withdraws only that host's approval.
    fn add(&mut self, host: &str, list: &ReleaseListStatement) {
        self.hosts.push(host.to_string());
        for entry in &list.predicate.releases {
            if entry.is_yanked() {
                let reason = entry
                    .status_reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default();
                self.yanked
                    .insert(entry.version.clone(), format!("{host}{reason}"));
                continue;
            }
            self.stamps
                .entry(entry.version.clone())
                .or_insert_with(|| Stamp {
                    host: host.to_string(),
                    entry: entry.clone(),
                    digest: list.digest_of(&entry.packslip).map(str::to_string),
                });
        }
    }

    /// The first non-yanked stamp that admits a version.
    pub(crate) fn stamp(&self, version: &str) -> Option<&Stamp> {
        self.stamps.get(version)
    }

    pub(crate) fn allows(&self, version: &str) -> bool {
        self.stamp(version).is_some()
    }

    /// Why a version is not installable.
    pub(crate) fn refusal(&self, project: &str, version: &str) -> eyre::Report {
        if let Some(by) = self.yanked.get(version) {
            return eyre!("packslip:{project}@{version} was withdrawn by {by}");
        }
        eyre!(
            "packslip:{project}@{version} carries no stamp from {}; mise installs only versions a trusted stamper lists. Set `trust = \"vendor\"` in the tool's options to take the vendor's own manifest instead",
            self.hosts.iter().join(", ")
        )
    }
}

/// What mise remembers about a stamper between runs: the highest list
/// sequence it accepted per project, so a replayed older list is refused.
#[derive(Debug, Default, Serialize, Deserialize)]
struct HostState {
    #[serde(default)]
    sequences: BTreeMap<String, u64>,
}

fn state_dir() -> PathBuf {
    dirs::STATE.join("packslip").join("stampers")
}

fn state_path(dir: &Path, host: &str) -> PathBuf {
    dir.join(format!("{host}.json"))
}

/// The state file is what stands between a replayed list and an install,
/// so a file that cannot be read is an error rather than an empty slate.
fn read_state(dir: &Path, host: &str) -> Result<HostState> {
    let path = state_path(dir, host);
    if !path.is_file() {
        return Ok(HostState::default());
    }
    let text = file::read_to_string(&path)?;
    serde_json::from_str(&text).wrap_err_with(|| {
        format!(
            "{} is not the stamper state mise wrote; remove it to start over, which forgets which lists were already accepted",
            path.display()
        )
    })
}

fn write_state(dir: &Path, host: &str, state: &HostState) -> Result<()> {
    file::create_dir_all(dir)?;
    file::write_atomic(state_path(dir, host), serde_json::to_vec_pretty(state)?)
}

/// One mise at a time reads and rewrites a host's state. The lock sits
/// beside that state, not under the cache: two mise processes can share a
/// state directory and disagree about their cache, and a lock they do not
/// share is no lock at all — the loser's write would drop an accepted
/// sequence and reopen the rollback this state exists to refuse.
fn locked(dir: &Path, host: &str) -> Result<fslock::LockFile> {
    file::create_dir_all(dir)?;
    let path = state_path(dir, host).with_extension("lock");
    let mut lock = fslock::LockFile::open(&path)?;
    if !lock.try_lock()? {
        debug!("waiting for lock on {}", path.display());
        lock.lock()?;
    }
    Ok(lock)
}

/// Refuse a list whose sequence is below the last one accepted from this
/// host for this project, and remember the new one.
fn check_sequence(host: &str, project: &str, list: &ReleaseListStatement) -> Result<()> {
    let dir = state_dir();
    let _lock = locked(&dir, host)?;
    check_sequence_in(&dir, host, project, list)
}

/// A host that answered 404 for a project it had a list for before is not
/// "no list": it would drop that list's yanks and let another host's stamp
/// stand alone. Refuse until the host publishes again.
fn missing_list(host: &str, project: &str, url: &str) -> Result<()> {
    let dir = state_dir();
    let _lock = locked(&dir, host)?;
    missing_list_in(&dir, host, project, url)
}

fn missing_list_in(dir: &Path, host: &str, project: &str, url: &str) -> Result<()> {
    let state = read_state(dir, host)?;
    if let Some(last) = state.sequences.get(project) {
        bail!(
            "{host} published a stamp list for {project} before (sequence {last}) but now answers 404 at {url}; refusing to treat that as no list, since it would drop the yanks that list carried"
        );
    }
    Ok(())
}

fn check_sequence_in(
    dir: &Path,
    host: &str,
    project: &str,
    list: &ReleaseListStatement,
) -> Result<()> {
    let mut state = read_state(dir, host)?;
    let sequence = list.predicate.sequence;
    if let Some(&last) = state.sequences.get(project)
        && sequence < last
    {
        bail!(
            "the stamp list from {host} for {project} has sequence {sequence}, below the {last} already accepted; refusing what may be a rollback"
        );
    }
    if state.sequences.get(project) != Some(&sequence) {
        state.sequences.insert(project.to_string(), sequence);
        write_state(dir, host, &state)?;
    }
    Ok(())
}

/// Fetch and verify every trusted host's list for a project. `None` when
/// no stampers are configured or the tool trusts its vendor alone. A host
/// that has no list for the project stamps nothing; one whose list fails
/// to verify is an error, since silently ignoring it would let a broken
/// host widen what another admits.
pub(crate) async fn fetch(project: &str, opts: &ToolVersionOptions) -> Result<Option<Stamps>> {
    if trusts_vendor(opts) {
        return Ok(None);
    }
    let stampers = stampers()?;
    if stampers.is_empty() {
        return Ok(None);
    }
    let mut stamps = Stamps::default();
    for stamper in &stampers {
        let url = stamper.url(project);
        let text = match HTTP_FETCH.get_text(&url).await {
            Ok(text) => text,
            Err(err) if is_not_found(&err) => {
                missing_list(&stamper.host, project, &url)?;
                debug!("{}: no stamp list for {project} at {url}", stamper.host);
                stamps.hosts.push(stamper.host.clone());
                continue;
            }
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("fetching the stamp list for {project} from {url}"));
            }
        };
        let list = verify_release_list(&text, &stamper.pin, true)
            .wrap_err_with(|| format!("verifying the stamp list from {}", stamper.host))?;
        if list.predicate.project != project {
            bail!(
                "the stamp list at {url} is for {}, not {project}",
                list.predicate.project
            );
        }
        check_sequence(&stamper.host, project, &list)?;
        stamps.add(&stamper.host, &list);
    }
    Ok(Some(stamps))
}

fn is_not_found(err: &eyre::Report) -> bool {
    crate::http::error_code(err) == Some(404)
}

#[cfg(test)]
mod tests {
    use super::*;
    use packslip::model::{
        Digest, Identity, RELEASES_PREDICATE_TYPE, ReleaseList, ReleaseStatus, STATEMENT_TYPE,
        Scheme, Subject,
    };

    fn list(project: &str, sequence: u64, entries: &[(&str, &str, bool)]) -> ReleaseListStatement {
        ReleaseListStatement {
            kind: STATEMENT_TYPE.into(),
            subject: entries
                .iter()
                .map(|(_, url, _)| Subject {
                    name: url.to_string(),
                    digest: Digest {
                        sha256: "a".repeat(64),
                        sha512: None,
                    },
                })
                .collect(),
            predicate_type: RELEASES_PREDICATE_TYPE.into(),
            predicate: ReleaseList {
                project: project.into(),
                generated_at: "2026-09-01T00:00:00Z".into(),
                expires_at: "2026-10-01T00:00:00Z".into(),
                sequence,
                latest: None,
                identity: Identity {
                    scheme: Scheme::SigstoreKey,
                    key_id: "5A0A0B8B9C6D7E1F".into(),
                    issuer: None,
                },
                releases: entries
                    .iter()
                    .map(|(version, url, yanked)| ReleaseRef {
                        version: version.to_string(),
                        published_at: "2026-09-01T00:00:00Z".into(),
                        packslip: url.to_string(),
                        status: yanked.then_some(ReleaseStatus::Yanked),
                        status_reason: yanked.then(|| "bad build".to_string()),
                        ..ReleaseRef::default()
                    })
                    .collect(),
                extensions: Default::default(),
            },
        }
    }

    #[test]
    fn stampers_parse_with_a_pin() {
        let key = Stamper::parse(
            "stamps.example.com=RWTAVOFZR6bcSVX+dSDiaFuHVVytD5UMn/HtxqPp/UbQDE5MW1nPZth7",
        )
        .unwrap();
        assert_eq!(key.host, "stamps.example.com");
        assert!(matches!(key.pin, Pin::Key(_)));
        assert_eq!(
            key.url("github.com/jdx/mise"),
            "https://stamps.example.com/.well-known/packslip/github.com/jdx/mise.json"
        );
        let identity =
            Stamper::parse(" registry.mise.jdx.dev = https://github.com/jdx/mise-registry/ ")
                .unwrap();
        assert!(matches!(identity.pin, Pin::Identity(_)));
        for bad in [
            "stamps.example.com",
            "=RWTAVOFZR6bcSVX+dSDiaFuHVVytD5UMn/HtxqPp/UbQDE5MW1nPZth7",
            "nodots=https://github.com/o/r/",
            "a.example.com/path=https://github.com/o/r/",
            "stamps.example.com=not a key",
        ] {
            assert!(Stamper::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn any_non_yanked_stamp_admits() {
        let mut stamps = Stamps::default();
        stamps.add(
            "a.example.com",
            &list(
                "github.com/o/r",
                1,
                &[
                    ("1.0.0", "https://x/1.0.0/packslip.sigstore.json", false),
                    ("1.1.0", "https://x/1.1.0/packslip.sigstore.json", false),
                ],
            ),
        );
        stamps.add(
            "b.example.com",
            &list(
                "github.com/o/r",
                7,
                &[
                    ("1.1.0", "https://y/1.1.0/packslip.sigstore.json", true),
                    ("1.2.0", "https://y/1.2.0/packslip.sigstore.json", false),
                ],
            ),
        );
        assert!(stamps.allows("1.0.0"));
        assert!(stamps.allows("1.2.0"));
        assert!(stamps.allows("1.1.0"), "a still approves it");
        assert!(!stamps.allows("2.0.0"), "nobody stamped it");
        let first = stamps.stamp("1.0.0").unwrap();
        assert_eq!(first.host, "a.example.com");
        assert_eq!(first.digest.as_deref(), Some("a".repeat(64).as_str()));
        assert_eq!(
            first.entry.packslip,
            "https://x/1.0.0/packslip.sigstore.json"
        );
        let mut reversed = Stamps::default();
        reversed.add(
            "b.example.com",
            &list(
                "github.com/o/r",
                1,
                &[("1.1.0", "https://y/1.1.0/p.json", true)],
            ),
        );
        assert!(!reversed.allows("1.1.0"));
        reversed.add(
            "a.example.com",
            &list(
                "github.com/o/r",
                1,
                &[("1.1.0", "https://x/1.1.0/p.json", false)],
            ),
        );
        assert!(reversed.allows("1.1.0"));
        let why = stamps.refusal("github.com/o/r", "2.0.0").to_string();
        assert!(
            why.contains("no stamp from a.example.com, b.example.com")
                && why.contains("trust = \"vendor\""),
            "{why}"
        );
    }

    #[test]
    fn sequences_only_go_up_per_host_and_project() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let host = "seq.example.com";
        let project = "github.com/o/r";
        let entries = [("1.0.0", "https://x/p.json", false)];
        check_sequence_in(d, host, project, &list(project, 5, &entries)).unwrap();
        check_sequence_in(d, host, project, &list(project, 5, &entries)).unwrap();
        check_sequence_in(d, host, project, &list(project, 6, &entries)).unwrap();
        let err = check_sequence_in(d, host, project, &list(project, 4, &entries)).unwrap_err();
        assert!(err.to_string().contains("rollback"), "{err}");
        assert!(d.join("seq.example.com.json").is_file());
        // A host that had a list for the project may not turn into "no list".
        let err = missing_list_in(d, host, project, "https://seq.example.com/x.json").unwrap_err();
        assert!(err.to_string().contains("sequence 6"), "{err}");
        missing_list_in(
            d,
            host,
            "github.com/o/unseen",
            "https://seq.example.com/y.json",
        )
        .unwrap();
        missing_list_in(
            d,
            "new.example.com",
            project,
            "https://new.example.com/y.json",
        )
        .unwrap();
        // Another project or host starts fresh.
        check_sequence_in(
            d,
            host,
            "github.com/o/other",
            &list("github.com/o/other", 1, &entries),
        )
        .unwrap();
        check_sequence_in(d, "other.example.com", project, &list(project, 1, &entries)).unwrap();
        // A state file mise cannot read is an error, not an empty slate.
        std::fs::write(d.join("seq.example.com.json"), b"{not json").unwrap();
        let err = check_sequence_in(d, host, project, &list(project, 9, &entries)).unwrap_err();
        assert!(err.to_string().contains("remove it"), "{err}");
    }

    #[test]
    fn the_sequence_lock_sits_beside_the_state_it_guards() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("stampers");
        let host = "lock.example.com";
        drop(locked(&d, host).unwrap());
        assert!(
            d.join("lock.example.com.lock").is_file(),
            "processes sharing a state directory must share the lock, so it \
             cannot live under a cache directory they may not share"
        );
    }

    #[test]
    fn vendor_trust_is_a_tool_option() {
        let mut opts = ToolVersionOptions::default();
        assert!(!trusts_vendor(&opts));
        opts.insert_option("trust".into(), toml::Value::String("vendor".into()))
            .unwrap();
        assert!(trusts_vendor(&opts));
    }
}
