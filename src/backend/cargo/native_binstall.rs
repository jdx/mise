use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use eyre::{Context, bail, eyre};
use url::Url;

use crate::Result;
use crate::backend::cargo::CargoOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{get_filename_from_url, read_archive_metadata};
use crate::config::Settings;
use crate::file::{self, ExtractBudget, ExtractOptions, ExtractionFormat};
use crate::hash;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::minisign;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env};

pub(super) const WARN_AT: &str = "2027.1.0";
pub(super) const DEFAULT_AT: &str = "2027.7.0";

#[derive(Debug, Clone, Copy)]
pub(super) enum NativeBinstallAction {
    Install,
    WarnOnly,
}

pub(super) async fn install(
    ctx: &InstallContext,
    tv: &ToolVersion,
    tool_name: &str,
    action: NativeBinstallAction,
) -> Result<bool> {
    let request_options = tv.request.options();
    let opts = CargoOptions::new(&request_options);
    let version = tv.version.as_str();
    let crate_name = opts.crate_arg().unwrap_or_else(|| tool_name.to_string());
    if Settings::get().cargo.registry_name.is_some() {
        return Ok(false);
    }
    // This installer only reads crates.io, while the `cargo install` it stands
    // in for takes the crate from the registry cargo is configured to use. The
    // same name and version there can be another publisher's code.
    let cargo_env = cargo_install_env(ctx, tv).await?;
    if cargo_config_names_other_registry(&cargo_env, &tv.install_path()) {
        debug!(
            "cargo is configured to install crates from a registry other than crates.io, so {crate_name} is built with cargo install"
        );
        return Ok(false);
    }
    let package = CratesIoPackage::fetch(&crate_name, version).await?;
    let target = cargo_target_triple(&PlatformTarget::from_current())
        .ok_or_else(|| eyre!("unsupported platform for native cargo binary install"))?;

    let target_platform = PlatformTarget::from_current();
    let manifest = download_crate_manifest(&package, tv, ctx).await?;
    let mut binstall = NativeBinstallMetadata::from_manifest(&manifest, &target, &target_platform)?;
    if binstall.disabled_strategies.contains("crate-meta-data") {
        return Ok(false);
    }
    if binstall.pkg_url.is_none() {
        let Some(discovered) =
            discover_native_binstall_metadata(&package, &crate_name, version, &target).await
        else {
            return Ok(false);
        };
        binstall.pkg_url = discovered.pkg_url;
        binstall.pkg_fmt = discovered.pkg_fmt;
    }
    let pkg_url = binstall
        .pkg_url
        .as_deref()
        .expect("native binstall metadata should have a package URL");

    let manifest_bins = native_manifest_bins(&manifest);
    let bins = native_bins_to_install(opts.bin(), &manifest_bins, &package, &crate_name);
    validate_native_bin_names(&bins)?;
    let bin_names = bins.iter().map(|bin| bin.name.clone()).collect::<Vec<_>>();
    let pkg_fmt = binstall.pkg_fmt.unwrap_or_else(|| "tgz".to_string());
    let package_format = native_package_format(&pkg_fmt)?;
    let binary_ext = if cfg!(windows) { ".exe" } else { "" };
    let archive_suffix = if package_format.extraction_format.is_some() {
        format!(".{}", package_format.template_value)
    } else {
        binary_ext.to_string()
    };
    let template_ctx = BinstallTemplateContext {
        package: &package,
        crate_name: &crate_name,
        version,
        target: &target,
        archive_format: package_format.template_value,
        archive_suffix: &archive_suffix,
        binary_ext,
    };

    let bin_root = tv.install_path().join("bin");

    if package_format.extraction_format.is_none() {
        if !raw_binary_url_supports_bins(pkg_url, &bin_names) {
            return Ok(false);
        }
        if matches!(action, NativeBinstallAction::WarnOnly) {
            for bin in &bins {
                let archive_url =
                    native_binstall_artifact_url(pkg_url, &template_ctx.vars(&bin.name))?;
                if !native_binstall_file_available(&archive_url).await {
                    return Ok(false);
                }
            }
            warn_native_binstall_rollout();
            return Ok(false);
        }
        file::create_dir_all(&bin_root)?;
        let download_dir = tempfile::Builder::new()
            .prefix("mise-cargo-binstall-bin-")
            .tempdir()?;
        let mut pending = vec![];
        for bin in &bins {
            let vars = template_ctx.vars(&bin.name);
            let archive_url = native_binstall_artifact_url(pkg_url, &vars)?;
            let signature =
                native_binstall_signature(binstall.signing.as_ref(), &archive_url, &vars)?;
            let src = download_dir
                .path()
                .join(format!("{}{}", bin.name, binary_ext));
            let dest = bin_root.join(format!("{}{}", bin.name, binary_ext));
            ctx.pr
                .set_message(format!("download {}", get_filename_from_url(&archive_url)));
            download_native_binstall_file(
                &archive_url,
                &src,
                signature.as_ref(),
                Some(ctx.pr.as_ref()),
            )
            .await?;
            pending.push((src, dest));
        }
        for (src, dest) in pending {
            fs::copy(&src, &dest).wrap_err_with(|| {
                format!(
                    "failed to copy native cargo binary {} to {}",
                    file::display_path(&src),
                    file::display_path(&dest)
                )
            })?;
            file::make_executable(&dest)?;
        }
        info!("installed {crate_name}@{version} from native cargo binary artifact");
        return Ok(true);
    }

    if matches!(action, NativeBinstallAction::WarnOnly) {
        let archive_urls = if template_contains_var(pkg_url, "bin") {
            bins.iter()
                .map(|bin| native_binstall_artifact_url(pkg_url, &template_ctx.vars(&bin.name)))
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![native_binstall_artifact_url(
                pkg_url,
                &template_ctx.vars(
                    bins.first()
                        .map(|bin| bin.name.as_str())
                        .unwrap_or(&crate_name),
                ),
            )?]
        };
        if !native_binstall_files_available(&archive_urls).await {
            return Ok(false);
        }
        warn_native_binstall_rollout();
        return Ok(false);
    }

    file::create_dir_all(&bin_root)?;

    if template_contains_var(pkg_url, "bin") {
        let download_dir = tempfile::Builder::new()
            .prefix("mise-cargo-binstall-archives-")
            .tempdir()?;
        // The crate chooses how many archives there are, one per bin, so
        // they share one extract_max_ratio limit.
        let mut extract_budget = ExtractBudget::default();
        let mut pending = vec![];
        for bin in &bins {
            let vars = template_ctx.vars(&bin.name);
            let archive_url = native_binstall_artifact_url(pkg_url, &vars)?;
            let signature =
                native_binstall_signature(binstall.signing.as_ref(), &archive_url, &vars)?;
            let archive_path = download_dir.path().join(format!(
                "{}-{}",
                bin.name,
                get_filename_from_url(&archive_url)
            ));
            let extract_dir = download_dir.path().join(format!("{}-extract", bin.name));
            ctx.pr
                .set_message(format!("download {}", get_filename_from_url(&archive_url)));
            download_native_binstall_file(
                &archive_url,
                &archive_path,
                signature.as_ref(),
                Some(ctx.pr.as_ref()),
            )
            .await?;
            file::create_dir_all(&extract_dir)?;
            file::extract_archive(
                &archive_path,
                &extract_dir,
                package_format.extraction_format.unwrap(),
                &ExtractOptions {
                    strip_components: 0,
                    pr: Some(ctx.pr.as_ref()),
                    preserve_mtime: true,
                },
            )?;
            extract_budget.add(&archive_path, &extract_dir)?;
            // The binary is copied by path, which reads through a shipped link.
            file::ensure_symlinks_stay_within(&extract_dir)?;
            let Some(src) = find_native_bin(&extract_dir, binstall.bin_dir.as_deref(), &vars)?
            else {
                if bin.required_features.is_empty() {
                    bail!(
                        "native cargo binary artifact did not contain {}{}",
                        bin.name,
                        binary_ext
                    );
                }
                debug!(
                    "native cargo binary artifact skipped optional bin {} requiring features {}",
                    bin.name,
                    bin.required_features.join(",")
                );
                continue;
            };
            let dest = bin_root.join(format!("{}{}", bin.name, binary_ext));
            pending.push((src, dest));
        }
        validate_native_bin_sources(&pending)?;
        if pending.is_empty() {
            return Ok(false);
        }
        for (src, dest) in pending {
            fs::copy(&src, &dest).wrap_err_with(|| {
                format!(
                    "failed to copy native cargo binary {} to {}",
                    file::display_path(&src),
                    file::display_path(&dest)
                )
            })?;
            file::make_executable(&dest)?;
        }
        info!("installed {crate_name}@{version} from native cargo binary artifact");
        return Ok(true);
    }

    let archive_vars = template_ctx.vars(
        bins.first()
            .map(|bin| bin.name.as_str())
            .unwrap_or(&crate_name),
    );
    let archive_url = native_binstall_artifact_url(pkg_url, &archive_vars)?;
    let signature =
        native_binstall_signature(binstall.signing.as_ref(), &archive_url, &archive_vars)?;

    let archive_path = tv.download_path().join(get_filename_from_url(&archive_url));
    ctx.pr.set_message(format!(
        "download {}",
        archive_path.file_name().unwrap().to_string_lossy()
    ));
    download_native_binstall_file(
        &archive_url,
        &archive_path,
        signature.as_ref(),
        Some(ctx.pr.as_ref()),
    )
    .await?;

    let extract_dir = tempfile::Builder::new()
        .prefix("mise-cargo-binstall-")
        .tempdir()?;
    ctx.pr.next_operation();
    file::extract_archive(
        &archive_path,
        extract_dir.path(),
        package_format.extraction_format.unwrap(),
        &ExtractOptions {
            strip_components: 0,
            pr: Some(ctx.pr.as_ref()),
            preserve_mtime: true,
        },
    )?;
    // The binaries are copied by path, which reads through a shipped link.
    file::ensure_symlinks_stay_within(extract_dir.path())?;

    let mut pending = vec![];
    for bin in &bins {
        let vars = template_ctx.vars(&bin.name);
        let Some(src) = find_native_bin(extract_dir.path(), binstall.bin_dir.as_deref(), &vars)?
        else {
            if bin.required_features.is_empty() {
                bail!(
                    "native cargo binary artifact did not contain {}{}",
                    bin.name,
                    binary_ext
                );
            }
            debug!(
                "native cargo binary artifact skipped optional bin {} requiring features {}",
                bin.name,
                bin.required_features.join(",")
            );
            continue;
        };
        let dest = bin_root.join(format!("{}{}", bin.name, binary_ext));
        pending.push((src, dest));
    }
    validate_native_bin_sources(&pending)?;
    if pending.is_empty() {
        return Ok(false);
    }
    for (src, dest) in pending {
        fs::copy(&src, &dest).wrap_err_with(|| {
            format!(
                "failed to copy native cargo binary {} to {}",
                file::display_path(&src),
                file::display_path(&dest)
            )
        })?;
        file::make_executable(&dest)?;
    }
    info!("installed {crate_name}@{version} from native cargo binary artifact");
    Ok(true)
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoPackageResponse {
    #[serde(rename = "crate")]
    krate: Option<CratesIoCrate>,
    version: CratesIoPackageVersion,
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoCrate {
    name: String,
    repository: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoPackageVersion {
    #[serde(rename = "crate", deserialize_with = "deserialize_crate_name")]
    name: String,
    num: String,
    dl_path: String,
    /// sha256 of the `.crate` file, the value cargo checks as `cksum` from the
    /// index. Required, so a reply without one cannot skip the check.
    checksum: String,
    repository: Option<String>,
    #[serde(default)]
    bin_names: Vec<String>,
}

#[derive(Debug)]
struct CratesIoPackage {
    name: String,
    num: String,
    dl_path: String,
    checksum: String,
    repository: Option<String>,
    bin_names: Vec<String>,
}

impl CratesIoPackage {
    async fn fetch(crate_name: &str, version: &str) -> Result<Self> {
        let url = format!("https://crates.io/api/v1/crates/{crate_name}/{version}");
        let response: CratesIoPackageResponse = HTTP_FETCH.json(&url).await?;
        let name = response
            .krate
            .as_ref()
            .map(|krate| krate.name.clone())
            .unwrap_or(response.version.name);
        let repository = response
            .krate
            .and_then(|krate| krate.repository)
            .or(response.version.repository);
        Ok(Self {
            name,
            num: response.version.num,
            dl_path: response.version.dl_path,
            checksum: response.version.checksum,
            repository,
            bin_names: response.version.bin_names,
        })
    }

    fn download_url(&self) -> String {
        format!("https://crates.io{}", self.dl_path)
    }
}

fn deserialize_crate_name<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum CrateName {
        Name(String),
        Object { name: String },
    }

    match <CrateName as serde::Deserialize>::deserialize(deserializer)? {
        CrateName::Name(name) | CrateName::Object { name } => Ok(name),
    }
}

#[derive(Debug, Default)]
struct NativeBinstallMetadata {
    pkg_url: Option<String>,
    bin_dir: Option<String>,
    pkg_fmt: Option<String>,
    disabled_strategies: BTreeSet<String>,
    signing: Option<NativeBinstallSigning>,
}

/// `[package.metadata.binstall.signing]`: the minisign key the crate's author
/// signs its prebuilt artifacts with. An artifact that does not verify against
/// it is not installed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct NativeBinstallSigning {
    algorithm: String,
    pubkey: String,
    /// Template for the signature's URL, `{ url }.sig` when absent.
    file: Option<String>,
}

/// Where an artifact's signature is and the key it has to verify against.
#[derive(Debug)]
struct NativeBinstallSignature<'a> {
    url: String,
    pubkey: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeCargoBin {
    name: String,
    required_features: Vec<String>,
}

impl NativeBinstallMetadata {
    fn from_manifest(
        manifest: &toml::Value,
        target: &str,
        platform: &PlatformTarget,
    ) -> Result<Self> {
        let Some(root) = manifest
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("binstall"))
            .and_then(toml::Value::as_table)
        else {
            return Ok(Self::default());
        };

        let mut merged = root.clone();
        let cfgs = native_target_cfgs(platform, target);
        if let Some(overrides) = root.get("overrides").and_then(toml::Value::as_table) {
            for (key, value) in overrides {
                if key == target {
                    continue;
                }
                if native_cfg_override_matches(key, &cfgs)
                    && let Some(target_override) = value.as_table()
                {
                    for (key, value) in target_override {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        if let Some(target_override) = root
            .get("overrides")
            .and_then(|o| o.get(target))
            .and_then(toml::Value::as_table)
        {
            for (key, value) in target_override {
                merged.insert(key.clone(), value.clone());
            }
        }

        // A declared key is honored or the artifact is not used: a table this
        // installer cannot read is an error, never "unsigned".
        let signing = merged
            .get("signing")
            .map(|signing| {
                signing
                    .clone()
                    .try_into::<NativeBinstallSigning>()
                    .wrap_err("invalid package.metadata.binstall.signing")
            })
            .transpose()?;
        if let Some(signing) = &signing
            && signing.algorithm != "minisign"
        {
            bail!(
                "unsupported package.metadata.binstall.signing algorithm: {}",
                signing.algorithm
            );
        }

        Ok(Self {
            pkg_url: merged
                .get("pkg-url")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            bin_dir: merged
                .get("bin-dir")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            pkg_fmt: merged
                .get("pkg-fmt")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            disabled_strategies: merged
                .get("disabled-strategies")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect(),
            signing,
        })
    }
}

const DEFAULT_PACKAGE_EXTENSIONS: &[(&str, &str)] = &[
    ("tar", ".tar"),
    ("tbz2", ".tbz2"),
    ("tbz2", ".tar.bz2"),
    ("tbz2", ".tbz"),
    ("tbz2", ".tar.bz"),
    ("tgz", ".tgz"),
    ("tgz", ".tar.gz"),
    ("txz", ".txz"),
    ("txz", ".tar.xz"),
    ("tzstd", ".tzstd"),
    ("tzstd", ".tzst"),
    ("tzstd", ".tar.zst"),
    ("zip", ".zip"),
    ("bin", ".bin"),
    ("bin", ""),
    ("bin", ".exe"),
];

/// The environment of the `cargo install` that `execute_install_command` runs:
/// the process environment, overridden by the toolset's and then by the
/// tool's `install_env`.
async fn cargo_install_env(
    ctx: &InstallContext,
    tv: &ToolVersion,
) -> Result<BTreeMap<String, String>> {
    let mut cargo_env: BTreeMap<String, String> = env::vars_safe().collect();
    cargo_env.extend(ctx.ts.env_with_path_without_tools(&ctx.config).await?);
    for (key, value) in tv.install_env() {
        match value.into_string() {
            Some(value) => cargo_env.insert(key, value),
            None => cargo_env.remove(&key),
        };
    }
    Ok(cargo_env)
}

/// Whether `cargo install`, run with `cargo_env` in `cwd`, can take a crate
/// from somewhere other than crates.io: a default registry, a
/// `[source.crates-io]` table that replaces crates.io, or the legacy
/// `registry.index`.
///
/// `cargo install` reads its configuration from `$CARGO_HOME` and the
/// `.cargo` directories above it, not from the working directory. A file that
/// cannot be parsed, or that includes other files, counts as naming another
/// registry, since `cargo install` is the answer that is right either way.
fn cargo_config_names_other_registry(cargo_env: &BTreeMap<String, String>, cwd: &Path) -> bool {
    const CRATES_IO: &str = "crates-io";
    let var = |key: &str| cargo_env.get(key).filter(|value| !value.is_empty());
    if var("CARGO_REGISTRY_INDEX").is_some()
        || var("CARGO_REGISTRY_DEFAULT").is_some_and(|registry| registry != CRATES_IO)
    {
        return true;
    }
    let cargo_home = match var("CARGO_HOME") {
        Some(cargo_home) => cwd.join(cargo_home),
        None => var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map_or_else(|| dirs::HOME.to_path_buf(), PathBuf::from)
            .join(".cargo"),
    };
    std::iter::once(cargo_home.clone())
        .chain(cargo_home.ancestors().map(|dir| dir.join(".cargo")))
        .flat_map(|dir| [dir.join("config.toml"), dir.join("config")])
        .filter(|path| path.is_file())
        .any(|path| {
            let Some(config) = fs::read_to_string(&path)
                .ok()
                .and_then(|body| body.parse::<toml::Table>().ok())
            else {
                return true;
            };
            config.contains_key("include")
                || config.get("registry").is_some_and(|registry| {
                    registry.get("index").is_some()
                        || registry
                            .get("default")
                            .is_some_and(|default| default.as_str() != Some(CRATES_IO))
                })
                || config
                    .get("source")
                    .is_some_and(|source| source.get(CRATES_IO).is_some())
        })
}

async fn discover_native_binstall_metadata(
    package: &CratesIoPackage,
    crate_name: &str,
    version: &str,
    target: &str,
) -> Option<NativeBinstallMetadata> {
    let repository = package.repository.as_deref()?;
    let repo = github_repo_from_repository(repository)?;
    let tags = [
        version.to_string(),
        format!("v{version}"),
        format!("{crate_name}/{version}"),
        format!("{crate_name}/v{version}"),
    ];

    for tag in tags {
        let release = match crate::github::get_release(&repo, &tag).await {
            Ok(release) => release,
            Err(err) => {
                debug!("failed to inspect GitHub release {repo}@{tag}: {err:#}");
                continue;
            }
        };
        if let Some((asset_url, pkg_fmt)) =
            select_default_github_asset(&release.assets, crate_name, version, target)
        {
            return Some(NativeBinstallMetadata {
                pkg_url: Some(asset_url),
                pkg_fmt: Some(pkg_fmt),
                ..Default::default()
            });
        }
    }
    None
}

fn github_repo_from_repository(repository: &str) -> Option<String> {
    let repository = repository.strip_prefix("git+").unwrap_or(repository);
    let url = Url::parse(repository).ok()?;
    if url.host_str()? != "github.com" {
        return None;
    }
    let mut segments = url.path_segments()?.filter(|segment| !segment.is_empty());
    let owner = segments.next()?;
    let repo = segments.next()?.trim_end_matches(".git");
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

fn select_default_github_asset(
    assets: &[crate::github::GithubAsset],
    crate_name: &str,
    version: &str,
    target: &str,
) -> Option<(String, String)> {
    let stems = [
        format!("{crate_name}-{target}-v{version}"),
        format!("{crate_name}-{target}-{version}"),
        format!("{crate_name}-{version}-{target}"),
        format!("{crate_name}-v{version}-{target}"),
        format!("{crate_name}_{target}_v{version}"),
        format!("{crate_name}_{target}_{version}"),
        format!("{crate_name}_{version}_{target}"),
        format!("{crate_name}_v{version}_{target}"),
        format!("{crate_name}-{target}"),
        format!("{crate_name}_{target}"),
    ];
    for stem in stems {
        for (pkg_fmt, extension) in DEFAULT_PACKAGE_EXTENSIONS {
            let candidate = format!("{stem}{extension}");
            if let Some(asset) = assets.iter().find(|asset| asset.name == candidate) {
                return Some((asset.browser_download_url.clone(), (*pkg_fmt).to_string()));
            }
        }
    }
    None
}

fn native_manifest_bins(manifest: &toml::Value) -> Vec<NativeCargoBin> {
    manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|bin| {
            let bin = bin.as_table()?;
            let name = bin.get("name").and_then(toml::Value::as_str)?.to_string();
            let required_features = bin
                .get("required-features")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect();
            Some(NativeCargoBin {
                name,
                required_features,
            })
        })
        .collect()
}

fn native_bins_to_install(
    requested_bin: Option<String>,
    manifest_bins: &[NativeCargoBin],
    package: &CratesIoPackage,
    crate_name: &str,
) -> Vec<NativeCargoBin> {
    if let Some(requested_bin) = requested_bin {
        let bin = manifest_bins
            .iter()
            .find(|bin| bin.name == requested_bin)
            .cloned()
            .unwrap_or(NativeCargoBin {
                name: requested_bin,
                required_features: vec![],
            });
        return vec![bin];
    }
    if !manifest_bins.is_empty() {
        return manifest_bins.to_vec();
    }
    if !package.bin_names.is_empty() {
        return package
            .bin_names
            .iter()
            .map(|name| NativeCargoBin {
                name: name.clone(),
                required_features: vec![],
            })
            .collect();
    }
    vec![NativeCargoBin {
        name: crate_name.to_string(),
        required_features: vec![],
    }]
}

struct BinstallTemplateVars<'a> {
    repo: &'a str,
    name: &'a str,
    version: &'a str,
    target: &'a str,
    archive_format: &'a str,
    archive_suffix: &'a str,
    bin: &'a str,
    binary_ext: &'a str,
}

struct BinstallTemplateContext<'a> {
    package: &'a CratesIoPackage,
    crate_name: &'a str,
    version: &'a str,
    target: &'a str,
    archive_format: &'a str,
    archive_suffix: &'a str,
    binary_ext: &'a str,
}

impl<'a> BinstallTemplateContext<'a> {
    fn vars(&self, bin: &'a str) -> BinstallTemplateVars<'a> {
        BinstallTemplateVars {
            repo: self.package.repository.as_deref().unwrap_or_default(),
            name: self.crate_name,
            version: self.version,
            target: self.target,
            archive_format: self.archive_format,
            archive_suffix: self.archive_suffix,
            bin,
            binary_ext: self.binary_ext,
        }
    }
}

async fn download_crate_manifest(
    package: &CratesIoPackage,
    tv: &ToolVersion,
    ctx: &InstallContext,
) -> Result<toml::Value> {
    // The name and version come from the crates.io response, so they must not
    // be able to point the download outside the download directory.
    let crate_file_name = format!("{}-{}.crate", package.name, package.num);
    if !file::is_plain_file_name(&crate_file_name) {
        bail!("crates.io returned an unusable crate file name: {crate_file_name:?}");
    }
    let crate_path = tv.download_path().join(crate_file_name);
    ctx.pr.set_message(format!(
        "download {}",
        crate_path.file_name().unwrap().to_string_lossy()
    ));
    HTTP.download_file(package.download_url(), &crate_path, Some(ctx.pr.as_ref()))
        .await?;
    read_crate_manifest(&crate_path, package, Some(ctx.pr.as_ref()))
}

/// Verifies a downloaded `.crate`, unpacks it and parses its `Cargo.toml`.
fn read_crate_manifest(
    crate_path: &Path,
    package: &CratesIoPackage,
    pr: Option<&dyn SingleReport>,
) -> Result<toml::Value> {
    // crates.io redirects the download to another host, and the manifest in
    // the file chooses the binary that gets installed, so like cargo only the
    // file whose sha256 crates.io published is read.
    hash::ensure_checksum(crate_path, &package.checksum, pr, "sha256")?;

    let temp_dir = tempfile::Builder::new()
        .prefix("mise-cargo-crate-")
        .tempdir()?;
    file::extract_archive(
        crate_path,
        temp_dir.path(),
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr,
            preserve_mtime: true,
        },
    )?;
    // The manifest is read by path, so a link shipped in the crate (as the
    // file or as its directory) would make it read a file outside the crate.
    file::ensure_symlinks_stay_within(temp_dir.path())?;

    let manifest_path = temp_dir
        .path()
        .join(format!("{}-{}/Cargo.toml", package.name, package.num));
    let manifest = read_archive_metadata(&manifest_path)?;
    Ok(toml::from_str(&manifest)?)
}

/// The longest signature file mise downloads. A minisign signature is a few
/// hundred bytes.
const MAX_SIGNATURE_LEN: u64 = 64 * 1024;

/// Downloads the artifact at `url` to `path`. With a `signature`, the artifact
/// is checked against the crate's key before this returns, so nothing is
/// extracted or copied from a file the key does not sign. The key comes from
/// the crate on crates.io and the artifact from wherever `pkg-url` points.
async fn download_native_binstall_file(
    url: &str,
    path: &Path,
    signature: Option<&NativeBinstallSignature<'_>>,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    let download_url = resolve_native_binstall_download_url(url).await;
    HTTP.download_file(&download_url, path, pr).await?;
    if let Some(signature) = signature
        && let Err(err) = verify_native_binstall_signature(path, signature).await
    {
        // The caller builds from source instead and logs its reason only at
        // debug level, so the rejected artifact is named here.
        warn!("not using native cargo binary artifact {url}: {err:#}");
        return Err(err);
    }
    Ok(())
}

async fn verify_native_binstall_signature(
    path: &Path,
    signature: &NativeBinstallSignature<'_>,
) -> Result<()> {
    let download_url = resolve_native_binstall_download_url(&signature.url).await;
    let mut sig_path = path.as_os_str().to_owned();
    sig_path.push(".sig");
    let sig_path = PathBuf::from(sig_path);
    HTTP.download_file_limited(&download_url, &sig_path, MAX_SIGNATURE_LEN, None)
        .await
        .wrap_err_with(|| format!("failed to download signature {}", signature.url))?;
    let sig = file::read_to_string(&sig_path)?;
    minisign::verify(signature.pubkey, &file::read(path)?, &sig)
        .wrap_err("minisign signature verification failed")
}

/// The signature `signing` requires for the artifact at `artifact_url`: its
/// `file` template, `{ url }.sig` by default, expanded with the artifact's
/// variables and `{ url }`, and held to https like the artifact.
fn native_binstall_signature<'a>(
    signing: Option<&'a NativeBinstallSigning>,
    artifact_url: &str,
    vars: &BinstallTemplateVars<'_>,
) -> Result<Option<NativeBinstallSignature<'a>>> {
    let Some(signing) = signing else {
        return Ok(None);
    };
    let template = signing.file.as_deref().unwrap_or("{ url }.sig");
    let mut replacements = binstall_template_replacements(vars).to_vec();
    replacements.push(("url", artifact_url));
    let url = expand_template(template, &replacements)?;
    Ok(Some(NativeBinstallSignature {
        url: require_https(url, "signature")?,
        pubkey: &signing.pubkey,
    }))
}

async fn native_binstall_file_available(url: &str) -> bool {
    if let Some((repo, tag, asset_name)) = crate::github::release_asset_from_url(url) {
        return match crate::github::get_release(&repo, &tag).await {
            Ok(release) => release.assets.iter().any(|asset| asset.name == asset_name),
            Err(err) => {
                debug!("failed to inspect GitHub release {repo}@{tag}/{asset_name}: {err:#}");
                false
            }
        };
    }
    if HTTP.head(url).await.is_ok() {
        return true;
    }
    match HTTP.get_async(url).await {
        Ok(_) => true,
        Err(err) => {
            debug!("native cargo binary artifact is unavailable at {url}: {err:#}");
            false
        }
    }
}

async fn native_binstall_files_available(urls: &[String]) -> bool {
    for url in urls {
        if !native_binstall_file_available(url).await {
            return false;
        }
    }
    true
}

async fn resolve_native_binstall_download_url(url: &str) -> String {
    // The versions host stays in play here: a cargo package's release is public
    // by definition, so the cached copy is both usable and cheaper.
    match crate::github::release_asset_api_url(url, true).await {
        Some(api_url) => crate::github::pick_reachable_asset_url(url, &api_url).await,
        None => url.to_string(),
    }
}

#[derive(Debug)]
struct NativeTargetCfg {
    os: String,
    arch: String,
    env: Option<String>,
    family: String,
}

fn native_target_cfgs(platform: &PlatformTarget, triple: &str) -> NativeTargetCfg {
    let os = match platform.os_name() {
        "macos" => "macos",
        "windows" => "windows",
        os => os,
    }
    .to_string();
    let arch = match platform.arch_name() {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        arch => arch,
    }
    .to_string();
    let env = if triple.contains("-musl") {
        Some("musl".to_string())
    } else if triple.contains("-gnu") || triple.contains("-gnueabihf") {
        Some("gnu".to_string())
    } else if triple.contains("-msvc") {
        Some("msvc".to_string())
    } else {
        None
    };
    let family = if os == "windows" { "windows" } else { "unix" }.to_string();
    NativeTargetCfg {
        os,
        arch,
        env,
        family,
    }
}

fn native_cfg_override_matches(key: &str, cfgs: &NativeTargetCfg) -> bool {
    let Some(expr) = key
        .strip_prefix("cfg(")
        .and_then(|key| key.strip_suffix(')'))
    else {
        return false;
    };
    native_cfg_expr_matches(expr.trim(), cfgs)
}

fn native_cfg_expr_matches(expr: &str, cfgs: &NativeTargetCfg) -> bool {
    let expr = expr.trim();
    if expr == "unix" {
        return cfgs.family == "unix";
    }
    if expr == "windows" {
        return cfgs.family == "windows";
    }
    if let Some(inner) = expr
        .strip_prefix("not(")
        .and_then(|expr| expr.strip_suffix(')'))
    {
        return !native_cfg_expr_matches(inner, cfgs);
    }
    if let Some(inner) = expr
        .strip_prefix("all(")
        .and_then(|expr| expr.strip_suffix(')'))
    {
        let parts = split_native_cfg_args(inner);
        return !parts.is_empty() && parts.iter().all(|part| native_cfg_expr_matches(part, cfgs));
    }
    if let Some(inner) = expr
        .strip_prefix("any(")
        .and_then(|expr| expr.strip_suffix(')'))
    {
        return split_native_cfg_args(inner)
            .iter()
            .any(|part| native_cfg_expr_matches(part, cfgs));
    }
    if let Some((key, value)) = expr.split_once('=') {
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        return match key {
            "target_os" => cfgs.os == value,
            "target_arch" => cfgs.arch == value,
            "target_env" => cfgs.env.as_deref() == Some(value),
            "target_family" => cfgs.family == value,
            _ => false,
        };
    }
    false
}

fn split_native_cfg_args(args: &str) -> Vec<&str> {
    let mut parts = vec![];
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in args.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(args[start..].trim());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn cargo_target_triple(target: &PlatformTarget) -> Option<String> {
    let libc = target.libc().unwrap_or("gnu");
    match (target.os_name(), target.arch_name(), libc) {
        ("macos", "x64", _) => Some("x86_64-apple-darwin".to_string()),
        ("macos", "arm64", _) => Some("aarch64-apple-darwin".to_string()),
        ("windows", "x64", _) => Some("x86_64-pc-windows-msvc".to_string()),
        ("windows", "arm64", _) => Some("aarch64-pc-windows-msvc".to_string()),
        ("linux", "x64", "musl") => Some("x86_64-unknown-linux-musl".to_string()),
        ("linux", "x64", _) => Some("x86_64-unknown-linux-gnu".to_string()),
        ("linux", "arm64", "musl") => Some("aarch64-unknown-linux-musl".to_string()),
        ("linux", "arm64", _) => Some("aarch64-unknown-linux-gnu".to_string()),
        ("linux", "armv7", _) => Some("armv7-unknown-linux-gnueabihf".to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct NativePackageFormat<'a> {
    template_value: &'a str,
    extraction_format: Option<ExtractionFormat>,
}

fn native_package_format(pkg_fmt: &str) -> Result<NativePackageFormat<'_>> {
    match pkg_fmt {
        "tgz" | "tar.gz" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: Some(ExtractionFormat::TarGz),
        }),
        "txz" | "tar.xz" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: Some(ExtractionFormat::TarXz),
        }),
        "tbz" | "tbz2" | "tar.bz2" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: Some(ExtractionFormat::TarBz2),
        }),
        "tzst" | "tzstd" | "tar.zst" | "tar.zstd" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: Some(ExtractionFormat::TarZst),
        }),
        "zip" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: Some(ExtractionFormat::Zip),
        }),
        "tar" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: Some(ExtractionFormat::Tar),
        }),
        "bin" => Ok(NativePackageFormat {
            template_value: pkg_fmt,
            extraction_format: None,
        }),
        other => Err(eyre!(
            "unsupported native cargo binary package format: {other}"
        )),
    }
}

/// Expand `pkg-url` into the URL of the artifact to download.
///
/// The artifact is installed as an executable with no checksum, so TLS is all
/// that authenticates it, and a URL that is not `https` is refused. The crate
/// metadata the URL is made from, usually `{ repo }` (the crate's
/// `repository`), was read over https. This is the URL before
/// `url_replacements`, which still apply to it.
fn native_binstall_artifact_url(pkg_url: &str, vars: &BinstallTemplateVars<'_>) -> Result<String> {
    require_https(expand_binstall_template(pkg_url, vars)?, "artifact")
}

fn require_https(url: String, what: &str) -> Result<String> {
    let parsed = Url::parse(&url)
        .wrap_err_with(|| format!("native cargo binary {what} URL is not valid: {url}"))?;
    if parsed.scheme() != "https" {
        bail!("native cargo binary {what} must be downloaded over https: {url}");
    }
    Ok(url)
}

/// The longest a `pkg-url` or `bin-dir` template may expand to. A URL or a
/// path inside an archive needs a few hundred bytes.
const MAX_EXPANDED_TEMPLATE_LEN: usize = 8 * 1024;

/// Expand the `{ key }` and `{key}` placeholders of a `pkg-url` or `bin-dir`
/// template in one pass: a substituted value is copied as it is, never
/// searched for placeholders itself. An expansion longer than
/// `MAX_EXPANDED_TEMPLATE_LEN` is refused when it gets there, so the crate
/// metadata cannot make mise build a string any longer.
fn expand_binstall_template(template: &str, vars: &BinstallTemplateVars<'_>) -> Result<String> {
    expand_template(template, &binstall_template_replacements(vars))
}

fn binstall_template_replacements<'a>(
    vars: &BinstallTemplateVars<'a>,
) -> [(&'static str, &'a str); 8] {
    [
        ("repo", vars.repo),
        ("name", vars.name),
        ("version", vars.version),
        ("target", vars.target),
        ("archive-format", vars.archive_format),
        ("archive-suffix", vars.archive_suffix),
        ("bin", vars.bin),
        ("binary-ext", vars.binary_ext),
    ]
}

fn expand_template(template: &str, replacements: &[(&str, &str)]) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while !rest.is_empty() {
        let (piece, consumed) = rest
            .strip_prefix('{')
            .and_then(|after| {
                let (inner, _) = after.split_once('}')?;
                let key = inner
                    .strip_prefix(' ')
                    .and_then(|key| key.strip_suffix(' '))
                    .unwrap_or(inner);
                let (_, value) = replacements.iter().find(|(name, _)| *name == key)?;
                Some((*value, inner.len() + 2))
            })
            .unwrap_or_else(|| {
                // up to the next `{` after this one, which is a single byte
                let skip = usize::from(rest.starts_with('{'));
                let literal = rest[skip..].find('{').map_or(rest.len(), |at| at + skip);
                (&rest[..literal], literal)
            });
        if out.len() + piece.len() > MAX_EXPANDED_TEMPLATE_LEN {
            bail!(
                "native cargo binary template expands to more than {MAX_EXPANDED_TEMPLATE_LEN} bytes"
            );
        }
        out.push_str(piece);
        rest = &rest[consumed..];
    }
    Ok(out)
}

fn template_contains_var(template: &str, key: &str) -> bool {
    template.contains(&format!("{{ {key} }}")) || template.contains(&format!("{{{key}}}"))
}

pub(super) fn rollout_warning_active() -> bool {
    rollout_warning_active_for(&crate::version::V)
}

fn rollout_warning_active_for(current: &versions::Versioning) -> bool {
    use versions::Versioning;

    debug_assert!(
        *current < Versioning::new(DEFAULT_AT).unwrap(),
        "native cargo binary installs should be the default now; make cargo.binstall_native a two-way switch"
    );
    *current >= Versioning::new(WARN_AT).unwrap()
}

fn warn_native_binstall_rollout() {
    warn_once!(
        "mise will install cargo packages from native precompiled binary artifacts by default in {DEFAULT_AT} when cargo-binstall is unavailable.\n\
         To use native cargo binaries now: mise settings cargo.binstall_native=true\n\
         To keep using cargo install: mise settings cargo.binstall_native=false"
    );
}

fn raw_binary_url_supports_bins(template: &str, bins: &[String]) -> bool {
    bins.len() <= 1 || template_contains_var(template, "bin")
}

fn find_native_bin(
    extract_dir: &Path,
    bin_dir: Option<&str>,
    vars: &BinstallTemplateVars<'_>,
) -> Result<Option<PathBuf>> {
    let explicit_bin_dir = bin_dir.is_some();
    let bin_dir = bin_dir
        .map(str::to_string)
        .unwrap_or_else(|| infer_native_bin_dir_template(extract_dir, vars));
    let rendered = expand_binstall_template(&bin_dir, vars)?;
    let relative_path = validate_native_bin_relative_path(&rendered)?;
    let candidate = extract_dir.join(relative_path);
    if explicit_bin_dir && !template_contains_var(&bin_dir, "bin") {
        let file_name = format!("{}{}", vars.bin, vars.binary_ext);
        let names_requested_bin = candidate
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .is_some_and(|candidate_name| candidate_name == file_name);
        if !names_requested_bin {
            return Ok(None);
        }
    }
    Ok(candidate.is_file().then_some(candidate))
}

fn infer_native_bin_dir_template(extract_dir: &Path, vars: &BinstallTemplateVars<'_>) -> String {
    let candidates = [
        format!("{}-{}-v{}", vars.name, vars.target, vars.version),
        format!("{}-{}-{}", vars.name, vars.target, vars.version),
        format!("{}-{}-{}", vars.name, vars.version, vars.target),
        format!("{}-v{}-{}", vars.name, vars.version, vars.target),
        format!("{}-{}", vars.name, vars.target),
        format!("{}-{}", vars.name, vars.version),
        format!("{}-v{}", vars.name, vars.version),
        vars.name.to_string(),
    ];
    for candidate in candidates {
        if extract_dir.join(&candidate).is_dir() {
            return format!("{candidate}/{{ bin }}{{ binary-ext }}");
        }
    }
    "{ bin }{ binary-ext }".to_string()
}

fn validate_native_bin_relative_path(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    if path.components().next().is_none() {
        bail!("native cargo binary path is empty");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) {
        bail!(
            "native cargo binary path must stay inside the artifact: {}",
            path.display()
        );
    }
    Ok(path)
}

/// Bin names come from the crate's manifest, the crates.io `bin_names` list or
/// the `bin` tool option, and are joined onto the download and install
/// directories, so each one must be a single plain file name.
fn validate_native_bin_names(bins: &[NativeCargoBin]) -> Result<()> {
    for bin in bins {
        if !file::is_plain_file_name(&bin.name) {
            bail!(
                "native cargo binary name must be a plain file name: {:?}",
                bin.name
            );
        }
    }
    Ok(())
}

fn validate_native_bin_sources(pending: &[(PathBuf, PathBuf)]) -> Result<()> {
    let mut sources = BTreeSet::new();
    for (src, _) in pending {
        if !sources.insert(src) {
            bail!(
                "native cargo binary artifact maps multiple bins to {}",
                file::display_path(src)
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;

    #[test]
    fn native_binstall_metadata_merges_target_overrides() {
        let manifest = toml::Value::Table(toml::toml! {
            [package.metadata.binstall]
            pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }.{ archive-format }"
            bin-dir = "{ bin }{ binary-ext }"
            pkg-fmt = "tgz"

            [package.metadata.binstall.overrides.x86_64-apple-darwin]
            pkg-fmt = "zip"
        });

        let metadata = NativeBinstallMetadata::from_manifest(
            &manifest,
            "x86_64-apple-darwin",
            &PlatformTarget::new(Platform::parse("macos-x64").unwrap()),
        )
        .unwrap();

        assert_eq!(metadata.pkg_fmt.as_deref(), Some("zip"));
        assert_eq!(metadata.bin_dir.as_deref(), Some("{ bin }{ binary-ext }"));
    }

    #[test]
    fn native_binstall_metadata_merges_cfg_overrides() {
        let manifest: toml::Value = toml::from_str(
            r#"
            [package.metadata.binstall]
            pkg-url = "base"
            pkg-fmt = "tgz"
    
            [package.metadata.binstall.overrides.'cfg(unix)']
            pkg-fmt = "tar"
    
            [package.metadata.binstall.overrides.'cfg(all(target_os = "linux", target_env = "musl"))']
            pkg-url = "musl"
            "#,
        )
        .unwrap();

        let metadata = NativeBinstallMetadata::from_manifest(
            &manifest,
            "x86_64-unknown-linux-musl",
            &PlatformTarget::new(Platform::parse("linux-x64-musl").unwrap()),
        )
        .unwrap();

        assert_eq!(metadata.pkg_url.as_deref(), Some("musl"));
        assert_eq!(metadata.pkg_fmt.as_deref(), Some("tar"));
    }

    #[test]
    fn native_binstall_metadata_exact_override_wins_over_cfg() {
        let manifest: toml::Value = toml::from_str(
            r#"
            [package.metadata.binstall]
            pkg-url = "base"
            pkg-fmt = "tgz"
    
            [package.metadata.binstall.overrides.'cfg(unix)']
            pkg-fmt = "tar"
    
            [package.metadata.binstall.overrides.x86_64-unknown-linux-gnu]
            pkg-fmt = "zip"
            "#,
        )
        .unwrap();

        let metadata = NativeBinstallMetadata::from_manifest(
            &manifest,
            "x86_64-unknown-linux-gnu",
            &PlatformTarget::new(Platform::parse("linux-x64").unwrap()),
        )
        .unwrap();

        assert_eq!(metadata.pkg_fmt.as_deref(), Some("zip"));
    }

    #[test]
    fn native_binstall_metadata_is_optional() {
        let manifest = toml::Value::Table(toml::toml! {
            [package]
            name = "demo"
        });

        let metadata = NativeBinstallMetadata::from_manifest(
            &manifest,
            "x86_64-unknown-linux-gnu",
            &PlatformTarget::new(Platform::parse("linux-x64").unwrap()),
        )
        .unwrap();

        assert_eq!(metadata.pkg_url, None);
    }

    #[test]
    fn native_binstall_metadata_reads_signing() {
        let platform = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let parse = |manifest: &str| {
            NativeBinstallMetadata::from_manifest(
                &toml::from_str(manifest).unwrap(),
                "x86_64-unknown-linux-gnu",
                &platform,
            )
        };

        let metadata = parse(
            r#"
            [package.metadata.binstall]
            pkg-url = "base"

            [package.metadata.binstall.signing]
            algorithm = "minisign"
            pubkey = "base-key"

            [package.metadata.binstall.overrides.x86_64-unknown-linux-gnu.signing]
            algorithm = "minisign"
            pubkey = "linux-key"
            file = "{ url }.minisig"
            "#,
        )
        .unwrap();
        assert_eq!(
            metadata.signing,
            Some(NativeBinstallSigning {
                algorithm: "minisign".to_string(),
                pubkey: "linux-key".to_string(),
                file: Some("{ url }.minisig".to_string()),
            })
        );
        let unsigned = parse("[package.metadata.binstall]\npkg-url = \"base\"\n").unwrap();
        assert_eq!(unsigned.signing, None);

        // a key that can't be honored is an error, never "unsigned"
        for signing in [
            "signing = \"minisign\"",
            "signing = { algorithm = \"minisign\" }",
            "signing = { pubkey = \"key\" }",
            "signing = { algorithm = \"gpg\", pubkey = \"key\" }",
        ] {
            let manifest = format!("[package.metadata.binstall]\npkg-url = \"base\"\n{signing}\n");
            assert!(
                parse(&manifest).is_err(),
                "expected {signing} to be rejected"
            );
        }
    }

    #[test]
    fn native_binstall_signature_follows_the_file_template() {
        let vars = BinstallTemplateVars {
            repo: "https://example.com/demo",
            name: "demo",
            version: "1.2.3",
            target: "x86_64-unknown-linux-gnu",
            archive_format: "tgz",
            archive_suffix: ".tgz",
            bin: "demo",
            binary_ext: "",
        };
        let signing = |file: Option<&str>| NativeBinstallSigning {
            algorithm: "minisign".to_string(),
            pubkey: "key".to_string(),
            file: file.map(str::to_string),
        };
        let url = "https://example.com/demo-1.2.3.tgz";
        let signature_url = |file| {
            native_binstall_signature(Some(&signing(file)), url, &vars)
                .map(|signature| signature.unwrap().url)
        };

        assert_eq!(
            signature_url(None).unwrap(),
            "https://example.com/demo-1.2.3.tgz.sig"
        );
        assert_eq!(
            signature_url(Some("{url}.minisig")).unwrap(),
            "https://example.com/demo-1.2.3.tgz.minisig"
        );
        assert_eq!(
            signature_url(Some("{ repo }/sigs/{ name }-{ version }.sig")).unwrap(),
            "https://example.com/demo/sigs/demo-1.2.3.sig"
        );
        assert!(signature_url(Some("http://example.com/{ name }.sig")).is_err());
        assert!(
            native_binstall_signature(None, url, &vars)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn signed_artifact_must_verify_against_the_declared_key() {
        const PUBLISHED: &str =
            "#!/bin/sh\necho \"foo 1.0.0, as built and signed by its author\"\n";
        const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\n\
            RUTwDwDwDwDwARyVSE7SSrknx0xQT1OVt7qB4OUuP6yYJsFbFLXMjD5qK8Y5Z+KKZNUvUqM6iU50Xf0KIOthW7Zaos+CwuzLEwY=\n\
            trusted comment: foo 1.0.0\n\
            trpNvGXNiB3XysqMmEIg0xKNCHwRF3R5QcWruJwldwiBfJkkuoNlb8TOuj2m2y3oLMJwfHvGFOtci1/6bb4UDQ==\n";
        const PUBKEY: &str = "RWTwDwDwDwDwAfkBh4MnQJnodUaPZgCBWQlrseqOePeaoIGoLkN1WGyE";

        let mut server = mockito::Server::new_async().await;
        for (path, body) in [
            ("/published", PUBLISHED),
            ("/published.sig", SIGNATURE),
            ("/tampered", "#!/bin/sh\necho tampered\n"),
            ("/tampered.sig", SIGNATURE),
            ("/unsigned", PUBLISHED),
        ] {
            server
                .mock("GET", path)
                .with_status(200)
                .with_body(body)
                .create_async()
                .await;
        }
        server
            .mock("GET", "/unsigned.sig")
            .with_status(404)
            .create_async()
            .await;

        let temp_dir = tempfile::tempdir().unwrap();
        let download = async |name: &str, signed: bool| {
            let url = format!("{}/{name}", server.url());
            let signature = NativeBinstallSignature {
                url: format!("{url}.sig"),
                pubkey: PUBKEY,
            };
            download_native_binstall_file(
                &url,
                &temp_dir.path().join(name),
                signed.then_some(&signature),
                None,
            )
            .await
        };

        download("published", true).await.unwrap();
        assert!(download("tampered", true).await.is_err());
        assert!(download("unsigned", true).await.is_err());
        // a crate that declares no key has nothing to verify
        download("tampered", false).await.unwrap();
    }

    #[test]
    fn github_repository_and_conventional_asset_are_discovered() {
        assert_eq!(
            github_repo_from_repository("git+https://github.com/example/demo.git"),
            Some("example/demo".to_string())
        );
        let assets = vec![crate::github::GithubAsset {
            from_versions_host: false,
            name: "demo-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            browser_download_url: "https://github.com/example/demo/releases/download/v1.2.3/demo-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            url: "https://api.github.com/repos/example/demo/releases/assets/1".to_string(),
            digest: None,
            updated_at: None,
        }];

        let discovered =
            select_default_github_asset(&assets, "demo", "1.2.3", "x86_64-unknown-linux-gnu");

        assert_eq!(
            discovered,
            Some((assets[0].browser_download_url.clone(), "tgz".to_string()))
        );
    }

    #[tokio::test]
    async fn warn_only_artifact_probe_requires_an_existing_file() {
        let mut server = mockito::Server::new_async().await;
        let available = server
            .mock("HEAD", "/available.tgz")
            .with_status(200)
            .create_async()
            .await;
        let head_rejected = server
            .mock("HEAD", "/head-rejected.tgz")
            .with_status(405)
            .create_async()
            .await;
        let get_available = server
            .mock("GET", "/head-rejected.tgz")
            .with_status(200)
            .create_async()
            .await;
        let unavailable_head = server
            .mock("HEAD", "/unavailable.tgz")
            .with_status(404)
            .create_async()
            .await;
        let unavailable_get = server
            .mock("GET", "/unavailable.tgz")
            .with_status(404)
            .create_async()
            .await;

        assert!(native_binstall_file_available(&format!("{}/available.tgz", server.url())).await);
        assert!(
            native_binstall_file_available(&format!("{}/head-rejected.tgz", server.url())).await
        );
        assert!(
            !native_binstall_file_available(&format!("{}/unavailable.tgz", server.url())).await
        );
        available.assert_async().await;
        head_rejected.assert_async().await;
        get_available.assert_async().await;
        unavailable_head.assert_async().await;
        unavailable_get.assert_async().await;
    }

    #[tokio::test]
    async fn warn_only_artifact_probe_checks_every_archive() {
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("HEAD", "/first.tar.gz")
            .with_status(200)
            .create_async()
            .await;
        let second_head = server
            .mock("HEAD", "/second.tar.gz")
            .with_status(404)
            .create_async()
            .await;
        let second_get = server
            .mock("GET", "/second.tar.gz")
            .with_status(404)
            .create_async()
            .await;
        let urls = [
            format!("{}/first.tar.gz", server.url()),
            format!("{}/second.tar.gz", server.url()),
        ];

        assert!(!native_binstall_files_available(&urls).await);
        first.assert_async().await;
        second_head.assert_async().await;
        second_get.assert_async().await;
    }

    #[test]
    fn native_manifest_bins_reads_required_features() {
        let manifest = toml::Value::Table(toml::toml! {
            [[bin]]
            name = "main"

            [[bin]]
            name = "extra"
            required-features = ["extras"]
        });

        assert_eq!(
            native_manifest_bins(&manifest),
            vec![
                NativeCargoBin {
                    name: "main".to_string(),
                    required_features: vec![]
                },
                NativeCargoBin {
                    name: "extra".to_string(),
                    required_features: vec!["extras".to_string()]
                }
            ]
        );
    }

    #[test]
    fn expand_binstall_template_handles_spaced_and_compact_vars() {
        let rendered = expand_binstall_template(
            "{ repo }/{name}-{ version }-{ target }{ archive-suffix }",
            &BinstallTemplateVars {
                repo: "https://example.com",
                name: "demo",
                version: "1.2.3",
                target: "x86_64-unknown-linux-gnu",
                archive_format: "tar.gz",
                archive_suffix: ".tar.gz",
                bin: "demo",
                binary_ext: "",
            },
        )
        .unwrap();

        assert_eq!(
            rendered,
            "https://example.com/demo-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn expand_binstall_template_is_bounded_and_single_pass() {
        let long_bin = "b".repeat(1024);
        let vars = |repo, bin| BinstallTemplateVars {
            repo,
            name: "demo",
            version: "1.2.3",
            target: "x86_64-unknown-linux-gnu",
            archive_format: "tgz",
            archive_suffix: ".tgz",
            bin,
            binary_ext: "",
        };

        // a value is not searched for placeholders, so `{bin}` in the
        // crate's `repository` stays as it is
        assert_eq!(
            expand_binstall_template("{ repo }/{bin}", &vars("{bin}{ bin }", "demo")).unwrap(),
            "{bin}{ bin }/demo"
        );
        // unknown, unclosed and unbalanced braces are copied, and so is text
        // that is not ASCII
        assert_eq!(
            expand_binstall_template("é{ x }{bin{{bin}}{ name", &vars("", "demo")).unwrap(),
            "é{ x }{bin{demo}{ name"
        );

        // exactly at the bound, in either spelling
        let at_bound = format!("{}{}", "{bin}".repeat(4), "{ bin }".repeat(4));
        assert_eq!(
            expand_binstall_template(&at_bound, &vars("", &long_bin))
                .unwrap()
                .len(),
            MAX_EXPANDED_TEMPLATE_LEN
        );
        // and one byte past it
        for template in [
            format!("{at_bound}x"),
            "{bin}".repeat(60_000),
            "x".repeat(MAX_EXPANDED_TEMPLATE_LEN + 1),
        ] {
            let err = expand_binstall_template(&template, &vars("", &long_bin)).unwrap_err();
            assert!(err.to_string().contains("more than"), "{err}");
        }
    }

    #[test]
    fn expand_binstall_template_honors_bin_dir_metadata_vars() {
        let rendered = expand_binstall_template(
            "{ name }-{ version }/{ bin }{ binary-ext }",
            &BinstallTemplateVars {
                repo: "https://example.com",
                name: "demo",
                version: "1.2.3",
                target: "x86_64-pc-windows-msvc",
                archive_format: "zip",
                archive_suffix: ".zip",
                bin: "demo-cli",
                binary_ext: ".exe",
            },
        )
        .unwrap();

        assert_eq!(rendered, "demo-1.2.3/demo-cli.exe");
    }

    #[test]
    fn native_package_format_handles_binstall_aliases() {
        assert_eq!(
            native_package_format("tgz").unwrap().extraction_format,
            Some(ExtractionFormat::TarGz)
        );
        assert_eq!(
            native_package_format("txz").unwrap().extraction_format,
            Some(ExtractionFormat::TarXz)
        );
        assert_eq!(
            native_package_format("tar.xz").unwrap().extraction_format,
            Some(ExtractionFormat::TarXz)
        );
        assert_eq!(
            native_package_format("tzstd").unwrap().extraction_format,
            Some(ExtractionFormat::TarZst)
        );
        assert_eq!(
            native_package_format("tar.zst").unwrap().extraction_format,
            Some(ExtractionFormat::TarZst)
        );
        assert_eq!(
            native_package_format("bin").unwrap().extraction_format,
            None
        );
        assert_eq!(native_package_format("tgz").unwrap().template_value, "tgz");
        assert!(native_package_format("pkg").is_err());
    }

    #[test]
    fn raw_binary_archive_suffix_uses_binary_extension() {
        let package_format = native_package_format("bin").unwrap();
        let binary_ext = ".exe";
        let archive_suffix = if package_format.extraction_format.is_some() {
            format!(".{}", package_format.template_value)
        } else {
            binary_ext.to_string()
        };

        assert_eq!(archive_suffix, ".exe");
    }

    #[test]
    fn raw_binary_url_requires_bin_placeholder_for_multiple_bins() {
        let bins = vec!["one".to_string(), "two".to_string()];
        assert!(!raw_binary_url_supports_bins(
            "https://example.com/{ name }",
            &bins
        ));
        assert!(raw_binary_url_supports_bins(
            "https://example.com/{ bin }",
            &bins
        ));
        assert!(raw_binary_url_supports_bins(
            "https://example.com/{ name }",
            &["one".to_string()]
        ));
    }

    #[test]
    fn native_binstall_rollout_warning_dates_match_rollout() {
        use versions::Versioning;

        let before_warning = Versioning::new("2026.12.0").unwrap();
        let warning_starts = Versioning::new(WARN_AT).unwrap();

        assert!(!rollout_warning_active_for(&before_warning));
        assert!(rollout_warning_active_for(&warning_starts));
    }

    #[test]
    fn fixed_bin_dir_only_matches_requested_binary() {
        let temp_dir = tempfile::tempdir().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        file::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("actual"), "").unwrap();

        let vars = BinstallTemplateVars {
            repo: "",
            name: "demo",
            version: "1.2.3",
            target: "x86_64-unknown-linux-gnu",
            archive_format: "tgz",
            archive_suffix: ".tgz",
            bin: "expected",
            binary_ext: "",
        };

        assert!(
            find_native_bin(temp_dir.path(), Some("bin/actual"), &vars)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn infers_native_bin_dir_from_common_archive_layout() {
        let temp_dir = tempfile::tempdir().unwrap();
        let artifact_dir = temp_dir.path().join("demo-x86_64-unknown-linux-gnu-v1.2.3");
        file::create_dir_all(&artifact_dir).unwrap();
        fs::write(artifact_dir.join("demo"), "").unwrap();
        let vars = BinstallTemplateVars {
            repo: "",
            name: "demo",
            version: "1.2.3",
            target: "x86_64-unknown-linux-gnu",
            archive_format: "tgz",
            archive_suffix: ".tgz",
            bin: "demo",
            binary_ext: "",
        };

        let src = find_native_bin(temp_dir.path(), None, &vars)
            .unwrap()
            .unwrap();
        assert_eq!(src, artifact_dir.join("demo"));
    }

    #[test]
    fn native_bin_dir_rejects_paths_outside_artifact() {
        assert!(validate_native_bin_relative_path("../bin/demo").is_err());
        assert!(validate_native_bin_relative_path("/bin/demo").is_err());
        assert!(validate_native_bin_relative_path("").is_err());
    }

    #[test]
    fn native_bin_names_must_be_plain_file_names() {
        let package = CratesIoPackage {
            name: "demo".to_string(),
            num: "1.0.0".to_string(),
            dl_path: "/api/v1/crates/demo/1.0.0/download".to_string(),
            checksum: String::new(),
            repository: None,
            bin_names: vec!["../../home/user/.bashrc".to_string()],
        };
        let bin = |name: &str| NativeCargoBin {
            name: name.to_string(),
            required_features: vec![],
        };
        // from the crates.io bin_names list
        let bins = native_bins_to_install(None, &[], &package, "demo");
        assert!(validate_native_bin_names(&bins).is_err());
        for name in ["../../x", "/home/user/.bashrc", "x/", "a/b", "..", ".", ""] {
            // from the manifest's [[bin]] tables
            let bins = native_bins_to_install(None, &[bin(name)], &package, "demo");
            assert!(
                validate_native_bin_names(&bins).is_err(),
                "expected {name:?} to be rejected"
            );
        }
        let bins = native_bins_to_install(None, &[bin("demo"), bin("demo-cli")], &package, "demo");
        assert!(validate_native_bin_names(&bins).is_ok());
    }

    #[test]
    fn native_bin_sources_reject_duplicate_sources() {
        let src = PathBuf::from("bin/demo");
        let pending = vec![
            (src.clone(), PathBuf::from("bin/one")),
            (src, PathBuf::from("bin/two")),
        ];

        assert!(validate_native_bin_sources(&pending).is_err());
    }

    #[test]
    fn crates_io_package_deserializes_string_or_object_crate_field() {
        let string_field: CratesIoPackageResponse = serde_json::from_value(serde_json::json!({
            "version": {
                "crate": "demo",
                "num": "1.2.3",
                "dl_path": "/api/v1/crates/demo/1.2.3/download",
                "checksum": "0000000000000000000000000000000000000000000000000000000000000000",
                "repository": null,
                "bin_names": ["demo"]
            }
        }))
        .unwrap();
        let object_field: CratesIoPackageResponse = serde_json::from_value(serde_json::json!({
            "version": {
                "crate": { "name": "demo" },
                "num": "1.2.3",
                "dl_path": "/api/v1/crates/demo/1.2.3/download",
                "checksum": "0000000000000000000000000000000000000000000000000000000000000000",
                "repository": null,
                "bin_names": ["demo"]
            }
        }))
        .unwrap();

        assert_eq!(string_field.version.name, "demo");
        assert_eq!(object_field.version.name, "demo");
    }

    #[test]
    fn crates_io_package_uses_top_level_repository_when_present() {
        let response: CratesIoPackageResponse = serde_json::from_value(serde_json::json!({
            "crate": {
                "name": "demo",
                "repository": "https://example.com/demo"
            },
            "version": {
                "crate": "demo",
                "num": "1.2.3",
                "dl_path": "/api/v1/crates/demo/1.2.3/download",
                "checksum": "0000000000000000000000000000000000000000000000000000000000000000",
                "repository": null,
                "bin_names": ["demo"]
            }
        }))
        .unwrap();

        let repository = response.krate.and_then(|krate| krate.repository);

        assert_eq!(repository.as_deref(), Some("https://example.com/demo"));
    }

    fn crate_file(dir: &Path, entries: &[(&str, &str)]) -> PathBuf {
        let path = dir.join("demo-1.0.0.crate");
        let encoder = flate2::write::GzEncoder::new(
            fs::File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = jdx_tar::Builder::new(encoder);
        for (name, contents) in entries {
            let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::File);
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            archive
                .append_data(&mut header, name, std::io::Cursor::new(contents.as_bytes()))
                .unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap();
        path
    }

    fn demo_package(checksum: String) -> CratesIoPackage {
        CratesIoPackage {
            name: "demo".to_string(),
            num: "1.0.0".to_string(),
            dl_path: "/api/v1/crates/demo/1.0.0/download".to_string(),
            checksum,
            repository: None,
            bin_names: vec![],
        }
    }

    const DEMO_MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n";

    #[test]
    fn crate_manifest_is_read_only_from_the_published_crate() {
        let dir = tempfile::tempdir().unwrap();
        let crate_path = crate_file(dir.path(), &[("demo-1.0.0/Cargo.toml", DEMO_MANIFEST)]);
        let published = hash::file_hash_sha256(&crate_path, None).unwrap();

        let manifest = read_crate_manifest(&crate_path, &demo_package(published), None).unwrap();
        assert_eq!(manifest["package"]["name"].as_str(), Some("demo"));

        let err =
            read_crate_manifest(&crate_path, &demo_package("0".repeat(64)), None).unwrap_err();
        assert!(
            format!("{err:#}").contains("Checksum mismatch"),
            "unexpected error: {err:#}"
        );
        assert!(read_crate_manifest(&crate_path, &demo_package(String::new()), None).is_err());
    }

    fn read_demo_manifest(crate_path: &Path) -> Result<toml::Value> {
        let published = hash::file_hash_sha256(crate_path, None).unwrap();
        read_crate_manifest(crate_path, &demo_package(published), None)
    }

    #[test]
    fn crate_manifest_is_read_up_to_a_limit() {
        let limit = crate::backend::static_helpers::ARCHIVE_METADATA_LIMIT as usize;
        let dir = tempfile::tempdir().unwrap();

        let manifest = format!("{DEMO_MANIFEST}{}", "#".repeat(limit - DEMO_MANIFEST.len()));
        let crate_path = crate_file(dir.path(), &[("demo-1.0.0/Cargo.toml", &manifest)]);
        assert!(read_demo_manifest(&crate_path).is_ok());

        let manifest = format!("{manifest}#");
        let crate_path = crate_file(dir.path(), &[("demo-1.0.0/Cargo.toml", &manifest)]);
        let err = read_demo_manifest(&crate_path).unwrap_err();
        assert!(
            format!("{err:#}").contains("larger than 1 MiB"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn crate_manifest_is_not_read_through_a_shipped_link() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("Cargo.toml"), DEMO_MANIFEST).unwrap();

        for (name, target) in [
            (
                "demo-1.0.0/Cargo.toml",
                outside.join("Cargo.toml").as_path(),
            ),
            ("demo-1.0.0", outside.as_path()),
            ("demo-1.0.0/Cargo.toml", Path::new("/dev/zero")),
        ] {
            let crate_path = dir.path().join("demo-1.0.0.crate");
            let encoder = flate2::write::GzEncoder::new(
                fs::File::create(&crate_path).unwrap(),
                flate2::Compression::default(),
            );
            let mut archive = jdx_tar::Builder::new(encoder);
            let mut header = jdx_tar::Header::new_gnu(jdx_tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            archive.append_link(&mut header, name, target).unwrap();
            archive.into_inner().unwrap().finish().unwrap();

            assert!(
                read_demo_manifest(&crate_path).is_err(),
                "{name} -> {} was read",
                target.display()
            );
        }
    }

    #[test]
    fn crates_io_package_requires_a_checksum() {
        let response = serde_json::from_value::<CratesIoPackageResponse>(serde_json::json!({
            "version": {
                "crate": "demo",
                "num": "1.2.3",
                "dl_path": "/api/v1/crates/demo/1.2.3/download",
                "repository": null
            }
        }));

        assert!(response.is_err());
    }

    #[test]
    fn cargo_registry_config_sends_the_install_to_cargo() {
        let root = tempfile::tempdir().unwrap();
        let cargo_home = root.path().join("up").join("cargo");
        let cwd = root.path().join("cwd");
        fs::create_dir_all(&cargo_home).unwrap();
        fs::create_dir_all(cwd.join(".cargo")).unwrap();
        let mut cargo_env = BTreeMap::from([(
            "CARGO_HOME".to_string(),
            cargo_home.to_string_lossy().to_string(),
        )]);
        let default_company = "[registry]\ndefault = \"company\"\n";

        // no configuration, one that keeps crates.io, and the working
        // directory's, which `cargo install` does not read
        assert!(!cargo_config_names_other_registry(&cargo_env, &cwd));
        fs::write(
            cargo_home.join("config.toml"),
            "[net]\nretry = 3\n[registry]\ndefault = \"crates-io\"\n",
        )
        .unwrap();
        fs::write(cwd.join(".cargo/config.toml"), default_company).unwrap();
        assert!(!cargo_config_names_other_registry(&cargo_env, &cwd));
        fs::remove_file(cargo_home.join("config.toml")).unwrap();

        for config in [
            default_company,
            "[registry]\nindex = \"https://acme.example/index\"\n",
            "[source.crates-io]\nreplace-with = \"mirror\"\n",
            "include = [\"other.toml\"]\n",
            "[registry",
        ] {
            for path in [
                cargo_home.join("config.toml"),
                cargo_home.join("config"),
                root.path().join("up/.cargo/config.toml"),
            ] {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, config).unwrap();
                assert!(
                    cargo_config_names_other_registry(&cargo_env, &cwd),
                    "{config:?} in {}",
                    path.display()
                );
                fs::remove_file(&path).unwrap();
            }
        }

        cargo_env.insert("CARGO_REGISTRY_DEFAULT".into(), "crates-io".into());
        assert!(!cargo_config_names_other_registry(&cargo_env, &cwd));
        cargo_env.insert("CARGO_REGISTRY_DEFAULT".into(), "company".into());
        assert!(cargo_config_names_other_registry(&cargo_env, &cwd));
        cargo_env.remove("CARGO_REGISTRY_DEFAULT");
        cargo_env.insert(
            "CARGO_REGISTRY_INDEX".into(),
            "https://acme.example/index".into(),
        );
        assert!(cargo_config_names_other_registry(&cargo_env, &cwd));

        // without CARGO_HOME, cargo's home is `.cargo` in the home directory
        let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let cargo_env = BTreeMap::from([(
            home_var.to_string(),
            root.path().to_string_lossy().to_string(),
        )]);
        assert!(!cargo_config_names_other_registry(&cargo_env, &cwd));
        fs::create_dir_all(root.path().join(".cargo")).unwrap();
        fs::write(root.path().join(".cargo/config.toml"), default_company).unwrap();
        assert!(cargo_config_names_other_registry(&cargo_env, &cwd));
    }

    #[test]
    fn native_binstall_artifact_url_must_be_https() {
        let pkg_url = "{ repo }/releases/download/v{ version }/{ name }-{ target }";
        let vars = |repo| BinstallTemplateVars {
            repo,
            name: "demo",
            version: "1.2.3",
            target: "x86_64-unknown-linux-gnu",
            archive_format: "bin",
            archive_suffix: "",
            bin: "demo",
            binary_ext: "",
        };

        assert_eq!(
            native_binstall_artifact_url(pkg_url, &vars("https://github.com/acme/demo")).unwrap(),
            "https://github.com/acme/demo/releases/download/v1.2.3/demo-x86_64-unknown-linux-gnu"
        );
        // a crate's `repository` is a link for people, and can be an http one
        let err = native_binstall_artifact_url(pkg_url, &vars("http://github.com/acme/demo"))
            .unwrap_err();
        assert!(err.to_string().contains("over https"), "{err}");
        // `pkg-url` can also name the scheme itself
        for pkg_url in [
            "http://example.com/{ name }-{ target }",
            "HTTP://example.com/{ name }-{ target }",
            "ftp://example.com/{ name }-{ target }",
            "file:///tmp/{ name }-{ target }",
        ] {
            assert!(
                native_binstall_artifact_url(pkg_url, &vars("https://github.com/acme/demo"))
                    .is_err(),
                "{pkg_url}"
            );
        }
        // with no `repository`, the URL has no scheme or host
        assert!(native_binstall_artifact_url(pkg_url, &vars("")).is_err());
    }
}
