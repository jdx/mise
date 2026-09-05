//! The packslip backend: a release described by its vendor's signed
//! manifest, a [packslip](https://packslip.dev).
//!
//! `packslip:github.com/owner/repo` (or `packslip:owner/repo`) reads the
//! repository's releases, keeps the ones carrying a packslip, and verifies
//! the bundle against a workflow of that repository before trusting a byte
//! of it. `packslip:tool.example.com` reads the project's signed release
//! list at its well-known URL and needs a `pubkey` (or `identity` and
//! `issuer`) tool option to pin. Either way the packslip says which
//! artifact fits this host, what its digest and size are, and which
//! executables it holds, so nothing is guessed from file names.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{Result, WrapErr, bail, eyre};
use itertools::Itertools;
use packslip::model::{
    Artifact, Host, ReleaseListStatement, ReleaseRef, Selection, Statement, is_bare_format,
    repository, repository_subpath, tag_version,
};
use packslip::sigstore::{Policy, Trust};
use reqwest::header::HeaderMap;

use crate::backend::options::VersionOrder;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{ArchiveLayout, install_artifact};
use crate::backend::{
    Backend, BackendType, MISE_BINS_DIR, SecurityFeature, VersionInfo,
    runtime_path_for_install_path,
};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::file;
use crate::github;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::packslip_pins::{self, Observed};
use crate::platform::Platform;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions};

/// The format is a draft and few projects publish one yet, so the backend
/// is behind the experimental setting until both settle.
pub(crate) const EXPERIMENTAL: bool = true;

/// The verified statement, kept beside the install so the rest of mise can
/// read what the release declared without verifying it again.
pub(crate) const STATEMENT_FILE: &str = ".mise-packslip.json";

/// Archive formats mise can unpack, best first. Installers (`deb`, `dmg`,
/// `msi`, ...) are not among them: mise installs into its own directory.
const FORMAT_PREFERENCE: [&str; 13] = [
    "tar.xz", "tar.zst", "tar.gz", "tgz", "tar.bz2", "tar", "zip", "7z", "xz", "zst", "gz", "bz2",
    "raw",
];

#[derive(Debug)]
pub(crate) struct PackslipBackend {
    ba: Arc<BackendArg>,
}

struct PackslipOptions<'a> {
    raw: &'a ToolVersionOptions,
}

impl<'a> PackslipOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self { raw }
    }

    /// Which build to take when the release has several for this platform.
    fn variant(&self) -> Option<String> {
        self.raw.get_string("variant")
    }

    /// A minisign-format public key line, or the path of a `.pub` file.
    fn pubkey(&self) -> Option<String> {
        self.raw.get_string("pubkey")
    }

    fn identity(&self) -> Option<String> {
        self.raw.get_string("identity")
    }

    fn identity_prefix(&self) -> Option<String> {
        self.raw.get_string("identity_prefix")
    }

    fn issuer(&self) -> Option<String> {
        self.raw.get_string("issuer")
    }

    /// Accept a key-signed bundle with no transparency log entry.
    fn allow_unlogged(&self) -> bool {
        matches!(self.raw.get("allow_unlogged"), Some("true"))
    }

    /// `vendor` takes the vendor's own manifest with no stamp from the
    /// hosts `packslip.stampers` names.
    fn trust(&self) -> Option<String> {
        self.raw.get_string("trust")
    }
}

pub(crate) fn install_time_option_keys() -> Vec<String> {
    [
        "variant",
        "pubkey",
        "identity",
        "identity_prefix",
        "issuer",
        "allow_unlogged",
        "ignore_requirements",
        "trust",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The packslip project name behind a tool name: `github.com/owner/repo`
/// as given, `owner/repo` with github.com implied, or a vendor's host.
pub(crate) fn project_name(tool_name: &str) -> Result<String> {
    let name = tool_name.trim_matches('/');
    let first = name.split('/').next().unwrap_or_default();
    let project = if first.contains('.') {
        name.to_string()
    } else {
        format!("github.com/{name}")
    };
    let forge_without_repo =
        packslip::model::project_host(&project) == "github.com" && repository(&project).is_none();
    if !packslip::model::valid_project(&project) || forge_without_repo {
        bail!(
            "packslip:{tool_name} is not a project name; use github.com/owner/repo, owner/repo, or a host such as tool.example.com"
        );
    }
    Ok(project)
}

/// The release asset a project's packslip is published as.
fn bundle_name(project: &str) -> String {
    match repository_subpath(project) {
        Some(sub) => format!("packslip.{}.sigstore.json", sub.replace('/', "-")),
        None => "packslip.sigstore.json".to_string(),
    }
}

/// The signed release list of a project on its own domain.
fn well_known_url(project: &str) -> String {
    match project.split_once('/') {
        Some((host, path)) => format!("https://{host}/.well-known/packslip/{path}.json"),
        None => format!("https://{project}/.well-known/packslip.json"),
    }
}

/// A listed version, when it is what a packslip requires: semver, whose
/// prerelease part says whether it is a prerelease. A tag that is not
/// semver is skipped, since no packslip can carry it.
fn version_info(
    version: Option<String>,
    created_at: Option<String>,
    release_url: Option<String>,
) -> Option<VersionInfo> {
    let version = version?;
    let parsed = match packslip::model::parse_version(&version) {
        Ok(parsed) => parsed,
        Err(_) => {
            debug!("skipping {version}: not semver, which a packslip requires");
            return None;
        }
    };
    Some(VersionInfo {
        version,
        created_at,
        release_url,
        prerelease: Some(!parsed.pre.is_empty()),
        ..Default::default()
    })
}

/// The versions to try for `latest`, newest first. `order` is the backend's
/// own, never a guess: a packslip version is semver because the
/// specification says so, and [`version_info`] has already dropped every
/// tag that is not. A recommendation changes only which candidate is tried
/// first, not that order.
fn latest_candidates(
    versions: Vec<VersionInfo>,
    preferred: Option<&str>,
    prereleases: bool,
    order: VersionOrder,
) -> Vec<String> {
    let versions = versions
        .into_iter()
        .filter(|v| prereleases || v.prerelease != Some(true))
        .map(|v| v.version)
        .collect();
    let mut versions = order.order(versions);
    versions.reverse();
    if let Some(index) = preferred.and_then(|p| versions.iter().position(|v| v == p)) {
        let preferred = versions.remove(index);
        versions.insert(0, preferred);
    }
    versions
}

/// Policy exclusions can fall back; integrity errors stop resolution.
async fn first_eligible<F, Fut>(candidates: Vec<String>, mut check: F) -> Result<Option<String>>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<bool>>,
{
    for version in candidates {
        if check(version.clone()).await? {
            return Ok(Some(version));
        }
    }
    Ok(None)
}

fn github_recommendation(
    project: &str,
    release: &github::GithubRelease,
    list: Option<&ReleaseListStatement>,
) -> Option<String> {
    if release.draft {
        return None;
    }
    if let Some(entry) = list.and_then(|l| {
        l.predicate
            .releases
            .iter()
            .find(|r| r.tag.as_deref() == Some(&release.tag_name))
    }) {
        return Some(entry.version.clone());
    }
    if !release
        .assets
        .iter()
        .any(|a| a.name == bundle_name(project))
    {
        return None;
    }
    tag_version(&release.tag_name, project)
}

/// This host in the packslip's vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostPlatform {
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) libc: Option<String>,
}

impl HostPlatform {
    pub(crate) fn current() -> Self {
        Self::from_platform(&Platform::current())
    }

    fn as_host(&self) -> Host<'_> {
        Host {
            os: &self.os,
            arch: &self.arch,
            libc: self.libc.as_deref(),
        }
    }

    pub(crate) fn from_platform(platform: &Platform) -> Self {
        let os = match platform.os.as_str() {
            "macos" => "darwin",
            other => other,
        };
        let arch = match platform.arch.as_str() {
            "x64" => "x86_64",
            "arm64" => "aarch64",
            "x86" => "i686",
            "arm" => "armv7",
            other => other,
        };
        let libc = (os == "linux").then(|| platform.libc().unwrap_or("gnu").to_string());
        Self {
            os: os.to_string(),
            arch: arch.to_string(),
            libc,
        }
    }
}

fn describe(artifact: &Artifact) -> String {
    let platform = [
        artifact.os.as_deref(),
        artifact.arch.as_deref(),
        artifact.libc.as_deref(),
        artifact.format.as_deref(),
    ]
    .into_iter()
    .flatten()
    .join("/");
    match &artifact.variant {
        Some(variant) => format!("{} ({platform}@{variant})", artifact.name),
        None => format!("{} ({platform})", artifact.name),
    }
}

/// The one artifact for this host, by the crate's rule: an artifact
/// with no `os`, `arch`, or `libc` fits any host, the most specific match
/// wins, then mise's format preference decides, and two that still tie
/// are refused. A gnu host that finds nothing takes a musl build, which
/// is static, and says so.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct NoHostArtifact(String);

pub(crate) fn select_artifact<'a>(
    artifacts: &'a [Artifact],
    host: &HostPlatform,
    variant: Option<&str>,
) -> Result<&'a Artifact> {
    let strict = host.as_host();
    let variants = artifacts
        .iter()
        .filter_map(|a| a.variant.as_deref())
        .unique()
        .join(", ");
    let hint = match variant {
        Some(v) => format!(" with variant {v:?}"),
        None if !variants.is_empty() => {
            format!("; set `variant` to one of {variants} if one of those is meant for you")
        }
        None => String::new(),
    };
    match packslip::select_artifact(artifacts, &strict, variant, &FORMAT_PREFERENCE) {
        Ok(artifact) => return Ok(artifact),
        Err(Selection::Ambiguous(a, b)) => bail!(
            "the packslip lists {a} and {b} for this host and mise will not guess between them{hint}"
        ),
        Err(Selection::NoMatch) => {}
    }
    if host.libc.as_deref() == Some("gnu") {
        let musl = Host {
            libc: Some("musl"),
            ..strict
        };
        match packslip::select_artifact(artifacts, &musl, variant, &FORMAT_PREFERENCE) {
            Ok(artifact) => {
                debug!(
                    "no gnu build fits this host; taking the musl build {}, which is static",
                    artifact.name
                );
                return Ok(artifact);
            }
            Err(Selection::Ambiguous(a, b)) => bail!(
                "the packslip lists {a} and {b} for this host and mise will not guess between them{hint}"
            ),
            Err(Selection::NoMatch) => {}
        }
    }
    let available = artifacts.iter().map(describe).join(", ");
    Err(NoHostArtifact(format!(
        "no artifact for {}/{}{}{hint}. The release has: {available}",
        host.os,
        host.arch,
        host.libc
            .as_deref()
            .map(|l| format!("/{l}"))
            .unwrap_or_default(),
    ))
    .into())
}

/// The artifact this host would select from a stored statement, for
/// scoping resources to it later. `None` when nothing fits, in which case
/// only unscoped resources apply.
pub(crate) fn selected_artifact(statement: &Statement, variant: Option<&str>) -> Option<Artifact> {
    select_artifact(
        &statement.predicate.artifacts,
        &HostPlatform::current(),
        variant,
    )
    .ok()
    .cloned()
}

/// A path from a packslip that may be joined onto a directory of mise's:
/// relative, slash-separated, every segment a plain file name. A verified
/// manifest is still the vendor's data, not mise's.
pub(crate) fn is_safe_relative(rel: &str) -> bool {
    !rel.is_empty() && rel.split('/').all(file::is_plain_file_name)
}

/// A path the packslip gives relative to the archive root, or, when mise
/// stripped a lone top-level directory on extraction, the same path without
/// its first component. `..` and absolute paths never resolve.
pub(crate) fn locate_in_install(install_path: &Path, rel: &str) -> Option<PathBuf> {
    locate_entry(install_path, rel, Path::is_file)
}

/// The same for a directory, such as a skill.
pub(crate) fn locate_dir_in_install(install_path: &Path, rel: &str) -> Option<PathBuf> {
    locate_entry(install_path, rel, Path::is_dir)
}

fn locate_entry(install_path: &Path, rel: &str, wanted: fn(&Path) -> bool) -> Option<PathBuf> {
    if !is_safe_relative(rel) {
        return None;
    }
    let exact = install_path.join(rel);
    if wanted(&exact) {
        return Some(exact);
    }
    rel.split_once('/')
        .map(|(_, rest)| install_path.join(rest))
        .filter(|p| wanted(p))
}

/// What the consumer pinned: the forge identity a name implies, or the key
/// or identity given in the tool options.
pub(crate) enum Pin {
    Identity(Policy),
    Key(packslip::minisign::PublicKey),
}

fn pin(project: &str, opts: &PackslipOptions<'_>) -> Result<Pin> {
    if let Some(pubkey) = opts.pubkey() {
        let text = if Path::new(&pubkey).is_file() {
            file::read_to_string(&pubkey)?
        } else {
            pubkey
        };
        let key = packslip::minisign::PublicKey::parse(&text)
            .map_err(|e| eyre!("packslip:{project}: pubkey: {e}"))?;
        return Ok(Pin::Key(key));
    }
    let explicit = Policy {
        issuer: opts.issuer(),
        identity: opts.identity(),
        identity_prefix: opts.identity_prefix(),
    };
    if !explicit.is_empty() {
        return Ok(Pin::Identity(explicit));
    }
    match Policy::for_project(project) {
        Some(policy) => Ok(Pin::Identity(policy)),
        None => bail!(
            "packslip:{project} is not on a forge mise knows, so nothing pins its signer; set `pubkey`, or `identity` and `issuer`, in its tool options"
        ),
    }
}

impl Pin {
    fn trust(&self) -> Trust<'_> {
        match self {
            Pin::Identity(policy) => Trust::Identity(policy),
            Pin::Key(key) => Trust::Key(key),
        }
    }
}

/// Verify a bundle and, for each artifact path given, its digest and size.
/// Blocks: packslip drives sigstore on a runtime of its own.
fn verify_bundle(
    bundle: &str,
    pin: &Pin,
    require_log: bool,
    artifacts: &[&Path],
) -> Result<packslip::Verified> {
    file::run_blocking(|| {
        let root = packslip::sigstore::trusted_root(None).map_err(|e| eyre!("{e}"))?;
        let options = packslip::Options {
            require_log,
            trusted_root: &root,
        };
        packslip::verify(bundle, &pin.trust(), options, artifacts).map_err(|e| eyre!("{e}"))
    })
}

pub(crate) fn verify_release_list(
    bundle: &str,
    pin: &Pin,
    require_log: bool,
) -> Result<ReleaseListStatement> {
    file::run_blocking(|| {
        let root = packslip::sigstore::trusted_root(None).map_err(|e| eyre!("{e}"))?;
        let options = packslip::Options {
            require_log,
            trusted_root: &root,
        };
        let verified = packslip::verify_release_list(bundle, &pin.trust(), options)
            .map_err(|e| eyre!("{e}"))?;
        if !verified.list.is_current(jiff::Timestamp::now()) {
            bail!(
                "the release list expired at {}; the vendor has not published a fresh one",
                verified.list.predicate.expires_at
            );
        }
        Ok(verified.list)
    })
}

/// The headers a download from GitHub needs; nothing for anywhere else.
/// Listing timestamps only filter candidates. The authenticated log time
/// decides whether a selected release is old enough to install.
fn check_verified_age(
    logged_at: Option<&str>,
    published_at: &str,
    before: Option<jiff::Timestamp>,
) -> Result<()> {
    let Some(before) = before else {
        return Ok(());
    };
    let (time, source) = match logged_at {
        Some(time) => (time, "transparency log"),
        None => (published_at, "unlogged manifest"),
    };
    if !verified_age_allowed(logged_at, published_at, Some(before))? {
        bail!(
            "packslip release was recorded by the {source} at {time}, after the allowed cutoff {before}; refusing to bypass minimum_release_age"
        );
    }
    Ok(())
}

/// A withdrawal in the vendor's signed list is the end of the matter: no
/// stamp, mirror, or cached manifest reinstates the version.
fn refuse_if_withdrawn(project: &str, version: &str, entry: &ReleaseRef) -> Result<()> {
    if entry.is_yanked() {
        bail!(
            "packslip:{project}@{version} was withdrawn by the vendor{}",
            entry
                .status_reason
                .as_deref()
                .map(|r| format!(": {r}"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn verified_age_allowed(
    logged_at: Option<&str>,
    published_at: &str,
    before: Option<jiff::Timestamp>,
) -> Result<bool> {
    let Some(before) = before else {
        return Ok(true);
    };
    let recorded: jiff::Timestamp = logged_at
        .unwrap_or(published_at)
        .parse()
        .wrap_err("invalid verified packslip timestamp")?;
    Ok(recorded <= before)
}

fn headers_for(url: &str) -> Result<HeaderMap> {
    if url.starts_with("https://github.com/") || url.starts_with("https://api.github.com/") {
        github::get_headers(url)
    } else {
        Ok(HeaderMap::new())
    }
}

/// Refuse a list whose sequence is below one already accepted for the
/// project, and remember the highest seen. The crate verifies the list
/// and its expiry; this is the consumer's part, kept with the pins.
fn check_sequence(project: &str, list: &ReleaseListStatement) -> Result<()> {
    crate::packslip_pins::check_sequence(project, list.predicate.sequence)
}

/// Where a release's packslip is and what to send to fetch it.
struct Located {
    url: String,
    headers: HeaderMap,
    /// The digest a signed release list recorded for the bundle, if any.
    digest: Option<String>,
}

impl PackslipBackend {
    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn ensure_experimental(&self) -> Result<()> {
        Settings::get().ensure_experimental("packslip backend")
    }

    fn project(&self) -> Result<String> {
        project_name(&self.ba.tool_name())
    }

    /// The forge repository as `owner/repo`, for a project on github.com.
    fn repo(project: &str) -> Option<String> {
        repository(project).map(|(_, owner, repo)| format!("{owner}/{repo}"))
    }

    async fn release_list(
        &self,
        project: &str,
        pin: &Pin,
        opts: &PackslipOptions<'_>,
    ) -> Result<ReleaseListStatement> {
        let url = well_known_url(project);
        let text = HTTP_FETCH.get_text(&url).await.wrap_err_with(|| {
            format!("fetching the release list of packslip:{project} from {url}")
        })?;
        let list = verify_release_list(&text, pin, !opts.allow_unlogged())
            .wrap_err_with(|| format!("verifying the release list of packslip:{project}"))?;
        if list.predicate.project != project {
            bail!(
                "the release list at {url} is for {}, not {project}",
                list.predicate.project
            );
        }
        check_sequence(project, &list)?;
        Ok(list)
    }

    /// The signed list a github.com repository may keep at `.well-known`
    /// on its default branch, verified against the same identity as its
    /// packslips. `None` when the repository has none, which is the usual
    /// case: a vendor writes one only to withdraw a release, flag a
    /// security fix, or list a release whose tag names no version.
    async fn github_list(
        &self,
        project: &str,
        repo: &str,
        pin: &Pin,
        opts: &PackslipOptions<'_>,
    ) -> Result<Option<ReleaseListStatement>> {
        let path = match repository_subpath(project) {
            Some(sub) => format!(".well-known/packslip/{sub}.json"),
            None => ".well-known/packslip.json".to_string(),
        };
        let url = format!("https://api.github.com/repos/{repo}/contents/{path}?ref=HEAD");
        let headers = github::get_headers(&url)?;
        let text = match HTTP_FETCH
            .get_text_request(&url)
            .headers(&headers)
            .send()
            .await
        {
            Ok(text) => text,
            Err(err) if crate::http::error_code(&err) == Some(404) => {
                packslip_pins::check_missing_list(project)?;
                return Ok(None);
            }
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("fetching the release list of packslip:{project}"));
            }
        };
        let list = verify_release_list(&text, pin, !opts.allow_unlogged())
            .wrap_err_with(|| format!("verifying the release list of packslip:{project}"))?;
        if list.predicate.project != project {
            bail!(
                "the release list in github.com/{repo} is for {}, not {project}",
                list.predicate.project
            );
        }
        check_sequence(project, &list)?;
        Ok(Some(list))
    }

    /// What the vendor themselves say about a version, from the release
    /// list they sign: a withdrawal refuses it outright, and the entry pins
    /// the manifest's digest. `None` only when no signed list covers the
    /// version, which a project served from a GitHub repository is allowed
    /// to do and any other project is not.
    async fn vendor_entry(
        &self,
        project: &str,
        tv: &ToolVersion,
        pin: &Pin,
        opts: &PackslipOptions<'_>,
    ) -> Result<Option<Located>> {
        if let Some(repo) = Self::repo(project) {
            let Some(list) = self.github_list(project, &repo, pin, opts).await? else {
                return Ok(None);
            };
            let Some(entry) = list
                .predicate
                .releases
                .iter()
                .find(|r| r.version == tv.version)
            else {
                return Ok(None);
            };
            refuse_if_withdrawn(project, &tv.version, entry)?;
            return Ok(Some(Located {
                headers: headers_for(&entry.packslip)?,
                url: entry.packslip.clone(),
                digest: list.digest_of(&entry.packslip).map(str::to_string),
            }));
        }
        let list = self.release_list(project, pin, opts).await?;
        let Some(entry) = list
            .predicate
            .releases
            .iter()
            .find(|r| r.version == tv.version)
        else {
            bail!(
                "the release list of packslip:{project} has no version {}",
                tv.version
            );
        };
        refuse_if_withdrawn(project, &tv.version, entry)?;
        Ok(Some(Located {
            url: entry.packslip.clone(),
            headers: HeaderMap::new(),
            digest: list.digest_of(&entry.packslip).map(str::to_string),
        }))
    }

    async fn locate_bundle(
        &self,
        project: &str,
        tv: &ToolVersion,
        pin: &Pin,
        opts: &PackslipOptions<'_>,
    ) -> Result<Located> {
        if let Some(located) = self.vendor_entry(project, tv, pin, opts).await? {
            return Ok(located);
        }
        // No signed list names the manifest, so the release asset is the
        // only place left to find it. Only a GitHub project gets here.
        let asset_name = bundle_name(project);
        let repo = Self::repo(project)
            .ok_or_else(|| eyre!("packslip:{project} publishes no signed release list"))?;
        let releases = github::list_releases_including_prereleases(&repo).await?;
        let found = releases.iter().find_map(|r| {
            let asset = r.assets.iter().find(|a| a.name == asset_name)?;
            (tag_version(&r.tag_name, project).as_deref() == Some(tv.version.as_str()))
                .then_some(asset)
        });
        let Some(asset) = found else {
            bail!(
                "github.com/{repo} has no release {} carrying {asset_name}; mise installs from a packslip and does not guess at release assets",
                tv.version
            );
        };
        Ok(Located {
            headers: github::get_headers(&asset.browser_download_url)?,
            url: asset.browser_download_url.clone(),
            digest: None,
        })
    }

    async fn recommendation(
        &self,
        project: &str,
        opts: &PackslipOptions<'_>,
        pin: &Pin,
    ) -> Result<Option<String>> {
        let Some(repo) = Self::repo(project) else {
            return Ok(self
                .release_list(project, pin, opts)
                .await?
                .predicate
                .latest);
        };
        let list = self.github_list(project, &repo, pin, opts).await?;
        if let Some(latest) = list.as_ref().and_then(|l| l.predicate.latest.as_ref()) {
            return Ok(Some(latest.clone()));
        }
        let release = match github::get_release_with_versions_host(&repo, "latest", false).await {
            Ok(release) => release,
            Err(err) if crate::http::error_code(&err) == Some(404) => return Ok(None),
            Err(err) => return Err(err),
        };
        Ok(github_recommendation(project, &release, list.as_ref()))
    }

    /// Verification failures propagate; only policy exclusions return a reason.
    async fn candidate_exclusion(
        &self,
        project: &str,
        version: &str,
        opts: &PackslipOptions<'_>,
        pin: &Pin,
        before: Option<jiff::Timestamp>,
        stamps: Option<&crate::packslip_stamps::Stamps>,
    ) -> Result<Option<String>> {
        use sha2::{Digest, Sha256};
        let request = ToolRequest::new_with_options(
            self.ba.clone(),
            version,
            opts.raw.clone(),
            crate::toolset::ToolSource::Unknown,
        )?;
        let tv = ToolVersion::new(request, version.to_string());
        let stamp = match stamps {
            Some(stamps) => match stamps.stamp(version) {
                Some(stamp) => Some(stamp),
                None => return Ok(Some(stamps.refusal(project, version).to_string())),
            },
            None => None,
        };
        // The same reach `install` makes, and for the same reason: with a
        // stamp in hand the manifest is already named, so the vendor is asked
        // only for what the vendor decides. Going through `locate_bundle`
        // would demand the original release asset too, and refuse a version
        // `install` accepts.
        let (url, vendor_digest) = match stamp {
            Some(stamp) => (
                stamp.entry.packslip.clone(),
                self.vendor_entry(project, &tv, pin, opts)
                    .await?
                    .and_then(|vendor| vendor.digest),
            ),
            None => {
                let vendor = self.locate_bundle(project, &tv, pin, opts).await?;
                (vendor.url, vendor.digest)
            }
        };
        let url = url.as_str();
        let text = HTTP_FETCH
            .get_text_request(url)
            .headers(&headers_for(url)?)
            .send()
            .await?;
        let actual = hex::encode(Sha256::digest(text.as_bytes()));
        for expected in vendor_digest
            .iter()
            .chain(stamp.and_then(|s| s.digest.as_ref()))
        {
            if &actual != expected {
                bail!("packslip:{project}@{version}: manifest digest differs from signed list");
            }
        }
        let verified = verify_bundle(&text, pin, !opts.allow_unlogged(), &[])?;
        if verified.project != project || verified.version != version {
            bail!(
                "packslip:{project}@{version}: verified manifest project/version differs from discovery"
            );
        }
        let scheme = verified.scheme.to_string();
        let attested_by = verified.attested_by.to_string();
        packslip_pins::check(
            project,
            Observed {
                scheme: &scheme,
                key_id: &verified.key_id,
                issuer: verified.issuer.as_deref(),
                attested_by: &attested_by,
                provenance: verified.provenance_linked,
                logged: verified.logged_at.is_some(),
            },
        )?;
        // Parse errors are verification errors, not age-policy exclusions.
        if !verified_age_allowed(
            verified.logged_at.as_deref(),
            &verified.published_at,
            before,
        )? {
            return Ok(Some(
                "verified release time is after the allowed cutoff".into(),
            ));
        }
        let payload = packslip::sigstore::peek_statement(&text).map_err(|e| eyre!("{e}"))?;
        let statement: Statement = serde_json::from_slice(&payload)?;
        let artifact = match select_artifact(
            &statement.predicate.artifacts,
            &HostPlatform::current(),
            opts.variant().as_deref(),
        ) {
            Ok(artifact) => artifact,
            Err(err) if err.is::<NoHostArtifact>() => return Ok(Some(err.to_string())),
            Err(err) => return Err(err),
        };
        let requirements = crate::packslip_requirements::check(artifact, &BTreeMap::new()).await;
        if !requirements.failures.is_empty() && opts.raw.get("ignore_requirements") != Some("true")
        {
            return Ok(Some(requirements.failures.join("; ")));
        }
        Ok(None)
    }

    async fn policy_versions(&self, raw_opts: &ToolVersionOptions) -> Result<Vec<VersionInfo>> {
        let mut versions = self.vendor_versions(raw_opts).await?;
        // Only what a trusted stamper lists is released, as far as mise is
        // concerned; the rest is not offered.
        let project = self.project()?;
        if let Some(stamps) = crate::packslip_stamps::fetch(&project, raw_opts).await? {
            let before = versions.len();
            versions.retain(|v| stamps.allows(&v.version));
            debug!(
                "packslip:{project}: {} of {before} version(s) carry a stamp",
                versions.len()
            );
        }
        Ok(versions)
    }

    /// Put every executable the packslip names into the install's bin dir
    /// under the name it should have on PATH.
    fn link_bins(tv: &ToolVersion, artifact: &Artifact) -> Result<()> {
        if artifact.bin.is_empty() {
            return Ok(());
        }
        let install_path = tv.install_path();
        let bins_dir = install_path.join(MISE_BINS_DIR);
        file::create_dir_all(&bins_dir)?;
        for bin in &artifact.bin {
            if !file::is_plain_file_name(&bin.name) {
                bail!(
                    "the packslip names an executable {:?}, which is not a plain file name",
                    bin.name
                );
            }
            let Some(src) = locate_in_install(&install_path, &bin.path) else {
                bail!(
                    "the packslip lists executable {} in {}, but the archive holds no such file",
                    bin.path,
                    artifact.name
                );
            };
            file::make_executable(&src)?;
            let dst = bins_dir.join(&bin.name);
            if dst.exists() || dst.is_symlink() {
                file::remove_all(&dst)?;
            }
            file::make_symlink_or_copy(&src, &dst)?;
        }
        Ok(())
    }

    /// Every version the vendor published, before stamps are applied.
    async fn vendor_versions(&self, raw_opts: &ToolVersionOptions) -> Result<Vec<VersionInfo>> {
        self.ensure_experimental()?;
        let project = self.project()?;
        let opts = PackslipOptions::new(raw_opts);
        let pin = pin(&project, &opts)?;
        if let Some(repo) = Self::repo(&project) {
            let asset_name = bundle_name(&project);
            // GitHub's prerelease flag is not consulted: the version says.
            let mut versions: Vec<VersionInfo> = github::list_releases_including_prereleases(&repo)
                .await?
                .into_iter()
                .filter(|r| r.assets.iter().any(|a| a.name == asset_name))
                .filter_map(|r| {
                    version_info(
                        tag_version(&r.tag_name, &project),
                        Some(r.released_at().to_string()),
                        Some(format!(
                            "https://github.com/{repo}/releases/tag/{}",
                            r.tag_name
                        )),
                    )
                })
                .collect();
            versions.reverse();
            // The repository's own signed list, when it keeps one, is the
            // last word on what it names: a withdrawn release goes, and a
            // release whose tag names no version is added.
            if let Some(list) = self.github_list(&project, &repo, &pin, &opts).await? {
                for entry in &list.predicate.releases {
                    if entry.is_yanked() {
                        versions.retain(|v| v.version != entry.version);
                    } else if !versions.iter().any(|v| v.version == entry.version)
                        && let Some(info) = version_info(
                            Some(entry.version.clone()),
                            Some(entry.published_at.clone()),
                            None,
                        )
                    {
                        versions.push(info);
                    }
                }
            }
            return Ok(versions);
        }
        let list = self.release_list(&project, &pin, &opts).await?;
        let mut versions: Vec<VersionInfo> = list
            .predicate
            .releases
            .iter()
            .filter(|r| !r.is_yanked())
            .filter_map(|r| {
                version_info(Some(r.version.clone()), Some(r.published_at.clone()), None)
            })
            .collect();
        versions.reverse();
        Ok(versions)
    }
}

#[async_trait]
impl Backend for PackslipBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Packslip
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<SecurityFeature> {
        vec![SecurityFeature::Packslip]
    }

    /// A packslip version is semver, and the specification ranks releases
    /// by semver precedence, never by the order a release list gives.
    fn version_order(&self, _opts: &ToolVersionOptions) -> Result<VersionOrder> {
        Ok(VersionOrder::Semver)
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &[
            "pubkey",
            "identity",
            "identity_prefix",
            "issuer",
            "allow_unlogged",
            "trust",
        ]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        self.policy_versions(&opts).await
    }

    async fn list_remote_versions_with_info_and_options(
        &self,
        config: &Arc<Config>,
        listing_opts: &ToolVersionOptions,
        selection_opts: &ToolVersionOptions,
        _refresh: bool,
        has_local_version_listing_override: bool,
    ) -> Result<Vec<VersionInfo>> {
        // A cached accepted-version set cannot reflect changed stamper trust,
        // new withdrawals, expired lists, or a list that disappeared. Offline
        // it is still all there is: rechecking policy means asking GitHub and
        // every stamper, which listing may not do, so serve the cache the way
        // every other backend does. Installing rechecks regardless.
        let versions = if Settings::get().offline() {
            let cache = self
                .remote_version_cache_for(config, listing_opts, has_local_version_listing_override)
                .await?;
            let cache = cache.lock().await;
            crate::backend::cached_remote_versions_offline(&self.ba, &cache)
        } else {
            self.policy_versions(selection_opts).await?
        };
        Ok(versions
            .into_iter()
            .filter(|v| self.include_prereleases(selection_opts) || v.prerelease != Some(true))
            .collect())
    }

    async fn latest_version_with_selection_options(
        &self,
        config: &Arc<Config>,
        query: Option<String>,
        selection_opts: &ToolVersionOptions,
        before_date: Option<jiff::Timestamp>,
        refresh: bool,
    ) -> Result<Option<String>> {
        self.ensure_experimental()?;
        let before =
            crate::backend::effective_latest_before_date(self, selection_opts, before_date)?;
        let query = query.as_deref().unwrap_or("latest");
        if query != "latest" {
            return self
                .latest_version_for_query_with_selection_options(
                    config,
                    query,
                    selection_opts,
                    before,
                    refresh,
                )
                .await;
        }
        if Settings::get().offline() {
            // Policy lives on the network — the vendor's list, every stamper's,
            // and the manifest itself — so offline there is nothing to consult
            // and nothing to recommend. Take the newest the cache knows of;
            // installing it will recheck policy, or fail for want of a network.
            let versions = self
                .list_remote_versions_with_info_with_selection_options(
                    config,
                    selection_opts,
                    refresh,
                )
                .await?;
            let candidates = latest_candidates(
                versions,
                None,
                self.include_prereleases(selection_opts),
                self.version_order(selection_opts)?,
            );
            return Ok(candidates.into_iter().next());
        }
        // Read policy directly: a cached version list must not hide new yanks,
        // changed stampers, or a missing previously accepted signed list.
        let project = self.project()?;
        let opts = PackslipOptions::new(selection_opts);
        let pin = pin(&project, &opts)?;
        let recommendation = self.recommendation(&project, &opts, &pin).await?;
        let versions = self.vendor_versions(selection_opts).await?;
        let stamps = crate::packslip_stamps::fetch(&project, selection_opts).await?;
        let candidates = latest_candidates(
            versions,
            recommendation.as_deref(),
            self.include_prereleases(selection_opts),
            self.version_order(selection_opts)?,
        );
        if let Some(preferred) = &recommendation
            && !candidates.iter().any(|v| v == preferred)
        {
            warn!(
                "packslip:{project}: skipping recommended {preferred}: absent, withdrawn, or excluded prerelease"
            );
        }
        first_eligible(candidates, |version| {
            let project = &project;
            let opts = &opts;
            let pin = &pin;
            let stamps = stamps.as_ref();
            async move {
                if let Some(reason) = self
                    .candidate_exclusion(project, &version, opts, pin, before, stamps)
                    .await?
                {
                    warn!("packslip:{project}: skipping {version}: {reason}");
                    return Ok(false);
                }
                Ok(true)
            }
        })
        .await
    }

    async fn install_operation_count(&self, _tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        4
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        self.ensure_experimental()?;
        let project = self.project()?;
        let raw_opts = tv.request.options();
        let opts = PackslipOptions::new(&raw_opts);
        let pin = pin(&project, &opts)?;
        let require_log = !opts.allow_unlogged();

        // A stamp first, when the settings ask for one: it says this exact
        // manifest is admitted, and points at it. Then the manifest: nothing
        // else is downloaded until it verifies.
        let stamp = match crate::packslip_stamps::fetch(&project, &raw_opts).await? {
            Some(stamps) => Some(
                stamps
                    .stamp(&tv.version)
                    .ok_or_else(|| stamps.refusal(&project, &tv.version))?
                    .clone(),
            ),
            None => None,
        };
        // The vendor stays authoritative for withdrawals and digest pins even
        // when a stamper supplies a mirror URL. What the vendor's list cannot
        // speak to is whether the original release asset is still on GitHub,
        // and a stamp already names the manifest and its digest — so mise asks
        // the signed list here rather than `locate_bundle`, and a deleted asset
        // does not veto a release the vendor never withdrew.
        let (located, vendor_digest) = match &stamp {
            Some(stamp) => {
                debug!(
                    "{}: stamped by {}, manifest at {}",
                    tv.style(),
                    stamp.host,
                    stamp.entry.packslip
                );
                // A stamp says a host reviewed this manifest, and the digest is
                // the whole of what ties the claim to a file. Without one the
                // stamp admits a URL, and any later manifest the vendor signs
                // for this version can stand at it in place of the reviewed
                // one — so refuse rather than call that a review.
                let Some(digest) = stamp.digest.clone() else {
                    bail!(
                        "the stamp for packslip:{project}@{} from {} records no sha256 for {}, so nothing says the manifest is the one that host reviewed",
                        tv.version,
                        stamp.host,
                        stamp.entry.packslip
                    );
                };
                let vendor = self.vendor_entry(&project, &tv, &pin, &opts).await?;
                (
                    Located {
                        headers: headers_for(&stamp.entry.packslip)?,
                        url: stamp.entry.packslip.clone(),
                        digest: Some(digest),
                    },
                    vendor.and_then(|v| v.digest),
                )
            }
            // Without a stamp the located bundle is the vendor's own, so its
            // digest is already the one checked below.
            None => (self.locate_bundle(&project, &tv, &pin, &opts).await?, None),
        };
        let bundle_path = tv.download_path().join(bundle_name(&project));
        file::create_dir_all(tv.download_path())?;
        ctx.pr.set_message("download packslip".into());
        HTTP.download_file_with_headers(
            &located.url,
            &bundle_path,
            &located.headers,
            Some(ctx.pr.as_ref()),
        )
        .await?;
        let pinned: Vec<&String> = located.digest.iter().chain(vendor_digest.iter()).collect();
        if !pinned.is_empty() {
            let (actual, _) = packslip::digest_file(&bundle_path)?;
            for expected in pinned {
                if &actual != expected {
                    bail!(
                        "the packslip at {} is not the one the signed release list points at (sha256 {actual}, list says {expected})",
                        located.url
                    );
                }
            }
        }
        let bundle = file::read_to_string(&bundle_path)?;
        ctx.pr.set_message("verify packslip".into());
        let verified = verify_bundle(&bundle, &pin, require_log, &[])
            .wrap_err_with(|| format!("verifying the packslip of {}", tv.style()))?;
        let payload = packslip::sigstore::peek_statement(&bundle).map_err(|e| eyre!("{e}"))?;
        let statement: Statement = serde_json::from_slice(&payload)?;
        if verified.project != project {
            bail!("the packslip is for {}, not {project}", verified.project);
        }
        if verified.version != tv.version {
            bail!(
                "the packslip says version {}, not {}; the release's tag and its manifest disagree",
                verified.version,
                tv.version
            );
        }
        debug!(
            "{}: packslip signed by {} ({}){}",
            tv.style(),
            verified.key_id,
            verified.scheme,
            verified
                .logged_at
                .as_deref()
                .map(|t| format!(", logged {t}"))
                .unwrap_or_default()
        );
        let before = crate::install_before::resolve_before_date_for_tool(
            &self.ba,
            tv.before_date.or(ctx.before_date),
            raw_opts.minimum_release_age(),
        )?;
        check_verified_age(
            verified.logged_at.as_deref(),
            &verified.published_at,
            before,
        )?;

        // Who signed, against what this machine accepted before, and then
        // against what the project committed to in its lockfile.
        let scheme = verified.scheme.to_string();
        let attested_by = verified.attested_by.to_string();
        // Nothing is recorded until the release is accepted in full, so a
        // refused install leaves no pin behind.
        let observed = Observed {
            scheme: &scheme,
            key_id: &verified.key_id,
            issuer: verified.issuer.as_deref(),
            attested_by: &attested_by,
            provenance: verified.provenance_linked,
            logged: verified.logged_at.is_some(),
        };
        packslip_pins::check(&project, observed)?;
        let signer = format!(
            "{scheme}:{}",
            packslip_pins::signer_of(&scheme, &verified.key_id)
        );
        let platform_key = self.get_platform_key();
        // The signer describes the release, so every platform's lock entry
        // speaks for it, not only this host's.
        for info in tv.lock_platforms.values() {
            if let Some(locked) = &info.signer
                && *locked != signer
            {
                bail!(
                    "mise.lock says {} signed {}, but this release is signed by {signer}; remove the entry from mise.lock to accept the new signer",
                    locked,
                    tv.style()
                );
            }
            if info.attested_by.is_none()
                && info.signer.is_some()
                && verified.attested_by == packslip::Attestor::Repackager
            {
                bail!(
                    "mise.lock says the vendor's own packslip was accepted for {}, but this release is a repackager's; remove the entry from mise.lock to accept that",
                    tv.style()
                );
            }
        }

        // Then the one artifact for this host, by what the manifest says.
        let artifact = select_artifact(
            &statement.predicate.artifacts,
            &HostPlatform::current(),
            opts.variant().as_deref(),
        )?
        .clone();
        let mut commands = BTreeMap::new();
        if let Some(req) = &artifact.requires {
            for bin in &req.bin {
                // Spawnable, not merely present: the probe below runs the
                // path with `--version`, so a shebang-only script or a `.ps1`
                // would be chosen and then fail to start.
                if let Some(path) = ctx.ts.which_bin_spawnable(&ctx.config, &bin.name).await {
                    commands.insert(bin.name.clone(), path);
                }
            }
        }
        crate::packslip_requirements::check(&artifact, &commands)
            .await
            .enforce(raw_opts.get("ignore_requirements") == Some("true"))?;
        let Some(url) = artifact.url.clone() else {
            bail!("the packslip gives no download URL for {}", artifact.name);
        };
        if !file::is_plain_file_name(&artifact.name) {
            bail!(
                "the packslip names an artifact {:?}, which is not a plain file name",
                artifact.name
            );
        }
        let file_path = tv.download_path().join(&artifact.name);
        ctx.pr.next_operation();
        ctx.pr.set_message(format!("download {}", artifact.name));
        HTTP.download_file_with_headers(
            &url,
            &file_path,
            &headers_for(&url)?,
            Some(ctx.pr.as_ref()),
        )
        .await?;

        // The signed digest and size first, then what the lockfile remembers:
        // a lock entry written from an earlier packslip keeps its checksum and
        // is compared, so a newly signed manifest cannot quietly replace what
        // the project committed to. A fresh entry records the signed sha256.
        ctx.pr.next_operation();
        ctx.pr.set_message(format!("verify {}", artifact.name));
        verify_bundle(&bundle, &pin, require_log, &[&file_path])
            .wrap_err_with(|| format!("verifying {} against its packslip", artifact.name))?;
        {
            let info = tv.lock_platforms.entry(platform_key).or_default();
            info.url = Some(url);
            if info.checksum.is_none()
                && let Some(sha256) = statement.digest_of(&artifact.name)
            {
                info.checksum = Some(format!("sha256:{sha256}"));
            }
            info.signer = Some(signer);
            // Set for a repackager's document and cleared for the vendor's,
            // so the lockfile ratchets up the way the pin does.
            info.attested_by = (verified.attested_by == packslip::Attestor::Repackager)
                .then(|| "repackager".to_string());
        }
        self.verify_checksum(ctx, &mut tv, &file_path)?;

        // Unpack as the manifest describes the file, not as its name suggests.
        ctx.pr.next_operation();
        let mut install_opts = tv.request.options();
        if let Some(format) = &artifact.format {
            install_opts
                .insert_option("format".into(), toml::Value::String(format.clone()))
                .map_err(|e| eyre!(e))?;
        }
        // A bare executable, compressed or not, lands at the path its bin
        // entry names: the artifact's own name minus any compression suffix.
        if artifact.format.as_deref().is_some_and(is_bare_format)
            && let Some(bin) = artifact.bin.first()
        {
            install_opts
                .insert_option("bin".into(), toml::Value::String(bin.path.clone()))
                .map_err(|e| eyre!(e))?;
        }
        // The statement names every executable by path, so nothing here may
        // rename one: a tidied platform suffix would put the file somewhere the
        // statement does not point, and `link_bins` would not find it.
        install_artifact(
            &tv,
            &file_path,
            &install_opts,
            ArchiveLayout::Declared,
            Some(ctx.pr.as_ref()),
        )?;
        Self::link_bins(&tv, &artifact)?;
        file::write(
            tv.install_path().join(STATEMENT_FILE),
            serde_json::to_vec_pretty(&statement)?,
        )?;
        // Completions and CLI specs the vendor keeps outside the artifact.
        crate::packslip::fetch_files(&tv, &statement, Some(&artifact), ctx.pr.as_ref()).await?;
        // The pin records what was installed, so a release that failed to
        // unpack or link leaves no mark; the check above is what refuses.
        // A pin that cannot be written must not leave its artifact behind
        // either: `always_keep_install` would preserve an install whose
        // signer was never recorded, and the next release — from any signer
        // at all — would then set the project's first pin with that one
        // still in place.
        if let Err(err) = packslip_pins::record(&project, observed) {
            let _ = file::remove_all(tv.install_path());
            return Err(err);
        }
        Ok(tv)
    }

    /// `variant` decides which artifact is downloaded, so a lock entry for a
    /// variant build is not the entry for the plain one. `trust` is kept so
    /// an install from the lock does not quietly relax to the vendor alone.
    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        let opts = PackslipOptions::new(&raw_opts);
        let mut options = BTreeMap::new();
        if let Some(variant) = opts.variant() {
            options.insert("variant".to_string(), variant);
        }
        if let Some(trust) = opts.trust() {
            options.insert("trust".to_string(), trust);
        }
        Ok(options)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let install_path = tv.install_path();
        if install_path.join(MISE_BINS_DIR).is_dir() {
            return Ok(vec![tv.runtime_path().join(MISE_BINS_DIR)]);
        }
        let bin = install_path.join("bin");
        if bin.is_dir() {
            return Ok(vec![runtime_path_for_install_path(tv, bin)]);
        }
        Ok(vec![tv.runtime_path()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use packslip::model::Bin;

    fn artifact(name: &str, os: &str, arch: &str, libc: Option<&str>, format: &str) -> Artifact {
        Artifact {
            name: name.into(),
            os: Some(os.into()),
            arch: Some(arch.into()),
            libc: libc.map(str::to_string),
            variant: None,
            size: 1,
            url: Some(format!("https://example.com/{name}")),
            format: Some(format.into()),
            bin: vec![Bin::new("tool")],
            requires: None,
            provenance: vec![],
            extensions: Default::default(),
        }
    }

    fn linux() -> HostPlatform {
        HostPlatform {
            os: "linux".into(),
            arch: "x86_64".into(),
            libc: Some("gnu".into()),
        }
    }

    #[tokio::test]
    async fn latest_keeps_recommendation_separate_from_order_and_admission() {
        let versions = || {
            ["2.8.4", "3.0.0", "4.0.0-beta.1"]
                .into_iter()
                .map(|v| version_info(Some(v.into()), None, None).unwrap())
                .collect()
        };
        assert_eq!(
            latest_candidates(versions(), Some("2.8.4"), false, VersionOrder::Semver),
            ["2.8.4", "3.0.0"]
        );
        assert_eq!(
            latest_candidates(versions(), None, false, VersionOrder::Semver),
            ["3.0.0", "2.8.4"]
        );
        assert_eq!(
            latest_candidates(versions(), Some("9.0.0"), false, VersionOrder::Semver),
            ["3.0.0", "2.8.4"]
        );
        assert_eq!(
            latest_candidates(
                versions(),
                Some("4.0.0-beta.1"),
                false,
                VersionOrder::Semver
            ),
            ["3.0.0", "2.8.4"]
        );
        assert_eq!(
            latest_candidates(versions(), Some("2.8.4"), true, VersionOrder::Semver),
            ["2.8.4", "4.0.0-beta.1", "3.0.0"]
        );
        {
            let selected = first_eligible(
                latest_candidates(versions(), Some("2.8.4"), false, VersionOrder::Semver),
                |v| async move { Ok(v != "2.8.4") },
            )
            .await
            .unwrap();
            assert_eq!(selected.as_deref(), Some("3.0.0"));
        }
        let result = first_eligible(
            latest_candidates(versions(), Some("2.8.4"), false, VersionOrder::Semver),
            |_v| async { bail!("signature failure") },
        )
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("signature failure")
        );
        assert_eq!(
            first_eligible(
                latest_candidates(versions(), None, false, VersionOrder::Semver),
                |_v| async { Ok(false) }
            )
            .await
            .unwrap(),
            None
        );
    }

    #[test]
    fn github_latest_is_project_specific_and_supports_list_tag_mappings() {
        let mut release: github::GithubRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "v2.8.4", "draft": false, "prerelease": false,
            "created_at": "2026-01-01T00:00:00Z", "assets": []
        }))
        .unwrap();
        assert_eq!(
            github_recommendation("github.com/o/r/sub", &release, None),
            None
        );
        release.assets.push(serde_json::from_value(serde_json::json!({
            "name": "packslip.sub.sigstore.json", "browser_download_url": "https://x/p.json", "url": "https://api.github.com/assets/1", "size": 1
        })).unwrap());
        assert_eq!(
            github_recommendation("github.com/o/r/sub", &release, None).as_deref(),
            Some("2.8.4")
        );
        release.tag_name = "custom-tag".into();
        let list: ReleaseListStatement = serde_json::from_value(serde_json::json!({
            "_type": "https://in-toto.io/Statement/v1", "subject": [], "predicateType": "https://packslip.dev/releases/v1",
            "predicate": {"project": "github.com/o/r/sub", "generated_at": "2026-01-01T00:00:00Z",
            "expires_at": "2027-01-01T00:00:00Z", "sequence": 1,
            "identity": {"scheme": "sigstore-key", "key_id": "key"},
            "releases": [{"version": "2.8.4", "tag": "custom-tag", "published_at": "2026-01-01T00:00:00Z", "packslip": "https://x/p.json"}]}
        })).unwrap();
        assert_eq!(
            github_recommendation("github.com/o/r/sub", &release, Some(&list)).as_deref(),
            Some("2.8.4")
        );
    }

    #[test]
    fn age_is_checked_against_the_authenticated_log_time() {
        let before = Some("2026-09-03T00:00:00Z".parse().unwrap());
        let old = "2026-09-01T00:00:00Z";
        let new = "2026-09-04T00:00:00Z";
        assert!(check_verified_age(Some(new), old, before).is_err());
        assert!(check_verified_age(Some(old), new, before).is_ok());
        assert!(check_verified_age(None, old, before).is_ok());
        assert!(check_verified_age(None, new, before).is_err());
        assert!(check_verified_age(Some(new), old, None).is_ok());
        assert!(check_verified_age(Some("invalid"), old, before).is_err());
        assert!(check_verified_age(Some("2026-09-03T00:00:00Z"), old, before).is_ok());
    }

    #[test]
    fn project_names() {
        assert_eq!(
            project_name("github.com/jdx/mise").unwrap(),
            "github.com/jdx/mise"
        );
        assert_eq!(project_name("jdx/mise").unwrap(), "github.com/jdx/mise");
        assert_eq!(
            project_name("oxc-project/oxc/oxlint").unwrap(),
            "github.com/oxc-project/oxc/oxlint"
        );
        assert_eq!(project_name("mise.jdx.dev").unwrap(), "mise.jdx.dev");
        assert!(project_name("mise").is_err());
        assert!(project_name("GitHub.com/jdx/mise").is_err());
        assert_eq!(bundle_name("github.com/jdx/mise"), "packslip.sigstore.json");
        assert_eq!(
            bundle_name("github.com/biomejs/biome/crates/cli"),
            "packslip.crates-cli.sigstore.json"
        );
        assert_eq!(
            well_known_url("mise.jdx.dev"),
            "https://mise.jdx.dev/.well-known/packslip.json"
        );
        assert_eq!(
            well_known_url("jdx.dev/mise"),
            "https://jdx.dev/.well-known/packslip/mise.json"
        );
    }

    #[test]
    fn versions_come_from_tags_through_the_crate() {
        assert_eq!(
            tag_version("v1.2.3", "github.com/o/r").as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            tag_version("jq-1.7.1", "github.com/jqlang/jq").as_deref(),
            Some("1.7.1")
        );
        assert_eq!(
            tag_version("oxlint_v1.0.0", "github.com/oxc-project/oxc/oxlint").as_deref(),
            Some("1.0.0")
        );
        assert_eq!(
            tag_version("v4.1", "github.com/o/r").as_deref(),
            Some("4.1.0"),
            "loose spellings are normalized"
        );
        assert_eq!(tag_version("nightly-20260904", "github.com/o/r"), None);
        assert!(version_info(None, None, None).is_none());
    }

    #[test]
    fn host_platforms() {
        let host = HostPlatform::from_platform(&Platform {
            os: "macos".into(),
            arch: "arm64".into(),
            qualifier: None,
        });
        assert_eq!(host.os, "darwin");
        assert_eq!(host.arch, "aarch64");
        assert_eq!(host.libc, None);
        let host = HostPlatform::from_platform(&Platform {
            os: "linux".into(),
            arch: "x64".into(),
            qualifier: Some("musl".into()),
        });
        assert_eq!(host.libc.as_deref(), Some("musl"));
        assert_eq!(linux().libc.as_deref(), Some("gnu"));
        assert_eq!(linux().as_host().libc, Some("gnu"));
    }

    #[test]
    fn selects_the_one_artifact_for_the_host() {
        let artifacts = vec![
            artifact("t-darwin-arm64.tar.xz", "darwin", "aarch64", None, "tar.xz"),
            artifact(
                "t-linux-x64.tar.gz",
                "linux",
                "x86_64",
                Some("gnu"),
                "tar.gz",
            ),
            artifact(
                "t-linux-x64.tar.xz",
                "linux",
                "x86_64",
                Some("gnu"),
                "tar.xz",
            ),
            artifact(
                "t-linux-x64-musl.tar.xz",
                "linux",
                "x86_64",
                Some("musl"),
                "tar.xz",
            ),
            artifact("t-linux-x64.deb", "linux", "x86_64", Some("gnu"), "deb"),
        ];
        let picked = select_artifact(&artifacts, &linux(), None).unwrap();
        assert_eq!(picked.name, "t-linux-x64.tar.xz", "best format wins");
        let musl = HostPlatform {
            libc: Some("musl".into()),
            ..linux()
        };
        assert_eq!(
            select_artifact(&artifacts, &musl, None).unwrap().name,
            "t-linux-x64-musl.tar.xz"
        );
        let mac = HostPlatform {
            os: "darwin".into(),
            arch: "aarch64".into(),
            libc: None,
        };
        assert_eq!(
            select_artifact(&artifacts, &mac, None).unwrap().name,
            "t-darwin-arm64.tar.xz"
        );
        let windows = HostPlatform {
            os: "windows".into(),
            arch: "x86_64".into(),
            libc: None,
        };
        let err = select_artifact(&artifacts, &windows, None).unwrap_err();
        assert!(
            err.to_string().contains("no artifact for windows/x86_64"),
            "{err}"
        );

        // Two artifacts that tie are refused, and a variant picks one.
        let mut fips = artifact(
            "t-fips-linux-x64.tar.xz",
            "linux",
            "x86_64",
            Some("gnu"),
            "tar.xz",
        );
        fips.variant = Some("fips".into());
        let with_variant = vec![artifacts[2].clone(), fips.clone()];
        assert_eq!(
            select_artifact(&with_variant, &linux(), Some("fips"))
                .unwrap()
                .name,
            "t-fips-linux-x64.tar.xz"
        );
        assert_eq!(
            select_artifact(&with_variant, &linux(), None).unwrap().name,
            "t-linux-x64.tar.xz"
        );
        let only_variants = vec![fips];
        let err = select_artifact(&only_variants, &linux(), None).unwrap_err();
        assert!(
            err.to_string().contains("set `variant` to one of fips"),
            "{err}"
        );
        let tie = vec![
            artifacts[2].clone(),
            artifact(
                "t-other-linux-x64.tar.xz",
                "linux",
                "x86_64",
                Some("gnu"),
                "tar.xz",
            ),
        ];
        let err = select_artifact(&tie, &linux(), None).unwrap_err();
        assert!(err.to_string().contains("will not guess"), "{err}");

        // A gnu host with no gnu build takes the static musl build; a
        // host that reports no libc takes only artifacts naming none.
        let musl_only = vec![artifacts[3].clone()];
        assert_eq!(
            select_artifact(&musl_only, &linux(), None).unwrap().name,
            "t-linux-x64-musl.tar.xz"
        );
        let no_libc = HostPlatform {
            libc: None,
            ..linux()
        };
        assert!(select_artifact(&musl_only, &no_libc, None).is_err());

        // A universal or portable artifact fits, and a build for the host
        // beats it; a compressed bare executable is installable.
        let mut universal = artifact("t-darwin.tar.xz", "darwin", "", None, "tar.xz");
        universal.arch = None;
        let mut jar = artifact("t.jar", "", "", None, "zip");
        jar.os = None;
        jar.arch = None;
        let mut bare = artifact("t-linux-x64.gz", "linux", "x86_64", Some("gnu"), "gz");
        bare.bin = vec![Bin::named("t-linux-x64", "t")];
        let mixed = vec![universal, jar.clone(), bare.clone()];
        assert_eq!(
            select_artifact(&mixed, &mac, None).unwrap().name,
            "t-darwin.tar.xz"
        );
        assert_eq!(
            select_artifact(&mixed, &linux(), None).unwrap().name,
            "t-linux-x64.gz"
        );
        assert_eq!(
            select_artifact(&[jar], &windows, None).unwrap().name,
            "t.jar"
        );
        assert_eq!(
            packslip::model::bare_file_name(&bare.name, "gz"),
            "t-linux-x64",
            "what the bin entry's path must be"
        );
    }

    #[test]
    fn an_ambiguous_musl_fallback_is_not_an_ineligible_host() {
        let artifacts = [
            artifact("a.tar.xz", "linux", "x86_64", Some("musl"), "tar.xz"),
            artifact("b.tar.xz", "linux", "x86_64", Some("musl"), "tar.xz"),
        ];
        let err = select_artifact(&artifacts, &linux(), None).unwrap_err();
        assert!(!err.is::<NoHostArtifact>());
        assert!(err.to_string().contains("will not guess"));
    }

    #[test]
    fn locates_bins_with_or_without_a_stripped_top_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/tool"), b"").unwrap();
        std::fs::write(root.join("bare"), b"").unwrap();
        assert_eq!(
            locate_in_install(root, "bin/tool"),
            Some(root.join("bin/tool"))
        );
        assert_eq!(
            locate_in_install(root, "tool-1.0/bin/tool"),
            Some(root.join("bin/tool")),
            "a lone top-level directory was stripped on extraction"
        );
        assert_eq!(locate_in_install(root, "bare"), Some(root.join("bare")));
        assert_eq!(locate_in_install(root, "missing"), None);
        assert_eq!(locate_in_install(root, "../bare"), None);
        assert_eq!(locate_in_install(root, "/etc/passwd"), None);
        assert_eq!(locate_in_install(root, "bin\\tool"), None);
        assert_eq!(locate_in_install(root, "bin/./tool"), None);
        assert!(is_safe_relative("share/zsh/site-functions/_tool"));
        for bad in ["", "/x", "a//b", "a/../b", "a/./b", "a\\b", "..", "."] {
            assert!(!is_safe_relative(bad), "{bad:?}");
        }
        assert_eq!(
            locate_in_install(root, "bin"),
            None,
            "a directory is not a bin"
        );
        assert_eq!(locate_dir_in_install(root, "bin"), Some(root.join("bin")));
        assert_eq!(
            locate_dir_in_install(root, "top/bin"),
            Some(root.join("bin"))
        );
        assert_eq!(
            locate_dir_in_install(root, "bare"),
            None,
            "a file is not a dir"
        );
    }
}
