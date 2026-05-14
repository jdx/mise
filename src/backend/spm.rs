use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::git::{CloneOptions, Git};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, ToolVersionOptions};
use crate::{dirs, file, github, gitlab};
use async_trait::async_trait;
use eyre::{WrapErr, bail};
use serde::Deserialize;
use serde::Deserializer;
use serde::de::{MapAccess, Visitor};
use std::path::{Path, PathBuf};
use std::{
    fmt::{self, Debug},
    sync::Arc,
};
use strum::{AsRefStr, EnumString, VariantNames};
use url::Url;
use xx::regex;

/// SPM backend requires experimental mode to be enabled
pub const EXPERIMENTAL: bool = true;

#[derive(Debug)]
pub struct SPMBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for SPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Spm
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["swift"])
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["provider", "api_url", "artifactbundle_asset"]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        let opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let provider = GitProvider::from_ba_with_opts(&self.ba, &opts);
        let repo = SwiftPackageRepo::new(&self.tool_name(), &provider)?;
        let versions = match provider.kind {
            GitProviderKind::GitLab => {
                gitlab::list_releases_from_url(&provider.api_url, repo.shorthand.as_str())
                    .await?
                    .into_iter()
                    .map(|r| VersionInfo {
                        version: r.tag_name,
                        created_at: r.released_at,
                        ..Default::default()
                    })
                    .rev()
                    .collect()
            }
            _ => github::list_releases_from_url(&provider.api_url, repo.shorthand.as_str())
                .await?
                .into_iter()
                .map(|r| VersionInfo {
                    version: r.tag_name,
                    created_at: Some(r.created_at),
                    ..Default::default()
                })
                .rev()
                .collect(),
        };

        Ok(versions)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let settings = Settings::get();
        settings.ensure_experimental("spm backend")?;

        // Check if swift is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "swift",
            &["swift"],
            "To use Swift Package Manager (spm) tools with mise, you need to install Swift first:\n\
              mise use swift@latest\n\n\
            Or install Swift via https://swift.org/download/",
        )
        .await;
        let mut tv = tv;
        let opts = tv.request.options();
        let provider = GitProvider::from_ba_with_opts(&self.ba, &opts);
        let repo = SwiftPackageRepo::new(&self.tool_name(), &provider)?;
        let revision = if tv.version == "latest" {
            self.latest_version(&ctx.config, None, ctx.before_date)
                .await?
                .ok_or_else(|| eyre::eyre!("No stable versions found"))?
        } else {
            tv.version.clone()
        };

        let artifactbundle_mode = resolve_artifactbundle_mode(&opts)?;
        if artifactbundle_mode == ArtifactBundleMode::SourceOnly
            && Settings::get().spm.artifactbundle_only
        {
            bail!("artifactbundle = false conflicts with spm.artifactbundle_only");
        }
        if artifactbundle_mode != ArtifactBundleMode::SourceOnly {
            let artifactbundle_required = requires_artifactbundle(artifactbundle_mode, &opts);
            match self
                .try_install_artifactbundle(ctx, &mut tv, &provider, &repo, &revision, &opts)
                .await
            {
                Ok(true) => return Ok(tv),
                Ok(false) if artifactbundle_required => {
                    bail!(
                        "No matching SwiftPM artifact bundle found for {}",
                        repo.shorthand
                    );
                }
                Ok(false) if Settings::get().spm.artifactbundle_only => {
                    bail!(
                        "No matching SwiftPM artifact bundle found for {}, but spm.artifactbundle_only is set",
                        repo.shorthand
                    );
                }
                Ok(false) => {}
                Err(err) if artifactbundle_required => return Err(err),
                Err(err) if Settings::get().spm.artifactbundle_only => return Err(err),
                Err(err) => {
                    debug!(
                        "SwiftPM artifact bundle install failed, falling back to source build: {err:?}"
                    );
                }
            }
        }

        self.install_from_source(ctx, &tv, &repo, &revision).await?;

        Ok(tv)
    }
}

impl SPMBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    async fn install_from_source(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        repo: &SwiftPackageRepo,
        revision: &str,
    ) -> eyre::Result<()> {
        let repo_dir = self.clone_package_repo(ctx, tv, repo, revision)?;

        let executables = self.get_executable_names(ctx, &repo_dir, tv).await?;
        if executables.is_empty() {
            return Err(eyre::eyre!("No executables found in the package"));
        }
        let executables = self.apply_filter_bins(tv, executables)?;
        let bin_path = tv.install_path().join("bin");
        file::create_dir_all(&bin_path)?;
        for executable in executables {
            let exe_path = self
                .build_executable(&executable, &repo_dir, ctx, tv)
                .await?;
            file::make_symlink(&exe_path, &bin_path.join(executable))?;
        }

        // delete (huge) intermediate artifacts
        file::remove_all(tv.install_path().join("repositories"))?;
        file::remove_all(tv.cache_path())?;

        Ok(())
    }

    fn clone_package_repo(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        package_repo: &SwiftPackageRepo,
        revision: &str,
    ) -> Result<PathBuf, eyre::Error> {
        let repo = Git::new(tv.cache_path().join("repo"));
        if !repo.exists() {
            debug!(
                "Cloning swift package repo {} to {}",
                package_repo.url.as_str(),
                repo.dir.display(),
            );
            repo.clone(
                package_repo.url.as_str(),
                CloneOptions::default().pr(ctx.pr.as_ref()),
            )?;
        }
        debug!("Checking out revision: {revision}");
        repo.update_tag(revision.to_string())?;

        // Updates submodules ensuring they match the checked-out revision
        repo.update_submodules()?;

        Ok(repo.dir)
    }

    fn apply_filter_bins(
        &self,
        tv: &ToolVersion,
        executables: Vec<String>,
    ) -> eyre::Result<Vec<String>> {
        let opts = tv.request.options();
        filter_executables(&opts, executables)
    }

    async fn get_executable_names(
        &self,
        ctx: &InstallContext,
        repo_dir: &PathBuf,
        tv: &ToolVersion,
    ) -> Result<Vec<String>, eyre::Error> {
        let package_json = cmd!(
            "swift",
            "package",
            "dump-package",
            "--package-path",
            &repo_dir,
            "--scratch-path",
            tv.install_path(),
            "--cache-path",
            dirs::CACHE.join("spm"),
        )
        .full_env(self.dependency_env(&ctx.config).await?)
        .read()?;
        let executables = serde_json::from_str::<PackageDescription>(&package_json)
            .wrap_err("Failed to parse package description")?
            .products
            .iter()
            .filter(|p| p.r#type.is_executable())
            .map(|p| p.name.clone())
            .collect::<Vec<String>>();
        debug!("Found executables: {:?}", executables);
        Ok(executables)
    }

    async fn build_executable(
        &self,
        executable: &str,
        repo_dir: &PathBuf,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> Result<PathBuf, eyre::Error> {
        debug!("Building swift package");
        CmdLineRunner::new("swift")
            .arg("build")
            .arg("--configuration")
            .arg("release")
            .arg("--product")
            .arg(executable)
            .arg("--scratch-path")
            .arg(tv.install_path())
            .arg("--package-path")
            .arg(repo_dir)
            .arg("--cache-path")
            .arg(dirs::CACHE.join("spm"))
            .with_pr(ctx.pr.as_ref())
            .prepend_path(
                self.dependency_toolset(&ctx.config)
                    .await?
                    .list_paths(&ctx.config)
                    .await,
            )?
            .execute()?;

        let bin_path = cmd!(
            "swift",
            "build",
            "--configuration",
            "release",
            "--product",
            &executable,
            "--package-path",
            &repo_dir,
            "--scratch-path",
            tv.install_path(),
            "--cache-path",
            dirs::CACHE.join("spm"),
            "--show-bin-path"
        )
        .full_env(self.dependency_env(&ctx.config).await?)
        .read()?;
        Ok(PathBuf::from(bin_path.trim().to_string()).join(executable))
    }

    async fn try_install_artifactbundle(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        provider: &GitProvider,
        repo: &SwiftPackageRepo,
        revision: &str,
        opts: &ToolVersionOptions,
    ) -> eyre::Result<bool> {
        let Some(asset) = resolve_artifactbundle_asset(provider, repo, revision, opts).await?
        else {
            return Ok(false);
        };

        let bundle_dir = tv.cache_path().join("artifactbundle");
        file::remove_all(&bundle_dir)?;
        file::create_dir_all(&bundle_dir)?;
        let download_path = tv.download_path().join(&asset.name);
        let headers = match provider.kind {
            GitProviderKind::GitLab => gitlab::get_headers(&asset.url),
            GitProviderKind::GitHub => github::get_headers(&asset.url),
        };
        ctx.pr.set_message(format!("download {}", asset.name));
        HTTP.download_file_with_headers(
            &asset.url,
            &download_path,
            &headers,
            Some(ctx.pr.as_ref()),
        )
        .await?;

        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(asset.url.clone());
        platform_info.url_api = asset.url_api.clone();
        if platform_info.checksum.is_none() {
            platform_info.checksum = asset.digest.clone();
        }
        self.verify_checksum(ctx, tv, &download_path)?;

        ctx.pr.set_message(format!("extract {}", asset.name));
        file::untar(
            &download_path,
            &bundle_dir,
            &file::TarOptions::new(file::TarFormat::Zip),
        )?;

        let triples = swift_target_triples(ctx, self).await?;
        let binaries = artifactbundle_binaries(&bundle_dir, &triples)?;
        if binaries.is_empty() {
            file::remove_all(tv.cache_path())?;
            return Ok(false);
        }
        let binaries = filter_artifactbundle_binaries(opts, binaries)?;
        if binaries.is_empty() {
            file::remove_all(tv.cache_path())?;
            return Ok(false);
        }

        let bin_path = tv.install_path().join("bin");
        let artifact_bin_path = tv.install_path().join("artifactbundle").join("bin");
        file::create_dir_all(&bin_path)?;
        file::create_dir_all(&artifact_bin_path)?;
        for binary in binaries {
            let installed_binary = artifact_bin_path.join(&binary.name);
            file::copy(&binary.path, &installed_binary)?;
            file::make_executable(&installed_binary)?;
            file::make_symlink(&installed_binary, &bin_path.join(&binary.name))?;
        }
        file::remove_all(tv.cache_path())?;
        Ok(true)
    }
}

/// Parses the `filter_bins` tool option if set.
///
/// Accepts either a comma-separated string (`filter_bins = "foo,bar"`) or a
/// TOML array (`filter_bins = ["foo", "bar"]`). Empty entries are ignored.
fn parse_filter_bins(opts: &crate::toolset::ToolVersionOptions) -> Option<Vec<String>> {
    let value = opts.opts.get("filter_bins")?;
    let bins: Vec<String> = match value {
        toml::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        toml::Value::String(s) => s
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => return None,
    };
    if bins.is_empty() { None } else { Some(bins) }
}

/// Restricts `executables` to those listed in `filter_bins`, preserving the
/// original declaration order from `Package.swift` rather than the order in
/// `filter_bins`. Returns an error if any name in `filter_bins` does not match
/// an available executable product.
fn filter_executables(
    opts: &crate::toolset::ToolVersionOptions,
    executables: Vec<String>,
) -> eyre::Result<Vec<String>> {
    let Some(filter) = parse_filter_bins(opts) else {
        return Ok(executables);
    };
    let missing: Vec<&str> = filter
        .iter()
        .filter(|b| !executables.contains(b))
        .map(|s| s.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(eyre::eyre!(
            "filter_bins references executable(s) not found in the package: {}. Available: {}",
            missing.join(", "),
            executables.join(", ")
        ));
    }
    Ok(executables
        .into_iter()
        .filter(|e| filter.contains(e))
        .collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitProvider {
    pub api_url: String,
    pub kind: GitProviderKind,
}

impl Default for GitProvider {
    fn default() -> Self {
        Self {
            api_url: github::API_URL.to_string(),
            kind: GitProviderKind::GitHub,
        }
    }
}

#[derive(AsRefStr, Clone, Debug, Eq, PartialEq, EnumString, VariantNames)]
pub enum GitProviderKind {
    #[strum(serialize = "github")]
    GitHub,
    #[strum(serialize = "gitlab")]
    GitLab,
}

impl GitProvider {
    #[cfg(test)]
    fn from_ba(ba: &BackendArg) -> Self {
        let opts = ba.opts();
        Self::from_ba_with_opts(ba, &opts)
    }

    fn from_ba_with_opts(ba: &BackendArg, opts: &ToolVersionOptions) -> Self {
        let provider = opts
            .get("provider")
            .unwrap_or(GitProviderKind::GitHub.as_ref());
        let kind = if ba.tool_name.contains("gitlab.com") {
            GitProviderKind::GitLab
        } else {
            match provider.to_lowercase().as_str() {
                "gitlab" => GitProviderKind::GitLab,
                _ => GitProviderKind::GitHub,
            }
        };

        let api_url = match opts.get("api_url") {
            Some(api_url) => api_url.trim_end_matches('/').to_string(),
            None => {
                Self::derive_api_url_from_tool_name(&ba.tool_name, &kind).unwrap_or_else(|| {
                    match kind {
                        GitProviderKind::GitHub => github::API_URL.to_string(),
                        GitProviderKind::GitLab => gitlab::API_URL.to_string(),
                    }
                })
            }
        };

        Self { api_url, kind }
    }

    /// When the tool name is a full URL pointing to a self-hosted instance,
    /// derive the API URL from the host instead of falling back to the public API.
    fn derive_api_url_from_tool_name(tool_name: &str, kind: &GitProviderKind) -> Option<String> {
        let name = tool_name.strip_prefix("spm:").unwrap_or(tool_name);
        let url = Url::parse(name).ok()?;
        let host = url.host_str()?;
        match host {
            "github.com" | "gitlab.com" => None,
            _ => {
                let api_path = match kind {
                    GitProviderKind::GitHub => github::API_PATH,
                    GitProviderKind::GitLab => gitlab::API_PATH,
                };
                let mut api_url = url.clone();
                api_url.set_path(api_path);
                Some(api_url.as_str().trim_end_matches('/').to_string())
            }
        }
    }
}

#[derive(Debug)]
struct SwiftPackageRepo {
    /// https://github.com/owner/repo.git
    url: Url,
    /// owner/repo_name
    shorthand: String,
}

impl SwiftPackageRepo {
    /// Parse the slug or the full URL of a GitHub package repository.
    fn new(name: &str, provider: &GitProvider) -> Result<Self, eyre::Error> {
        let name = name.strip_prefix("spm:").unwrap_or(name);
        let shorthand_regex = regex!(r"^(?:[a-zA-Z0-9_-]+/)+[a-zA-Z0-9._-]+$");
        let shorthand_in_url_regex = regex!(
            r"^https://(?P<domain>[^/]+)/(?P<shorthand>(?:[a-zA-Z0-9_-]+/)+[a-zA-Z0-9._-]+)\.git"
        );

        let (shorthand, url) = if let Some(caps) = shorthand_in_url_regex.captures(name) {
            let shorthand = caps.name("shorthand").unwrap().as_str();
            let url = Url::parse(name)?;
            (shorthand, url)
        } else if shorthand_regex.is_match(name) {
            let host = match provider.kind {
                GitProviderKind::GitHub => "github.com",
                GitProviderKind::GitLab => "gitlab.com",
            };
            let url_str = format!("https://{}/{}.git", host, name);
            let url = Url::parse(&url_str)?;
            (name, url)
        } else {
            Err(eyre::eyre!(
                "Invalid Swift package repository: {}. The repository should either be a repository slug (owner/name), or the complete URL (e.g. https://github.com/owner/name.git).",
                name
            ))?
        };

        Ok(Self {
            url,
            shorthand: shorthand.to_string(),
        })
    }
}

async fn swift_target_triples(
    ctx: &InstallContext,
    backend: &SPMBackend,
) -> eyre::Result<Vec<String>> {
    let target_info = cmd!("swift", "-print-target-info")
        .full_env(backend.dependency_env(&ctx.config).await?)
        .read()?;
    parse_swift_target_triples(&target_info)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactBundleMode {
    Auto,
    Required,
    SourceOnly,
}

impl ArtifactBundleMode {
    fn requires_artifactbundle(self) -> bool {
        matches!(self, Self::Required)
    }
}

fn resolve_artifactbundle_mode(opts: &ToolVersionOptions) -> eyre::Result<ArtifactBundleMode> {
    match opts.get_string("artifactbundle").as_deref() {
        None => Ok(ArtifactBundleMode::Auto),
        Some("true") => Ok(ArtifactBundleMode::Required),
        Some("false") => Ok(ArtifactBundleMode::SourceOnly),
        Some(value) => bail!("artifactbundle must be true or false, got {value}"),
    }
}

fn requires_artifactbundle(mode: ArtifactBundleMode, opts: &ToolVersionOptions) -> bool {
    mode.requires_artifactbundle() || opts.get("artifactbundle_asset").is_some()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactBundleReleaseAsset {
    name: String,
    url: String,
    url_api: Option<String>,
    digest: Option<String>,
}

async fn resolve_artifactbundle_asset(
    provider: &GitProvider,
    repo: &SwiftPackageRepo,
    revision: &str,
    opts: &ToolVersionOptions,
) -> eyre::Result<Option<ArtifactBundleReleaseAsset>> {
    let assets = match provider.kind {
        GitProviderKind::GitLab => {
            gitlab::list_releases_from_url(&provider.api_url, repo.shorthand.as_str())
                .await?
                .into_iter()
                .find(|r| r.tag_name == revision)
                .map(|r| {
                    r.assets
                        .links
                        .into_iter()
                        .map(|a| ArtifactBundleReleaseAsset {
                            name: a.name,
                            url: a.direct_asset_url,
                            url_api: Some(a.url),
                            digest: None,
                        })
                        .collect()
                })
        }
        GitProviderKind::GitHub => {
            github::list_releases_from_url(&provider.api_url, repo.shorthand.as_str())
                .await?
                .into_iter()
                .find(|r| r.tag_name == revision)
                .map(|r| {
                    r.assets
                        .into_iter()
                        .map(|a| ArtifactBundleReleaseAsset {
                            name: a.name,
                            url: a.browser_download_url,
                            url_api: Some(a.url),
                            digest: a.digest,
                        })
                        .collect()
                })
        }
    };
    let Some(assets) = assets else {
        return Ok(None);
    };
    select_artifactbundle_asset(assets, opts)
}

fn select_artifactbundle_asset(
    assets: Vec<ArtifactBundleReleaseAsset>,
    opts: &ToolVersionOptions,
) -> eyre::Result<Option<ArtifactBundleReleaseAsset>> {
    let artifactbundle_asset = opts.get("artifactbundle_asset");
    if let Some(name) = artifactbundle_asset {
        if !is_artifactbundle_zip(name) {
            bail!("artifactbundle_asset must end with .artifactbundle.zip, got {name}");
        }
        return assets
            .into_iter()
            .find(|a| a.name == name)
            .map(Some)
            .ok_or_else(|| {
                eyre::eyre!("artifactbundle_asset not found in release assets: {name}")
            });
    }

    let candidates = assets
        .into_iter()
        .filter(|a| is_artifactbundle_zip(&a.name))
        .collect::<Vec<_>>();
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.into_iter().next()),
        _ => bail!(
            "multiple SwiftPM artifact bundles found: {}. Set artifactbundle_asset to choose one",
            candidates
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn is_artifactbundle_zip(name: &str) -> bool {
    name.to_lowercase().ends_with(".artifactbundle.zip")
}

#[derive(Debug, Deserialize)]
struct SwiftTargetInfo {
    target: SwiftTarget,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwiftTarget {
    triple: String,
    unversioned_triple: String,
    module_triple: String,
}

fn parse_swift_target_triples(json: &str) -> eyre::Result<Vec<String>> {
    let info = serde_json::from_str::<SwiftTargetInfo>(json)
        .wrap_err("Failed to parse swift target info")?;
    let mut seen = std::collections::HashSet::new();
    let triples = vec![
        info.target.unversioned_triple,
        info.target.triple,
        info.target.module_triple,
    ]
    .into_iter()
    .filter(|triple| seen.insert(triple.clone()))
    .collect();
    Ok(triples)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactBundleBinary {
    name: String,
    path: PathBuf,
}

fn artifactbundle_binaries(
    extract_dir: &Path,
    target_triples: &[String],
) -> eyre::Result<Vec<ArtifactBundleBinary>> {
    let mut binaries = vec![];
    for bundle in artifactbundle_dirs(extract_dir)? {
        let info = file::read_to_string(bundle.join("info.json"))?;
        let manifest = serde_json::from_str::<ArtifactBundleInfo>(&info)
            .wrap_err("Failed to parse artifact bundle info.json")?;
        for (name, artifact) in manifest.artifacts {
            if artifact.r#type != "executable" {
                continue;
            }
            for variant in artifact.variants {
                if variant
                    .supported_triples
                    .iter()
                    .any(|triple| target_triples.contains(triple))
                {
                    let path = bundle.join(&variant.path);
                    if !path.is_file() {
                        bail!(
                            "artifact bundle executable does not exist: {}",
                            path.display()
                        );
                    }
                    binaries.push(ArtifactBundleBinary {
                        name: name.clone(),
                        path,
                    });
                    break;
                }
            }
        }
    }
    Ok(binaries)
}

fn artifactbundle_dirs(extract_dir: &Path) -> eyre::Result<Vec<PathBuf>> {
    let mut bundles = vec![];
    if extract_dir.join("info.json").is_file() {
        bundles.push(extract_dir.to_path_buf());
    }

    for entry in std::fs::read_dir(extract_dir)
        .wrap_err_with(|| format!("failed to read_dir: {}", extract_dir.display()))?
    {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        if is_artifactbundle_dir(&path) {
            bundles.push(path);
        } else {
            bundles.extend(direct_artifactbundle_dirs(&path)?);
        }
    }
    bundles.sort();
    Ok(bundles)
}

fn direct_artifactbundle_dirs(dir: &Path) -> eyre::Result<Vec<PathBuf>> {
    let mut bundles = vec![];
    for entry in
        std::fs::read_dir(dir).wrap_err_with(|| format!("failed to read_dir: {}", dir.display()))?
    {
        let path = entry?.path();
        if path.is_dir() && is_artifactbundle_dir(&path) {
            bundles.push(path);
        }
    }
    bundles.sort();
    Ok(bundles)
}

fn is_artifactbundle_dir(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "artifactbundle") && path.join("info.json").is_file()
}

fn filter_artifactbundle_binaries(
    opts: &ToolVersionOptions,
    binaries: Vec<ArtifactBundleBinary>,
) -> eyre::Result<Vec<ArtifactBundleBinary>> {
    let names = binaries.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let filtered = filter_executables(opts, names)?;
    Ok(binaries
        .into_iter()
        .filter(|b| filtered.contains(&b.name))
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::cli::args::BackendResolution;
    use crate::{config::Config, toolset::ToolVersionOptions};

    use super::*;
    use indexmap::indexmap;
    use pretty_assertions::assert_str_eq;

    #[tokio::test]
    async fn test_git_provider_from_ba() {
        // Example of defining a capture (closure) in Rust:
        let get_ba = |tool: String, opts: Option<ToolVersionOptions>| {
            BackendArg::new_raw(
                "spm".to_string(),
                Some(tool.clone()),
                tool,
                opts,
                BackendResolution::new(true),
            )
        };

        assert_eq!(
            GitProvider::from_ba(&get_ba("tool".to_string(), None)),
            GitProvider {
                api_url: github::API_URL.to_string(),
                kind: GitProviderKind::GitHub
            }
        );

        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "tool".to_string(),
                Some(ToolVersionOptions {
                    opts: indexmap![
                        "provider".to_string() => toml::Value::String("gitlab".to_string())
                    ]
                    .into(),
                    ..Default::default()
                })
            )),
            GitProvider {
                api_url: gitlab::API_URL.to_string(),
                kind: GitProviderKind::GitLab
            }
        );

        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "tool".to_string(),
                Some(ToolVersionOptions {
                    opts: indexmap![
                        "api_url".to_string() => toml::Value::String("https://gitlab.acme.com/api/v4".to_string()),
                        "provider".to_string() => toml::Value::String("gitlab".to_string()),
                    ]
                    .into(),
                    ..Default::default()
                })
            )),
            GitProvider {
                api_url: "https://gitlab.acme.com/api/v4".to_string(),
                kind: GitProviderKind::GitLab
            }
        );

        // Self-hosted GitHub Enterprise URL without api_url -> should derive from host
        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "https://github.acme.com/org/Tool.git".to_string(),
                None
            )),
            GitProvider {
                api_url: "https://github.acme.com/api/v3".to_string(),
                kind: GitProviderKind::GitHub
            }
        );

        // Self-hosted GitLab URL without api_url -> should derive from host
        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "https://gitlab.acme.com/org/Tool.git".to_string(),
                Some(ToolVersionOptions {
                    opts: indexmap![
                        "provider".to_string() => toml::Value::String("gitlab".to_string())
                    ]
                    .into(),
                    ..Default::default()
                })
            )),
            GitProvider {
                api_url: "https://gitlab.acme.com/api/v4".to_string(),
                kind: GitProviderKind::GitLab
            }
        );

        // github.com URL without api_url -> should use default
        assert_eq!(
            GitProvider::from_ba(&get_ba("https://github.com/org/Tool.git".to_string(), None)),
            GitProvider {
                api_url: github::API_URL.to_string(),
                kind: GitProviderKind::GitHub
            }
        );

        // Explicit api_url should take precedence over derived URL
        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "https://github.acme.com/org/Tool.git".to_string(),
                Some(ToolVersionOptions {
                    opts: indexmap![
                        "api_url".to_string() => toml::Value::String("https://custom-api.acme.com/v3".to_string())
                    ]
                    .into(),
                    ..Default::default()
                })
            )),
            GitProvider {
                api_url: "https://custom-api.acme.com/v3".to_string(),
                kind: GitProviderKind::GitHub
            }
        );
    }

    #[tokio::test]
    async fn test_spm_repo_init_by_shorthand() {
        let _config = Config::get().await.unwrap();
        let package_repo =
            SwiftPackageRepo::new("nicklockwood/SwiftFormat", &GitProvider::default()).unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");

        let package_repo = SwiftPackageRepo::new(
            "acme/nicklockwood/SwiftFormat",
            &GitProvider {
                api_url: gitlab::API_URL.to_string(),
                kind: GitProviderKind::GitLab,
            },
        )
        .unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://gitlab.com/acme/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "acme/nicklockwood/SwiftFormat");
    }

    #[tokio::test]
    async fn test_spm_repo_init_name() {
        let _config = Config::get().await.unwrap();
        assert!(
            SwiftPackageRepo::new("owner/name.swift", &GitProvider::default()).is_ok(),
            "name part can contain ."
        );
        assert!(
            SwiftPackageRepo::new("owner/name_swift", &GitProvider::default()).is_ok(),
            "name part can contain _"
        );
        assert!(
            SwiftPackageRepo::new("owner/name-swift", &GitProvider::default()).is_ok(),
            "name part can contain -"
        );
        assert!(
            SwiftPackageRepo::new("owner/name$swift", &GitProvider::default()).is_err(),
            "name part cannot contain characters other than a-zA-Z0-9._-"
        );
    }

    #[tokio::test]
    async fn test_spm_repo_init_by_url() {
        let package_repo = SwiftPackageRepo::new(
            "https://github.com/nicklockwood/SwiftFormat.git",
            &GitProvider::default(),
        )
        .unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");

        let package_repo = SwiftPackageRepo::new(
            "https://gitlab.acme.com/acme/someuser/SwiftTool.git",
            &GitProvider {
                api_url: "https://api.gitlab.acme.com/api/v4".to_string(),
                kind: GitProviderKind::GitLab,
            },
        )
        .unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://gitlab.acme.com/acme/someuser/SwiftTool.git"
        );
        assert_str_eq!(package_repo.shorthand, "acme/someuser/SwiftTool");
    }

    fn opts_with_filter_bins(value: toml::Value) -> ToolVersionOptions {
        ToolVersionOptions {
            opts: indexmap!["filter_bins".to_string() => value].into(),
            ..Default::default()
        }
    }

    fn opts_with(key: &str, value: toml::Value) -> ToolVersionOptions {
        ToolVersionOptions {
            opts: indexmap![key.to_string() => value].into(),
            ..Default::default()
        }
    }

    fn release_asset(name: &str) -> ArtifactBundleReleaseAsset {
        ArtifactBundleReleaseAsset {
            name: name.to_string(),
            url: format!("https://example.com/{name}"),
            url_api: None,
            digest: None,
        }
    }

    #[test]
    fn test_resolve_artifactbundle_mode() {
        assert_eq!(
            resolve_artifactbundle_mode(&ToolVersionOptions::default()).unwrap(),
            ArtifactBundleMode::Auto
        );
        assert_eq!(
            resolve_artifactbundle_mode(&opts_with("artifactbundle", toml::Value::Boolean(true)))
                .unwrap(),
            ArtifactBundleMode::Required
        );
        assert_eq!(
            resolve_artifactbundle_mode(&opts_with("artifactbundle", toml::Value::Boolean(false)))
                .unwrap(),
            ArtifactBundleMode::SourceOnly
        );
        assert!(
            resolve_artifactbundle_mode(&opts_with(
                "artifactbundle",
                toml::Value::String("sometimes".to_string())
            ))
            .is_err()
        );
    }

    #[test]
    fn test_requires_artifactbundle() {
        assert!(!requires_artifactbundle(
            ArtifactBundleMode::Auto,
            &ToolVersionOptions::default()
        ));
        assert!(requires_artifactbundle(
            ArtifactBundleMode::Required,
            &ToolVersionOptions::default()
        ));
        assert!(requires_artifactbundle(
            ArtifactBundleMode::Auto,
            &opts_with(
                "artifactbundle_asset",
                toml::Value::String("tool.artifactbundle.zip".to_string()),
            )
        ));
    }

    #[test]
    fn test_select_artifactbundle_asset() {
        let selected = select_artifactbundle_asset(
            vec![release_asset("tool.artifactbundle.zip")],
            &ToolVersionOptions::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.name, "tool.artifactbundle.zip");

        let selected = select_artifactbundle_asset(
            vec![
                release_asset("tool.artifactbundle.zip"),
                release_asset("tool.tar.gz"),
            ],
            &opts_with(
                "artifactbundle_asset",
                toml::Value::String("tool.artifactbundle.zip".to_string()),
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.name, "tool.artifactbundle.zip");

        assert!(
            select_artifactbundle_asset(
                vec![release_asset("tool.tar.gz")],
                &opts_with(
                    "artifactbundle_asset",
                    toml::Value::String("tool.tar.gz".to_string()),
                ),
            )
            .is_err()
        );
        assert!(
            select_artifactbundle_asset(
                vec![release_asset("tool.artifactbundle.zip")],
                &opts_with(
                    "artifactbundle_asset",
                    toml::Value::String("missing.artifactbundle.zip".to_string()),
                ),
            )
            .is_err()
        );
        assert!(
            select_artifactbundle_asset(
                vec![
                    release_asset("a.artifactbundle.zip"),
                    release_asset("b.artifactbundle.zip"),
                ],
                &ToolVersionOptions::default(),
            )
            .is_err()
        );
        assert!(
            select_artifactbundle_asset(
                vec![release_asset("tool.tar.gz")],
                &ToolVersionOptions::default(),
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn test_parse_swift_target_triples() {
        let triples = parse_swift_target_triples(
            r#"{
              "target": {
                "triple": "arm64-apple-macosx26.0",
                "unversionedTriple": "arm64-apple-macosx",
                "moduleTriple": "arm64-apple-macos"
              }
            }"#,
        )
        .unwrap();
        assert_eq!(
            triples,
            vec![
                "arm64-apple-macosx".to_string(),
                "arm64-apple-macosx26.0".to_string(),
                "arm64-apple-macos".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_swift_target_triples_deduplicates_non_consecutive_values() {
        let triples = parse_swift_target_triples(
            r#"{
              "target": {
                "triple": "arm64-apple-macosx26.0",
                "unversionedTriple": "arm64-apple-macosx",
                "moduleTriple": "arm64-apple-macosx"
              }
            }"#,
        )
        .unwrap();
        assert_eq!(
            triples,
            vec![
                "arm64-apple-macosx".to_string(),
                "arm64-apple-macosx26.0".to_string(),
            ]
        );
    }

    #[test]
    fn test_artifactbundle_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("tool.artifactbundle");
        let bin = bundle.join("tool-1.0.0-macosx/bin");
        file::create_dir_all(&bin).unwrap();
        file::write(bin.join("tool"), "").unwrap();
        file::write(
            bundle.join("info.json"),
            r#"{
              "schemaVersion": "1.0",
              "artifacts": {
                "tool": {
                  "version": "1.0.0",
                  "type": "executable",
                  "variants": [
                    {
                      "path": "tool-1.0.0-macosx/bin/tool",
                      "supportedTriples": ["arm64-apple-macosx"]
                    }
                  ]
                },
                "library": {
                  "version": "1.0.0",
                  "type": "library",
                  "variants": [
                    {
                      "path": "lib",
                      "supportedTriples": ["arm64-apple-macosx"]
                    }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let binaries =
            artifactbundle_binaries(tmp.path(), &["arm64-apple-macosx".to_string()]).unwrap();
        assert_eq!(binaries.len(), 1);
        assert_eq!(binaries[0].name, "tool");
        assert_eq!(binaries[0].path, bin.join("tool"));

        let binaries =
            artifactbundle_binaries(tmp.path(), &["x86_64-unknown-linux-gnu".to_string()]).unwrap();
        assert!(binaries.is_empty());
    }

    #[test]
    fn test_artifactbundle_binaries_uses_first_matching_variant() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("tool.artifactbundle");
        let first_bin = bundle.join("tool-1.0.0-macosx/bin");
        let second_bin = bundle.join("tool-1.0.0-macos/bin");
        file::create_dir_all(&first_bin).unwrap();
        file::create_dir_all(&second_bin).unwrap();
        file::write(first_bin.join("tool"), "").unwrap();
        file::write(second_bin.join("tool"), "").unwrap();
        file::write(
            bundle.join("info.json"),
            r#"{
              "schemaVersion": "1.0",
              "artifacts": {
                "tool": {
                  "version": "1.0.0",
                  "type": "executable",
                  "variants": [
                    {
                      "path": "tool-1.0.0-macosx/bin/tool",
                      "supportedTriples": ["arm64-apple-macosx"]
                    },
                    {
                      "path": "tool-1.0.0-macos/bin/tool",
                      "supportedTriples": ["arm64-apple-macos"]
                    }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let binaries = artifactbundle_binaries(
            tmp.path(),
            &[
                "arm64-apple-macosx".to_string(),
                "arm64-apple-macos".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(binaries.len(), 1);
        assert_eq!(binaries[0].path, first_bin.join("tool"));
    }

    #[test]
    fn test_artifactbundle_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        file::write(tmp.path().join("info.json"), "{}").unwrap();

        let direct = tmp.path().join("direct.artifactbundle");
        file::create_dir_all(&direct).unwrap();
        file::write(direct.join("info.json"), "{}").unwrap();

        let wrapped = tmp.path().join("wrapper").join("wrapped.artifactbundle");
        file::create_dir_all(&wrapped).unwrap();
        file::write(wrapped.join("info.json"), "{}").unwrap();

        let deeper = tmp
            .path()
            .join("wrapper")
            .join("too-deep")
            .join("deep.artifactbundle");
        file::create_dir_all(&deeper).unwrap();
        file::write(deeper.join("info.json"), "{}").unwrap();

        let dirs = artifactbundle_dirs(tmp.path()).unwrap();
        assert_eq!(
            dirs,
            vec![tmp.path().to_path_buf(), direct, wrapped],
            "search should be limited to the bundle root, direct children, and one wrapper directory"
        );
    }

    #[test]
    fn test_filter_artifactbundle_binaries() {
        let binaries = vec![
            ArtifactBundleBinary {
                name: "a".to_string(),
                path: PathBuf::from("/tmp/a"),
            },
            ArtifactBundleBinary {
                name: "b".to_string(),
                path: PathBuf::from("/tmp/b"),
            },
        ];
        let opts = opts_with_filter_bins(toml::Value::String("b".to_string()));
        let filtered = filter_artifactbundle_binaries(&opts, binaries).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "b");
    }

    #[test]
    fn test_parse_filter_bins() {
        assert_eq!(parse_filter_bins(&ToolVersionOptions::default()), None);

        assert_eq!(
            parse_filter_bins(&opts_with_filter_bins(toml::Value::String(
                "swiftly".to_string()
            ))),
            Some(vec!["swiftly".to_string()])
        );

        assert_eq!(
            parse_filter_bins(&opts_with_filter_bins(toml::Value::String(
                " foo , bar , ".to_string()
            ))),
            Some(vec!["foo".to_string(), "bar".to_string()])
        );

        assert_eq!(
            parse_filter_bins(&opts_with_filter_bins(toml::Value::Array(vec![
                toml::Value::String("foo".to_string()),
                toml::Value::String(" bar".to_string()),
                toml::Value::String("".to_string()),
            ]))),
            Some(vec!["foo".to_string(), "bar".to_string()])
        );

        assert_eq!(
            parse_filter_bins(&opts_with_filter_bins(toml::Value::String(
                " , ".to_string()
            ))),
            None,
            "whitespace-only entries should yield None"
        );
    }

    #[test]
    fn test_filter_executables_passthrough_when_unset() {
        let executables = vec!["a".to_string(), "b".to_string()];
        let result =
            filter_executables(&ToolVersionOptions::default(), executables.clone()).unwrap();
        assert_eq!(result, executables);
    }

    #[test]
    fn test_filter_executables_restricts_and_preserves_order() {
        let executables = vec![
            "swiftly".to_string(),
            "test-swiftly".to_string(),
            "helper".to_string(),
        ];
        let opts = opts_with_filter_bins(toml::Value::Array(vec![
            toml::Value::String("helper".to_string()),
            toml::Value::String("swiftly".to_string()),
        ]));
        let result = filter_executables(&opts, executables).unwrap();
        assert_eq!(result, vec!["swiftly".to_string(), "helper".to_string()]);
    }

    #[test]
    fn test_filter_executables_errors_on_missing_name() {
        let executables = vec!["swiftly".to_string(), "test-swiftly".to_string()];
        let opts = opts_with_filter_bins(toml::Value::String("does-not-exist".to_string()));
        let err = filter_executables(&opts, executables).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("does-not-exist"), "got: {msg}");
        assert!(msg.contains("swiftly"), "got: {msg}");
    }
}

/// https://developer.apple.com/documentation/packagedescription
#[derive(Deserialize)]
struct PackageDescription {
    products: Vec<PackageDescriptionProduct>,
}

#[derive(Deserialize)]
struct PackageDescriptionProduct {
    name: String,
    #[serde(deserialize_with = "PackageDescriptionProductType::deserialize_product_type_field")]
    r#type: PackageDescriptionProductType,
}

#[derive(Deserialize)]
enum PackageDescriptionProductType {
    Executable,
    Other,
}

impl PackageDescriptionProductType {
    fn is_executable(&self) -> bool {
        matches!(self, Self::Executable)
    }

    /// Swift determines the toolchain to use with a given package using a comment in the Package.swift file at the top.
    /// For example:
    ///   // swift-tools-version: 6.0
    ///
    /// The version of the toolchain can be older than the Swift version used to build the package. This versioning gives
    /// Apple the flexibility to introduce and flag breaking changes in the toolchain.
    ///
    /// How to determine the product type is something that might change across different versions of Swift.
    ///
    /// ## Swift 5.x
    ///
    /// Product type is a key in the map with an undocumented value that we are not interested in and can be easily skipped.
    ///
    /// Example:
    /// ```json
    /// "type" : {
    ///     "executable" : null
    /// }
    /// ```
    /// or
    /// ```json
    /// "type" : {
    ///     "library" : [
    ///       "automatic"
    ///     ]
    /// }
    /// ```
    ///
    /// ## Swift 6.x
    ///
    /// The product type is directly the value under the key "type"
    ///
    /// Example:
    ///
    /// ```json
    /// "type": "executable"
    /// ```
    ///
    fn deserialize_product_type_field<'de, D>(
        deserializer: D,
    ) -> Result<PackageDescriptionProductType, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TypeFieldVisitor;

        impl<'de> Visitor<'de> for TypeFieldVisitor {
            type Value = PackageDescriptionProductType;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map with a key 'executable' or other types")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                if let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => {
                            let value: String = map.next_value()?;
                            if value == "executable" {
                                Ok(PackageDescriptionProductType::Executable)
                            } else {
                                Ok(PackageDescriptionProductType::Other)
                            }
                        }
                        "executable" => {
                            // Skip the value by reading it into a dummy serde_json::Value
                            let _value: serde_json::Value = map.next_value()?;
                            Ok(PackageDescriptionProductType::Executable)
                        }
                        _ => {
                            let _value: serde_json::Value = map.next_value()?;
                            Ok(PackageDescriptionProductType::Other)
                        }
                    }
                } else {
                    Err(serde::de::Error::custom("missing key"))
                }
            }
        }

        deserializer.deserialize_map(TypeFieldVisitor)
    }
}

/// https://github.com/swiftlang/swift-evolution/blob/main/proposals/0305-swiftpm-binary-target-improvements.md
#[derive(Deserialize)]
struct ArtifactBundleInfo {
    artifacts: std::collections::BTreeMap<String, ArtifactBundleArtifact>,
}

#[derive(Deserialize)]
struct ArtifactBundleArtifact {
    r#type: String,
    variants: Vec<ArtifactBundleVariant>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactBundleVariant {
    path: String,
    supported_triples: Vec<String>,
}
