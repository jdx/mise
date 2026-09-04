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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{Result, WrapErr, bail, eyre};
use itertools::Itertools;
use packslip::model::{Artifact, ReleaseListStatement, Statement, repository, repository_subpath};
use packslip::sigstore::{Policy, Trust};
use reqwest::header::HeaderMap;

use crate::backend::static_helpers::install_artifact;
use crate::backend::{
    Backend, BackendType, MISE_BINS_DIR, SecurityFeature, VersionInfo,
    runtime_path_for_install_path,
};
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::file;
use crate::github;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::platform::Platform;
use crate::toolset::{ToolVersion, ToolVersionOptions};

/// The verified statement, kept beside the install so the rest of mise can
/// read what the release declared without verifying it again.
pub(crate) const STATEMENT_FILE: &str = ".mise-packslip.json";

/// Archive formats mise can unpack, best first. Installers (`deb`, `dmg`,
/// `msi`, ...) are not among them: mise installs into its own directory.
const FORMAT_PREFERENCE: [&str; 8] = [
    "tar.xz", "tar.zst", "tar.gz", "tgz", "tar.bz2", "zip", "7z", "raw",
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

/// The version a release tag names. The packslip inside the release is the
/// authority and is checked against this at install time; listing versions
/// from tags avoids downloading every bundle. A monorepo tool's tag is
/// prefixed with its subpath (`oxlint_v1.0.0`, `cli/v1.9.4`).
pub(crate) fn version_from_tag(tag: &str, subpath: Option<&str>) -> String {
    let mut tag = tag;
    if let Some(sub) = subpath {
        let last = sub.rsplit('/').next().unwrap_or(sub);
        'strip: for name in [sub, last] {
            for sep in ['/', '-', '_', '@'] {
                if let Some(rest) = tag.strip_prefix(&format!("{name}{sep}")) {
                    tag = rest;
                    break 'strip;
                }
            }
        }
    }
    tag.strip_prefix('v').unwrap_or(tag).to_string()
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

/// The one artifact for this host, as the specification's consumer rules
/// say: match os, arch, libc, an unpackable format, and the requested
/// variant; prefer the best format; refuse to guess between two that tie.
pub(crate) fn select_artifact<'a>(
    artifacts: &'a [Artifact],
    host: &HostPlatform,
    variant: Option<&str>,
) -> Result<&'a Artifact> {
    let format_rank = |a: &Artifact| {
        FORMAT_PREFERENCE
            .iter()
            .position(|f| Some(*f) == a.format.as_deref())
            .unwrap_or(usize::MAX)
    };
    let candidates: Vec<&Artifact> = artifacts
        .iter()
        .filter(|a| a.os.as_deref() == Some(host.os.as_str()))
        .filter(|a| a.arch.as_deref() == Some(host.arch.as_str()))
        .filter(|a| match (&host.libc, &a.libc) {
            (Some(host_libc), Some(libc)) => host_libc == libc,
            _ => true,
        })
        .filter(|a| format_rank(a) != usize::MAX)
        .filter(|a| a.variant.as_deref() == variant)
        .collect();
    let Some(best) = candidates.iter().map(|a| format_rank(a)).min() else {
        let available = artifacts.iter().map(describe).join(", ");
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
        bail!(
            "no artifact for {}/{}{}{hint}. The release has: {available}",
            host.os,
            host.arch,
            host.libc
                .as_deref()
                .map(|l| format!("/{l}"))
                .unwrap_or_default(),
        );
    };
    let ties: Vec<&Artifact> = candidates
        .into_iter()
        .filter(|a| format_rank(a) == best)
        .collect();
    match ties.as_slice() {
        [one] => Ok(one),
        several => bail!(
            "the packslip lists several artifacts for this platform and mise will not guess between them: {}",
            several.iter().map(|a| describe(a)).join(", ")
        ),
    }
}

/// A path the packslip gives relative to the archive root, or, when mise
/// stripped a lone top-level directory on extraction, the same path without
/// its first component. `..` and absolute paths never resolve.
pub(crate) fn locate_in_install(install_path: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.split('/').any(|s| s == ".." || s.is_empty()) {
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
        Ok(list)
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
            let sub = repository_subpath(project);
            let releases = github::list_releases_including_prereleases(&repo).await?;
            let found = releases.iter().find_map(|r| {
                let asset = r.assets.iter().find(|a| a.name == asset_name)?;
                (version_from_tag(&r.tag_name, sub) == tv.version).then_some(asset)
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
            let Some(src) = locate_in_install(&install_path, &bin.path) else {
                warn!(
                    "{}: the packslip lists executable {} but the archive holds no such file",
                    tv.style(),
                    bin.path
                );
                continue;
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
        let project = self.project()?;
        if let Some(repo) = Self::repo(&project) {
            let asset_name = bundle_name(&project);
            let sub = repository_subpath(&project);
            let mut versions: Vec<VersionInfo> = github::list_releases_including_prereleases(&repo)
                .await?
                .into_iter()
                .filter(|r| r.assets.iter().any(|a| a.name == asset_name))
                .map(|r| VersionInfo {
                    version: version_from_tag(&r.tag_name, sub),
                    created_at: Some(r.released_at().to_string()),
                    release_url: Some(format!(
                        "https://github.com/{repo}/releases/tag/{}",
                        r.tag_name
                    )),
                    prerelease: Some(r.prerelease),
                    ..Default::default()
                })
                .collect();
            versions.reverse();
            return Ok(versions);
        }
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = PackslipOptions::new(&raw_opts);
        let pin = pin(&project, &opts)?;
        let list = self.release_list(&project, &pin, &opts).await?;
        let mut versions: Vec<VersionInfo> = list
            .predicate
            .releases
            .iter()
            .filter(|r| !r.is_yanked())
            .map(|r| VersionInfo {
                version: r.version.clone(),
                created_at: Some(r.published_at.clone()),
                prerelease: Some(r.prerelease),
                ..Default::default()
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
        let file_path = tv.download_path().join(&artifact.name);
        ctx.pr.next_operation();
        ctx.pr.set_message(format!("download {}", artifact.name));
        let headers = if github::is_github_api_url(&url::Url::parse(&url)?)
            || url.starts_with("https://github.com/")
        {
            github::get_headers(&url)?
        } else {
            HeaderMap::new()
        };
        HTTP.download_file_with_headers(&url, &file_path, &headers, Some(ctx.pr.as_ref()))
            .await?;

        // The signed digest and size, before anything the lockfile remembers.
        ctx.pr.next_operation();
        ctx.pr.set_message(format!("verify {}", artifact.name));
        verify_bundle(&bundle, &pin, require_log, &[&file_path])
            .wrap_err_with(|| format!("verifying {} against its packslip", artifact.name))?;
        let platform_key = self.get_platform_key();
        {
            let info = tv.lock_platforms.entry(platform_key).or_default();
            info.url = Some(url);
            if let Some(sha256) = statement.digest_of(&artifact.name) {
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
        if artifact.format.as_deref() == Some("raw")
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
    fn versions_from_tags() {
        assert_eq!(version_from_tag("v1.2.3", None), "1.2.3");
        assert_eq!(version_from_tag("1.2.3", None), "1.2.3");
        assert_eq!(version_from_tag("oxlint_v1.0.0", Some("oxlint")), "1.0.0");
        assert_eq!(version_from_tag("cli/v1.9.4", Some("crates/cli")), "1.9.4");
        assert_eq!(
            version_from_tag("crates/cli/v1.9.4", Some("crates/cli")),
            "1.9.4"
        );
        assert_eq!(
            version_from_tag("buildifier-8.0.0", Some("buildifier")),
            "8.0.0"
        );
        assert_eq!(version_from_tag("v2.0.0", Some("oxlint")), "2.0.0");
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
        assert_eq!(
            locate_in_install(root, "bin"),
            None,
            "a directory is not a bin"
        );
    }
}
