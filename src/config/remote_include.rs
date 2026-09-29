//! Remote config fragments (`include = [...]` in `mise.toml`).
//!
//! A fragment is a whole config file that another config pulls in, ranking just
//! below it. It is read on every config load, so it is fetched once and served
//! from a cache of its own. A reference that names immutable content (a git
//! commit sha or an OCI digest) is cached forever; a branch, tag or OCI tag is
//! refreshed once the cache is older than `fetch_remote_versions_cache`, and
//! the cached copy keeps working when a refresh fails or returns something
//! that does not load.
//!
//! Trust is the including file's. Outside paranoid mode a trusted config already
//! runs whatever its repository holds after the next `git pull`, so a fragment
//! that moves is no different. Paranoid mode binds trust to content, so there
//! an include must be pinned: the including file's hash then covers it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eyre::{Result, WrapErr, bail};

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{Settings, SettingsExt, is_global_config};
use crate::file;
use crate::remote_source::RemoteSource;
use crate::task::task_file_providers::{OCI_INCLUDE_PREFIX, TaskFileProvidersBuilder};
use crate::{dirs, hash};

const FRAGMENT_FILE: &str = "mise.toml";
const DEFAULT_TTL: Duration = Duration::from_secs(60 * 60);

/// `cf` with everything it includes merged in, or `cf` itself when it includes
/// nothing. A fragment never becomes a config file of its own: it is part of
/// the file that includes it, so trust, paths and the lockfile are that file's.
pub(crate) async fn apply(cf: Arc<dyn ConfigFile>) -> Result<Arc<dyn ConfigFile>> {
    // Safe mode loads untrusted project config without a trust prompt, and
    // fetching a URL is not something such a config gets to do. Checked before
    // the references are rendered, since a template is code an untrusted
    // config must not get to run either.
    if Settings::safe_mode() && !is_global_config(cf.get_path()) {
        return Ok(cf);
    }
    let references = cf.remote_includes()?;
    if references.is_empty() {
        return Ok(cf);
    }
    let mut fragments = Vec::with_capacity(references.len());
    for reference in references {
        let fragment = load(cf.get_path(), &reference)
            .await
            .wrap_err_with(|| format!("failed to load config include {reference}"))?;
        fragments.push(fragment);
    }
    cf.with_remote_fragments(fragments)
        .wrap_err("invalid config include")
        .map(|merged| merged.unwrap_or(cf))
}

/// The cached copy of a fragment, refreshed first when it is due.
async fn load(parent: &Path, reference: &str) -> Result<(PathBuf, String)> {
    let pin = classify(reference)?;
    if pin == Pin::Mutable && Settings::try_get().is_ok_and(|s| s.paranoid) {
        bail!(
            "paranoid mode binds trust to content, so a config include must be pinned to a \
             commit sha or an OCI digest: {reference}"
        );
    }
    // per including file, so each one has a file of its own to be keyed by
    let key = hash::hash_sha256_to_str(&format!("{}\n{reference}", parent.display()));
    let cache = dirs::CACHE
        .join("config-includes")
        .join(format!("{key}.toml"));
    let age = file::modified_duration(&cache).ok();
    let ttl = ttl();
    debug!("config include {reference}: {pin:?}, cached {age:?} ago, ttl {ttl:?}");
    if !is_due(pin, age, ttl) {
        return Ok((cache.clone(), file::read_to_string(&cache)?));
    }
    file::create_dir_all(cache.parent().unwrap())?;
    // A body is only cached once it is known to load, so a bad edit upstream
    // can never replace a copy that works.
    let fetched = match fetch(reference).await {
        Ok(body) => MiseToml::parse_remote_fragment(&body, parent)
            .map(|_| body)
            .wrap_err_with(|| format!("invalid config include {reference}")),
        Err(err) => Err(err),
    };
    let body = match fetched {
        Ok(body) => {
            file::write_atomic(&cache, &body)?;
            body
        }
        // An unreachable or broken remote must not break every prompt: keep
        // the copy that works, and rewrite it so the next attempt waits a
        // full ttl.
        Err(err) if age.is_some() => {
            warn!("using the cached config include {reference}: {err:#}");
            let body = file::read_to_string(&cache)?;
            file::write_atomic(&cache, &body)?;
            body
        }
        Err(err) => return Err(err),
    };
    Ok((cache, body))
}

/// `None` is offline mode, where a cached fragment is always used.
fn ttl() -> Option<Duration> {
    match Settings::try_get() {
        Ok(settings) => settings.fetch_remote_versions_cache(),
        Err(_) => Some(DEFAULT_TTL),
    }
}

fn is_due(pin: Pin, age: Option<Duration>, ttl: Option<Duration>) -> bool {
    match (age, pin, ttl) {
        (None, _, _) => true,
        (Some(_), Pin::Immutable, _) | (Some(_), Pin::Mutable, None) => false,
        (Some(age), Pin::Mutable, Some(ttl)) => age >= ttl,
    }
}

async fn fetch(reference: &str) -> Result<String> {
    // The providers' own cache is bypassed: it is keyed by reference and never
    // refreshed. The artifact is temporary and removed when it is dropped.
    let providers = TaskFileProvidersBuilder::new().with_cache(false).build();
    let Some(provider) = providers.get_provider(reference) else {
        bail!("unsupported config include: {reference}");
    };
    let artifact = provider.get_local_artifact(reference).await?;
    let path = if reference.starts_with(OCI_INCLUDE_PREFIX) {
        artifact.path.join(FRAGMENT_FILE)
    } else {
        artifact.path.clone()
    };
    read_fragment(&path)
}

fn read_fragment(path: &Path) -> Result<String> {
    if path.symlink_metadata()?.file_type().is_symlink() {
        bail!(
            "a config include must be a regular file: {}",
            path.display()
        );
    }
    file::read_to_string(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    /// A commit sha or digest: the content can never change.
    Immutable,
    /// A branch, tag or default branch: the content can move.
    Mutable,
}

fn classify(reference: &str) -> Result<Pin> {
    if let Some(oci) = reference.strip_prefix(OCI_INCLUDE_PREFIX) {
        let is_digest = oci
            .rsplit_once("@sha256:")
            .is_some_and(|(_, digest)| is_hex(digest, &[64]));
        return Ok(if is_digest {
            Pin::Immutable
        } else {
            Pin::Mutable
        });
    }
    if let Some(git) = RemoteSource::parse_git(reference) {
        if !git.path.ends_with(".toml") {
            bail!("a git:: config include must point at a .toml file: {reference}");
        }
        let is_sha = git.git_ref.as_deref().is_some_and(|r| is_hex(r, &[40, 64]));
        return Ok(if is_sha { Pin::Immutable } else { Pin::Mutable });
    }
    bail!("a config include must be a git:: URL or an oci:: reference: {reference}")
}

fn is_hex(s: &str, lens: &[usize]) -> bool {
    lens.contains(&s.len()) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn a_sha_or_digest_is_immutable() {
        let git = format!("git::https://github.com/org/repo.git//base/mise.toml?ref={SHA}");
        assert_eq!(classify(&git).unwrap(), Pin::Immutable);
        let ssh = format!("git::ssh://git@github.com/org/repo.git//mise.toml?ref={SHA}");
        assert_eq!(classify(&ssh).unwrap(), Pin::Immutable);
        let oci = format!("oci::ghcr.io/org/base@sha256:{DIGEST}");
        assert_eq!(classify(&oci).unwrap(), Pin::Immutable);
        let tagged = format!("oci::ghcr.io/org/base:1.0@sha256:{DIGEST}");
        assert_eq!(classify(&tagged).unwrap(), Pin::Immutable);
    }

    #[test]
    fn a_branch_tag_or_missing_ref_is_mutable() {
        for reference in [
            "git::https://github.com/org/repo.git//mise.toml?ref=main".to_string(),
            "git::https://github.com/org/repo.git//mise.toml?ref=v1.0.0".to_string(),
            "git::https://github.com/org/repo.git//mise.toml".to_string(),
            // an abbreviated sha can become ambiguous, so it is not a pin
            "git::https://github.com/org/repo.git//mise.toml?ref=0123abc".to_string(),
            "oci::ghcr.io/org/base:1.0".to_string(),
            "oci::ghcr.io/org/base".to_string(),
            "oci::ghcr.io/org/base@sha256:abc".to_string(),
        ] {
            assert_eq!(classify(&reference).unwrap(), Pin::Mutable, "{reference}");
        }
    }

    #[test]
    fn rejects_other_sources_and_directories() {
        assert!(classify("https://example.com/mise.toml").is_err());
        assert!(classify("./local.toml").is_err());
        let dir = format!("git::https://github.com/org/repo.git//base?ref={SHA}");
        assert!(classify(&dir).is_err());
    }

    #[test]
    fn refresh_policy() {
        let hour = Some(Duration::from_secs(3600));
        let young = Some(Duration::from_secs(10));
        let old = Some(Duration::from_secs(7200));
        // nothing cached yet
        assert!(is_due(Pin::Immutable, None, hour));
        assert!(is_due(Pin::Mutable, None, None));
        // a pin is never refreshed
        assert!(!is_due(Pin::Immutable, old, hour));
        // a mutable reference is refreshed once older than the ttl
        assert!(!is_due(Pin::Mutable, young, hour));
        assert!(is_due(Pin::Mutable, old, hour));
        // offline mode keeps whatever is cached
        assert!(!is_due(Pin::Mutable, old, None));
    }
}
