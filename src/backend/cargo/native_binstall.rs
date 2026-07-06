use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use eyre::{Context, bail, eyre};
use url::Url;

use crate::Result;
use crate::backend::cargo::CargoOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;
use crate::config::Settings;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;

pub const WARN_AT: &str = "2027.1.0";
pub const DEFAULT_AT: &str = "2027.7.0";

#[derive(Debug, Clone, Copy)]
pub enum NativeBinstallAction {
    Install,
    WarnOnly,
}

pub async fn install(
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
    let package = CratesIoPackage::fetch(&crate_name, version).await?;
    let target = cargo_target_triple(&PlatformTarget::from_current())
        .ok_or_else(|| eyre!("unsupported platform for native cargo binary install"))?;

    let target_platform = PlatformTarget::from_current();
    let manifest = download_crate_manifest(&package, tv, ctx).await?;
    let Some(binstall) =
        NativeBinstallMetadata::from_manifest(&manifest, &target, &target_platform)?
    else {
        return Ok(false);
    };
    if binstall.disabled_strategies.contains("crate-meta-data") {
        return Ok(false);
    }

    let manifest_bins = native_manifest_bins(&manifest);
    let bins = native_bins_to_install(opts.bin(), &manifest_bins, &package, &crate_name);
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
        if !raw_binary_url_supports_bins(&binstall.pkg_url, &bin_names) {
            return Ok(false);
        }
        if matches!(action, NativeBinstallAction::WarnOnly) {
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
            let archive_url = expand_binstall_template(&binstall.pkg_url, &vars);
            let src = download_dir
                .path()
                .join(format!("{}{}", bin.name, binary_ext));
            let dest = bin_root.join(format!("{}{}", bin.name, binary_ext));
            ctx.pr
                .set_message(format!("download {}", get_filename_from_url(&archive_url)));
            download_native_binstall_file(&archive_url, &src, Some(ctx.pr.as_ref())).await?;
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
        warn_native_binstall_rollout();
        return Ok(false);
    }

    file::create_dir_all(&bin_root)?;

    if template_contains_var(&binstall.pkg_url, "bin") {
        let download_dir = tempfile::Builder::new()
            .prefix("mise-cargo-binstall-archives-")
            .tempdir()?;
        let mut pending = vec![];
        for bin in &bins {
            let vars = template_ctx.vars(&bin.name);
            let archive_url = expand_binstall_template(&binstall.pkg_url, &vars);
            let archive_path = download_dir.path().join(format!(
                "{}-{}",
                bin.name,
                get_filename_from_url(&archive_url)
            ));
            let extract_dir = download_dir.path().join(format!("{}-extract", bin.name));
            ctx.pr
                .set_message(format!("download {}", get_filename_from_url(&archive_url)));
            download_native_binstall_file(&archive_url, &archive_path, Some(ctx.pr.as_ref()))
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

    let archive_url = expand_binstall_template(
        &binstall.pkg_url,
        &template_ctx.vars(
            bins.first()
                .map(|bin| bin.name.as_str())
                .unwrap_or(&crate_name),
        ),
    );

    let archive_path = tv.download_path().join(get_filename_from_url(&archive_url));
    ctx.pr.set_message(format!(
        "download {}",
        archive_path.file_name().unwrap().to_string_lossy()
    ));
    download_native_binstall_file(&archive_url, &archive_path, Some(ctx.pr.as_ref())).await?;

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
    repository: Option<String>,
    #[serde(default)]
    bin_names: Vec<String>,
}

#[derive(Debug)]
struct CratesIoPackage {
    name: String,
    num: String,
    dl_path: String,
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
    pkg_url: String,
    bin_dir: Option<String>,
    pkg_fmt: Option<String>,
    disabled_strategies: BTreeSet<String>,
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
    ) -> Result<Option<Self>> {
        let Some(root) = manifest
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("binstall"))
            .and_then(toml::Value::as_table)
        else {
            return Ok(None);
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

        let Some(pkg_url) = merged.get("pkg-url").and_then(toml::Value::as_str) else {
            return Ok(None);
        };

        Ok(Some(Self {
            pkg_url: pkg_url.to_string(),
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
        }))
    }
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
    let crate_path = tv
        .download_path()
        .join(format!("{}-{}.crate", package.name, package.num));
    ctx.pr.set_message(format!(
        "download {}",
        crate_path.file_name().unwrap().to_string_lossy()
    ));
    HTTP.download_file(package.download_url(), &crate_path, Some(ctx.pr.as_ref()))
        .await?;

    let temp_dir = tempfile::Builder::new()
        .prefix("mise-cargo-crate-")
        .tempdir()?;
    file::extract_archive(
        &crate_path,
        temp_dir.path(),
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: Some(ctx.pr.as_ref()),
            preserve_mtime: true,
        },
    )?;

    let manifest_path = temp_dir
        .path()
        .join(format!("{}-{}/Cargo.toml", package.name, package.num));
    let manifest = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", file::display_path(&manifest_path)))?;
    Ok(toml::from_str(&manifest)?)
}

async fn download_native_binstall_file(
    url: &str,
    path: &std::path::Path,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    let download_url = resolve_native_binstall_download_url(url).await;
    HTTP.download_file(&download_url, path, pr).await
}

async fn resolve_native_binstall_download_url(url: &str) -> String {
    let Some((repo, tag, asset_name)) = github_release_asset_from_url(url) else {
        return url.to_string();
    };

    match crate::github::get_release(&repo, &tag).await {
        Ok(release) => {
            if let Some(asset) = release.assets.iter().find(|asset| asset.name == asset_name) {
                crate::github::pick_reachable_asset_url(&asset.browser_download_url, &asset.url)
                    .await
            } else {
                debug!("GitHub release {repo}@{tag} did not include asset {asset_name}");
                url.to_string()
            }
        }
        Err(err) => {
            debug!("failed to resolve GitHub release asset {repo}@{tag}/{asset_name}: {err:#}");
            url.to_string()
        }
    }
}

fn github_release_asset_from_url(url: &str) -> Option<(String, String, String)> {
    let url = Url::parse(url).ok()?;
    if url.host_str()? != "github.com" {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let [owner, repo, "releases", "download", tag, asset] = segments.as_slice() else {
        return None;
    };
    let tag = urlencoding::decode(tag).ok()?.into_owned();
    let asset = urlencoding::decode(asset).ok()?.into_owned();
    Some((format!("{owner}/{repo}"), tag, asset))
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

fn expand_binstall_template(template: &str, vars: &BinstallTemplateVars<'_>) -> String {
    let replacements = [
        ("repo", vars.repo),
        ("name", vars.name),
        ("version", vars.version),
        ("target", vars.target),
        ("archive-format", vars.archive_format),
        ("archive-suffix", vars.archive_suffix),
        ("bin", vars.bin),
        ("binary-ext", vars.binary_ext),
    ];
    let mut out = template.to_string();
    for (key, value) in replacements {
        out = out.replace(&format!("{{ {key} }}"), value);
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

fn template_contains_var(template: &str, key: &str) -> bool {
    template.contains(&format!("{{ {key} }}")) || template.contains(&format!("{{{key}}}"))
}

pub fn rollout_warning_active() -> bool {
    rollout_warning_active_for(&crate::cli::version::V)
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
    let rendered = expand_binstall_template(&bin_dir, vars);
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
        let metadata = metadata.unwrap();

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
        .unwrap()
        .unwrap();

        assert_eq!(metadata.pkg_url, "musl");
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
        .unwrap()
        .unwrap();

        assert_eq!(metadata.pkg_fmt.as_deref(), Some("zip"));
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
        );

        assert_eq!(
            rendered,
            "https://example.com/demo-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
        );
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
        );

        assert_eq!(rendered, "demo-1.2.3/demo-cli.exe");
    }

    #[test]
    fn github_release_asset_from_url_parses_browser_download_urls() {
        assert_eq!(
            github_release_asset_from_url(
                "https://github.com/owner/repo/releases/download/v1.2.3/tool-aarch64.tar.gz"
            ),
            Some((
                "owner/repo".to_string(),
                "v1.2.3".to_string(),
                "tool-aarch64.tar.gz".to_string()
            ))
        );
    }

    #[test]
    fn github_release_asset_from_url_decodes_tag_and_asset() {
        assert_eq!(
            github_release_asset_from_url(
                "https://github.com/owner/repo/releases/download/v1%2Bmeta/tool%20name.tar.gz"
            ),
            Some((
                "owner/repo".to_string(),
                "v1+meta".to_string(),
                "tool name.tar.gz".to_string()
            ))
        );
    }

    #[test]
    fn github_release_asset_from_url_ignores_non_release_urls() {
        assert_eq!(
            github_release_asset_from_url(
                "https://example.com/owner/repo/releases/download/v1/tool"
            ),
            None
        );
        assert_eq!(
            github_release_asset_from_url(
                "https://github.com/owner/repo/archive/refs/tags/v1.tar.gz"
            ),
            None
        );
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
                "repository": null,
                "bin_names": ["demo"]
            }
        }))
        .unwrap();

        let repository = response.krate.and_then(|krate| krate.repository);

        assert_eq!(repository.as_deref(), Some("https://example.com/demo"));
    }
}
