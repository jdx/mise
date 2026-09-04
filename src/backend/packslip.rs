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
    Artifact, Host, ReleaseListStatement, Selection, Statement, is_bare_format, repository,
    repository_subpath, tag_version,
};
use packslip::sigstore::{Policy, Trust};
use reqwest::header::HeaderMap;

use crate::backend::options::VersionOrder;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::install_artifact;
use crate::backend::{
    Backend, BackendType, MISE_BINS_DIR, SecurityFeature, VersionInfo,
    runtime_path_for_install_path,
};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::dirs;
use crate::file;
use crate::github;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
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
}

pub(crate) fn install_time_option_keys() -> Vec<String> {
    [
        "variant",
        "pubkey",
        "identity",
        "identity_prefix",
        "issuer",
        "allow_unlogged",
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
        if let Ok(artifact) =
            packslip::select_artifact(artifacts, &musl, variant, &FORMAT_PREFERENCE)
        {
            debug!(
                "no gnu build fits this host; taking the musl build {}, which is static",
                artifact.name
            );
            return Ok(artifact);
        }
    }
    let available = artifacts.iter().map(describe).join(", ");
    bail!(
        "no artifact for {}/{}{}{hint}. The release has: {available}",
        host.os,
        host.arch,
        host.libc
            .as_deref()
            .map(|l| format!("/{l}"))
            .unwrap_or_default(),
    )
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
    if !is_safe_relative(rel) {
        return None;
    }
    let exact = install_path.join(rel);
    if exact.is_file() {
        return Some(exact);
    }
    rel.split_once('/')
        .map(|(_, rest)| install_path.join(rest))
        .filter(|p| p.is_file())
}

/// What the consumer pinned: the forge identity a name implies, or the key
/// or identity given in the tool options.
enum Pin {
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

fn verify_release_list(bundle: &str, pin: &Pin, require_log: bool) -> Result<ReleaseListStatement> {
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
fn headers_for(url: &str) -> Result<HeaderMap> {
    if url.starts_with("https://github.com/") || url.starts_with("https://api.github.com/") {
        github::get_headers(url)
    } else {
        Ok(HeaderMap::new())
    }
}

/// Where mise remembers the highest list sequence it accepted per
/// project, so a mirror cannot show it an older list than it has seen.
/// `/` and `%` are escaped so two projects never share a file:
/// `github.com/foo/bar-baz` and `github.com/foo-bar/baz` stay apart.
fn sequence_file(project: &str) -> PathBuf {
    let name = project.replace('%', "%25").replace('/', "%2F");
    dirs::STATE
        .join("packslip")
        .join("sequence")
        .join(format!("{name}.txt"))
}

/// Refuse a list whose sequence is below one already accepted for the
/// project, and remember the highest seen. The crate verifies the list
/// and its expiry; this is the consumer's part.
fn check_sequence(project: &str, list: &ReleaseListStatement) -> Result<()> {
    check_sequence_at(&sequence_file(project), project, list)
}

fn check_sequence_at(path: &Path, project: &str, list: &ReleaseListStatement) -> Result<()> {
    let last: Option<u64> = file::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok());
    let sequence = list.predicate.sequence;
    if let Some(last) = last
        && sequence < last
    {
        bail!(
            "the release list of packslip:{project} has sequence {sequence}, but sequence {last} was already accepted; refusing to go back"
        );
    }
    if last != Some(sequence) {
        if let Some(parent) = path.parent() {
            file::create_dir_all(parent)?;
        }
        file::write_atomic(path, sequence.to_string())?;
    }
    Ok(())
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
            Err(err) if crate::http::error_code(&err) == Some(404) => return Ok(None),
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

    async fn locate_bundle(
        &self,
        project: &str,
        tv: &ToolVersion,
        pin: &Pin,
        opts: &PackslipOptions<'_>,
    ) -> Result<Located> {
        let asset_name = bundle_name(project);
        if let Some(repo) = Self::repo(project) {
            // A signed list the repository keeps decides first: it can
            // withdraw a release and it pins the bundle's digest.
            if let Some(list) = self.github_list(project, &repo, pin, opts).await?
                && let Some(entry) = list
                    .predicate
                    .releases
                    .iter()
                    .find(|r| r.version == tv.version)
            {
                if entry.is_yanked() {
                    bail!(
                        "packslip:{project}@{} was withdrawn by the vendor{}",
                        tv.version,
                        entry
                            .status_reason
                            .as_deref()
                            .map(|r| format!(": {r}"))
                            .unwrap_or_default()
                    );
                }
                return Ok(Located {
                    headers: headers_for(&entry.packslip)?,
                    url: entry.packslip.clone(),
                    digest: list.digest_of(&entry.packslip).map(str::to_string),
                });
            }
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
            return Ok(Located {
                headers: github::get_headers(&asset.browser_download_url)?,
                url: asset.browser_download_url.clone(),
                digest: None,
            });
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
        if entry.is_yanked() {
            bail!(
                "packslip:{project}@{} was withdrawn by the vendor{}",
                tv.version,
                entry
                    .status_reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            );
        }
        Ok(Located {
            url: entry.packslip.clone(),
            headers: HeaderMap::new(),
            digest: list.digest_of(&entry.packslip).map(str::to_string),
        })
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
        ]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        self.ensure_experimental()?;
        let project = self.project()?;
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = PackslipOptions::new(&raw_opts);
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

        // The manifest first: nothing else is downloaded until it verifies.
        let located = self.locate_bundle(&project, &tv, &pin, &opts).await?;
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
        if let Some(expected) = &located.digest {
            let (actual, _) = packslip::digest_file(&bundle_path)?;
            if &actual != expected {
                bail!(
                    "the packslip at {} is not the one the signed release list points at (sha256 {actual}, list says {expected})",
                    located.url
                );
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

        // Then the one artifact for this host, by what the manifest says.
        let artifact = select_artifact(
            &statement.predicate.artifacts,
            &HostPlatform::current(),
            opts.variant().as_deref(),
        )?
        .clone();
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
        let platform_key = self.get_platform_key();
        {
            let info = tv.lock_platforms.entry(platform_key).or_default();
            info.url = Some(url);
            if info.checksum.is_none()
                && let Some(sha256) = statement.digest_of(&artifact.name)
            {
                info.checksum = Some(format!("sha256:{sha256}"));
            }
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
        install_artifact(&tv, &file_path, &install_opts, Some(ctx.pr.as_ref()))?;
        Self::link_bins(&tv, &artifact)?;
        file::write(
            tv.install_path().join(STATEMENT_FILE),
            serde_json::to_vec_pretty(&statement)?,
        )?;
        Ok(tv)
    }

    /// `variant` decides which artifact is downloaded, so a lock entry for a
    /// variant build is not the entry for the plain one.
    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        let mut options = BTreeMap::new();
        if let Some(variant) = PackslipOptions::new(&raw_opts).variant() {
            options.insert("variant".to_string(), variant);
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
    fn list_sequences_only_go_up() {
        let dir = tempfile::tempdir().unwrap();
        let list = |sequence: u64| -> ReleaseListStatement {
            serde_json::from_value(serde_json::json!({
                "_type": "https://in-toto.io/Statement/v1",
                "subject": [],
                "predicateType": "https://packslip.dev/releases/v1",
                "predicate": {
                    "project": "tool.example.com",
                    "generated_at": "2026-09-01T00:00:00Z",
                    "expires_at": "2026-10-01T00:00:00Z",
                    "sequence": sequence,
                    "identity": { "scheme": "sigstore-key", "key_id": "AA" },
                    "releases": []
                }
            }))
            .unwrap()
        };
        let path = dir.path().join("seq.txt");
        let check = |sequence: u64| check_sequence_at(&path, "tool.example.com", &list(sequence));
        check(3).unwrap();
        check(5).unwrap();
        let err = check(4).unwrap_err();
        assert!(err.to_string().contains("refusing to go back"), "{err}");
        check(5).unwrap();
        assert_eq!(file::read_to_string(&path).unwrap(), "5");
    }

    #[test]
    fn sequence_files_do_not_collide() {
        let a = sequence_file("github.com/foo/bar-baz");
        let b = sequence_file("github.com/foo-bar/baz");
        assert_ne!(a, b);
        assert_eq!(
            a.file_name().unwrap().to_str().unwrap(),
            "github.com%2Ffoo%2Fbar-baz.txt"
        );
        assert_eq!(a.parent(), b.parent());
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
    }
}
