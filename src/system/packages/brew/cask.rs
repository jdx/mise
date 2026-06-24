use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use eyre::{WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use super::prefix;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::hash;
use crate::http::HTTP_FETCH;
use crate::result::Result;
use crate::system::packages::{
    InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager,
};
use crate::system::sudo;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressIcon, SingleReport};

const API_BASE: &str = "https://formulae.brew.sh/api";

pub struct BrewCaskManager {}

#[derive(Debug, Clone, Deserialize)]
struct Cask {
    token: String,
    version: String,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    artifacts: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppArtifact {
    source: String,
    target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryArtifact {
    source: String,
    target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PkgArtifact {
    source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CaskArtifacts {
    apps: Vec<AppArtifact>,
    binaries: Vec<BinaryArtifact>,
    pkgs: Vec<PkgArtifact>,
    pkg_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CaskReceipt {
    version: String,
    #[serde(default)]
    apps: Vec<PathBuf>,
    #[serde(default)]
    binaries: Vec<PathBuf>,
    #[serde(default)]
    pkg_ids: Vec<String>,
}

impl BrewCaskManager {
    pub fn new() -> Self {
        Self {}
    }

    async fn install_one(
        &self,
        req: &PackageRequest,
        opts: &InstallOpts,
        pr: Option<&dyn SingleReport>,
    ) -> Result<String> {
        let cask = fetch_cask(req).await?;
        let artifacts = cask_artifacts(&cask)?;
        if installed_cask_version(&cask, &artifacts)?.as_deref() == Some(cask.version.as_str()) {
            info!("brew-cask:{}: already installed", cask.token);
            return Ok(cask.version);
        }
        if opts.dry_run {
            miseprintln!("install cask {}/{}", cask.token, cask.version);
            for app in &artifacts.apps {
                miseprintln!("link app {}", app.target_name());
            }
            for binary in &artifacts.binaries {
                miseprintln!("link binary {}", binary.target_name()?);
            }
            for pkg in &artifacts.pkgs {
                miseprintln!("install pkg {}", pkg.source);
            }
            return Ok(cask.version);
        }
        prefix::bootstrap(false)?;
        let previous_binaries = previous_binary_targets(&cask)?;
        let archive = fetch_archive(&cask, pr).await?;
        let stage = extract_archive(&cask, &archive, pr)?;
        let caskroom_token = caskroom_token_dir(&cask.token);
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let tmp_caskroom = caskroom_tmp_dir(&cask);
        file::remove_all(&tmp_caskroom)?;
        file::create_dir_all(&tmp_caskroom)?;
        for app in &artifacts.apps {
            install_app(&stage, &tmp_caskroom, app)?;
        }
        for binary in &artifacts.binaries {
            stage_binary(&stage, &tmp_caskroom, binary)?;
        }
        for pkg in &artifacts.pkgs {
            install_pkg(&stage, pkg)?;
        }
        write_receipt(&tmp_caskroom, &cask, &artifacts)?;
        file::remove_all(&caskroom)?;
        file::rename(&tmp_caskroom, &caskroom)?;
        for binary in &artifacts.binaries {
            link_binary(&caskroom, binary)?;
        }
        remove_obsolete_binary_links(&cask, &previous_binaries, &binary_targets(&artifacts)?)?;
        remove_stale_versions(&caskroom_token, &cask.version)?;
        file::remove_all(stage)?;
        Ok(cask.version)
    }
}

impl AppArtifact {
    fn target_name(&self) -> &str {
        self.target.as_deref().unwrap_or(&self.source)
    }
}

impl BinaryArtifact {
    fn target_name(&self) -> Result<String> {
        match &self.target {
            Some(target) => Ok(target.clone()),
            None => Path::new(&self.source)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .ok_or_else(|| eyre!("brew-cask: invalid binary source '{}'", self.source)),
        }
    }

    fn target_path(&self) -> Result<PathBuf> {
        binary_target_path(&self.target_name()?)
    }
}

#[async_trait(?Send)]
impl SystemPackageManager for BrewCaskManager {
    fn name(&self) -> &'static str {
        "brew-cask"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn unavailable_reason(&self) -> String {
        "only available on macos".to_string()
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let mut statuses = Vec::with_capacity(pkgs.len());
        for req in pkgs {
            let cask = fetch_cask(req).await?;
            let artifacts = cask_artifacts(&cask)?;
            let version = installed_cask_version(&cask, &artifacts)?;
            let state = match version {
                Some(version) => match &req.version {
                    Some(requested) if version != *requested => {
                        PackageState::VersionMismatch { installed: version }
                    }
                    _ => PackageState::Installed { version },
                },
                None => PackageState::Missing,
            };
            statuses.push(PackageStatus {
                request: req.clone(),
                state,
            });
        }
        Ok(statuses)
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if let Some(p) = pkgs.iter().find(|p| p.version.is_some()) {
            bail!("brew casks are installed at their current version ('{p}')");
        }
        if opts.dry_run {
            prefix::bootstrap(true)?;
            for pkg in pkgs {
                self.install_one(pkg, opts, None).await?;
            }
            return Ok(());
        }
        let mpr = MultiProgressReport::get();
        mpr.init_footer(false, "install", pkgs.len());
        for pkg in pkgs {
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew-cask:{}", pkg.name));
            match self.install_one(pkg, opts, Some(&*pr)).await {
                Ok(version) => {
                    pr.finish_with_message(version);
                    mpr.footer_inc(1);
                }
                Err(err) => {
                    pr.finish_with_icon("failed".to_string(), ProgressIcon::Error);
                    mpr.footer_finish();
                    return Err(err);
                }
            }
        }
        mpr.footer_finish();
        Ok(())
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install(pkgs, opts).await
    }
}

async fn fetch_cask(req: &PackageRequest) -> Result<Cask> {
    let name = &req.name;
    let url = match split_tap_name(name) {
        Some(("homebrew", "cask", token)) => format!("{API_BASE}/cask/{token}.json"),
        Some((owner, tap, token)) => {
            let Some(base) = super::api::tap_raw_base(owner, tap, req.tap_url.as_deref()) else {
                bail!(
                    "brew-cask: unsupported tap URL for '{name}'; only GitHub tap URLs can be fetched directly"
                );
            };
            format!("{base}/api/cask/{token}.json")
        }
        None => format!("{API_BASE}/cask/{name}.json"),
    };
    HTTP_FETCH
        .json_cached::<Cask, _>(url)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to fetch Homebrew cask '{name}' directly. \
                 Tapped casks must publish API metadata at api/cask/<token>.json"
            )
        })
}

async fn fetch_archive(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let filename = archive_filename(&cask.url)
        .ok_or_else(|| eyre!("brew-cask:{}: URL has no file name", cask.token))?;
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("casks");
    file::create_dir_all(&cache_dir)?;
    let url_hash = &hash::hash_sha256_to_str(&cask.url)[..12];
    let archive = cache_dir.join(format!(
        "{}-{}-{url_hash}-{filename}",
        cask.token, cask.version
    ));
    if !archive.exists() {
        HTTP_FETCH.download_file(&cask.url, &archive, pr).await?;
    }
    match cask.sha256.as_deref() {
        Some("no_check") => {}
        Some(sha256) => hash::ensure_checksum(&archive, sha256, pr, "sha256")?,
        None => bail!("brew-cask:{}: cask metadata has no sha256", cask.token),
    }
    Ok(archive)
}

fn extract_archive(cask: &Cask, archive: &Path, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let extract_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-extract")
        .join(format!("{}-{}", cask.token, cask.version));
    file::remove_all(&extract_dir)?;
    file::create_dir_all(&extract_dir)?;
    let filename = archive
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    if filename.ends_with(".dmg") {
        file::un_dmg(archive, &extract_dir)?;
    } else {
        let format = ExtractionFormat::from_file_name(filename);
        if !format.is_archive() {
            bail!(
                "brew-cask:{}: unsupported archive type for {}",
                cask.token,
                filename
            );
        }
        file::extract_archive(
            archive,
            &extract_dir,
            format,
            &ExtractOptions {
                pr,
                ..Default::default()
            },
        )?;
    }
    Ok(extract_dir)
}

fn install_app(stage: &Path, caskroom: &Path, app: &AppArtifact) -> Result<()> {
    let source = find_app(stage, &app.source)
        .ok_or_else(|| eyre!("brew-cask: app artifact '{}' was not found", app.source))?;
    let caskroom_app = caskroom.join(app_bundle_name(app.target_name())?);
    file::remove_all(&caskroom_app)?;
    file::copy_dir_all(&source, &caskroom_app)?;
    let target = app_target_path(app.target_name())?;
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    let tmp_target = target.with_extension(format!(
        "mise-tmp-{}",
        crate::hash::hash_to_str(&target.display().to_string())
    ));
    file::remove_all(&tmp_target)?;
    file::copy_dir_all(&caskroom_app, &tmp_target)?;
    file::remove_all(&target)?;
    file::rename(&tmp_target, &target)?;
    Ok(())
}

fn install_pkg(stage: &Path, pkg: &PkgArtifact) -> Result<()> {
    let source = find_artifact(stage, &pkg.source)
        .ok_or_else(|| eyre!("brew-cask: pkg artifact '{}' was not found", pkg.source))?;
    let args = vec![
        "-pkg".to_string(),
        source.display().to_string(),
        "-target".to_string(),
        "/".to_string(),
    ];
    sudo::run("installer", &args, &[])
}

fn stage_binary(stage: &Path, caskroom: &Path, binary: &BinaryArtifact) -> Result<()> {
    let source = find_artifact(stage, &binary.source)
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            eyre!(
                "brew-cask: binary artifact '{}' was not found",
                binary.source
            )
        })?;
    let caskroom_binary = caskroom_binary_path(caskroom, binary)?;
    file::remove_all(&caskroom_binary)?;
    if let Some(parent) = caskroom_binary.parent() {
        file::create_dir_all(parent)?;
    }
    file::copy(&source, &caskroom_binary)?;
    file::make_executable(&caskroom_binary)?;
    Ok(())
}

fn link_binary(caskroom: &Path, binary: &BinaryArtifact) -> Result<()> {
    let caskroom_binary = caskroom_binary_path(caskroom, binary)?;
    if !caskroom_binary.is_file() {
        bail!(
            "brew-cask: binary artifact '{}' was not staged",
            binary.source
        );
    }
    let target = binary.target_path()?;
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    file::make_symlink(&caskroom_binary, &target)?;
    Ok(())
}

fn caskroom_binary_path(caskroom: &Path, binary: &BinaryArtifact) -> Result<PathBuf> {
    let target = binary.target_path()?;
    let relative = target.strip_prefix(prefix::prefix()).wrap_err_with(|| {
        format!(
            "brew-cask: binary target '{}' must be under {}",
            target.display(),
            prefix::prefix().display()
        )
    })?;
    if relative.components().next().is_none() {
        bail!(
            "brew-cask: invalid binary target '{}'",
            binary.target_name()?
        );
    }
    Ok(caskroom.join(relative))
}

fn cask_artifacts(cask: &Cask) -> Result<CaskArtifacts> {
    let mut artifacts = CaskArtifacts::default();
    for artifact in &cask.artifacts {
        let artifact_type = artifact_type(artifact);
        if is_non_install_artifact(&artifact_type) {
            collect_uninstall_pkg_ids(artifact, &mut artifacts.pkg_ids);
            continue;
        }
        if let Some(app) = parse_app_artifact(artifact) {
            artifacts.apps.push(app);
            continue;
        }
        if let Some(binary) = parse_binary_artifact(artifact) {
            artifacts.binaries.push(binary);
            continue;
        }
        if let Some(pkg) = parse_pkg_artifact(artifact)? {
            artifacts.pkgs.push(pkg);
            continue;
        }
        bail!(
            "brew-cask:{}: unsupported artifact type {}",
            cask.token,
            artifact_type
        );
    }
    if artifacts.apps.is_empty() && artifacts.binaries.is_empty() && artifacts.pkgs.is_empty() {
        bail!(
            "brew-cask:{}: no app, binary, or pkg artifact found; only app-bundle, binary, and pkg casks are supported",
            cask.token
        );
    }
    artifacts.pkg_ids.sort();
    artifacts.pkg_ids.dedup();
    if artifacts.pkgs.is_empty() {
        artifacts.pkg_ids.clear();
    } else if artifacts.pkg_ids.is_empty() {
        bail!(
            "brew-cask:{}: pkg artifacts require pkgutil ids in uninstall or zap metadata",
            cask.token
        );
    }
    Ok(artifacts)
}

fn artifact_target(value: &Value, values: &[Value]) -> Option<String> {
    values
        .get(1)
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("target"))
        .or_else(|| value.as_object().and_then(|o| o.get("target")))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_app_artifact(value: &Value) -> Option<AppArtifact> {
    let app = value.as_object()?.get("app")?;
    match app {
        Value::String(source) => Some(AppArtifact {
            source: source.clone(),
            target: None,
        }),
        Value::Array(values) => {
            let source = values.first()?.as_str()?.to_string();
            let target = artifact_target(value, values);
            Some(AppArtifact { source, target })
        }
        _ => None,
    }
}

fn parse_binary_artifact(value: &Value) -> Option<BinaryArtifact> {
    let binary = value.as_object()?.get("binary")?;
    match binary {
        Value::String(source) => Some(BinaryArtifact {
            source: source.clone(),
            target: value
                .as_object()
                .and_then(|o| o.get("target"))
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        Value::Array(values) => {
            let source = values.first()?.as_str()?.to_string();
            let target = artifact_target(value, values);
            Some(BinaryArtifact { source, target })
        }
        _ => None,
    }
}

fn parse_pkg_artifact(value: &Value) -> Result<Option<PkgArtifact>> {
    let Some(pkg) = value.as_object().and_then(|o| o.get("pkg")) else {
        return Ok(None);
    };
    match pkg {
        Value::String(source) => Ok(Some(PkgArtifact {
            source: source.clone(),
        })),
        Value::Array(values) => {
            if values.len() > 1 {
                bail!("brew-cask: pkg installer choices are not supported yet");
            }
            Ok(values
                .first()
                .and_then(Value::as_str)
                .map(|source| PkgArtifact {
                    source: source.to_string(),
                }))
        }
        _ => Ok(None),
    }
}

fn collect_uninstall_pkg_ids(value: &Value, pkg_ids: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        return;
    };
    for key in ["uninstall", "zap"] {
        let Some(metadata) = object.get(key) else {
            continue;
        };
        let values: Vec<&Value> = match metadata {
            Value::Array(values) => values.iter().collect(),
            value => vec![value],
        };
        for value in values {
            let Some(pkgutil) = value.as_object().and_then(|o| o.get("pkgutil")) else {
                continue;
            };
            match pkgutil {
                Value::String(id) => pkg_ids.push(id.clone()),
                Value::Array(ids) => {
                    pkg_ids.extend(ids.iter().filter_map(Value::as_str).map(str::to_string))
                }
                _ => {}
            }
        }
    }
}

fn find_app(root: &Path, name: &str) -> Option<PathBuf> {
    find_artifact(root, name).filter(|path| path.is_dir())
}

fn find_artifact(root: &Path, name: &str) -> Option<PathBuf> {
    let name_path = Path::new(name);
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .is_ok_and(|relative| relative.ends_with(name_path))
        })
        .map(|entry| entry.into_path())
}

fn app_target_path(target_name: &str) -> Result<PathBuf> {
    if target_name.contains('/') {
        let target = target_name.replace("$HOMEBREW_PREFIX", &prefix::prefix().to_string_lossy());
        let path = PathBuf::from(target);
        if path.is_absolute() {
            let prefix_app_dir = prefix::prefix().join("Applications");
            if path.starts_with("/Applications") || path.starts_with(&prefix_app_dir) {
                return Ok(path);
            }
            bail!("brew-cask: app target '{target_name}' must be under /Applications");
        }
        bail!("brew-cask: app target '{target_name}' must be an absolute path");
    }
    Ok(PathBuf::from("/Applications").join(target_name))
}

fn app_bundle_name(target_name: &str) -> Result<&str> {
    Path::new(target_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("brew-cask: invalid app target '{target_name}'"))
}

fn binary_target_path(target_name: &str) -> Result<PathBuf> {
    let prefix = prefix::prefix();
    let prefix_str = prefix.to_string_lossy();
    let target_name = target_name.replace("$HOMEBREW_PREFIX", prefix_str.as_ref());
    let path = PathBuf::from(&target_name);
    let target = if path.is_absolute() {
        path
    } else if target_name.contains('/') {
        prefix.join(path)
    } else {
        prefix.join("bin").join(path)
    };
    if target
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: binary target '{}' must not contain '..'",
            target.display()
        );
    }
    if !target.starts_with(&prefix) {
        bail!(
            "brew-cask: binary target '{}' must be under {}",
            target.display(),
            prefix.display()
        );
    }
    Ok(target)
}

fn installed_version(token: &str) -> Option<String> {
    let dir = caskroom_token_dir(token);
    let entries = std::fs::read_dir(dir).ok()?;
    let versions = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|ft| ft.is_dir())
                .and_then(|_| entry.file_name().into_string().ok())
        })
        .collect::<Vec<_>>();
    match versions.as_slice() {
        [version] => Some(version.clone()),
        [] => None,
        _ => {
            warn!("brew-cask:{token}: multiple Caskroom versions found; reinstall to reconcile");
            None
        }
    }
}

fn pkg_id_installed(pkg_id: &str) -> Result<bool> {
    let output = std::process::Command::new("pkgutil")
        .arg("--pkg-info")
        .arg(pkg_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(output.success())
}

fn pkg_ids_installed(pkg_ids: &[String]) -> Result<bool> {
    for pkg_id in pkg_ids {
        if !pkg_id_installed(pkg_id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn binary_targets(artifacts: &CaskArtifacts) -> Result<Vec<PathBuf>> {
    artifacts
        .binaries
        .iter()
        .map(BinaryArtifact::target_path)
        .collect::<Result<Vec<_>>>()
}

fn previous_binary_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    let Some(version) = installed_version(&cask.token) else {
        return Ok(Vec::new());
    };
    let version_dir = caskroom_version_dir(&cask.token, &version);
    Ok(read_receipt(&version_dir)?
        .map(|receipt| receipt.binaries)
        .unwrap_or_default())
}

fn remove_obsolete_binary_links(
    cask: &Cask,
    previous_targets: &[PathBuf],
    current_targets: &[PathBuf],
) -> Result<()> {
    let token_dir = file::desymlink_path(&caskroom_token_dir(&cask.token));
    for target in previous_targets {
        if current_targets.contains(target) {
            continue;
        }
        let Ok(metadata) = target.symlink_metadata() else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(link_target) = std::fs::read_link(target) else {
            continue;
        };
        let resolved = if link_target.is_absolute() {
            link_target
        } else {
            target
                .parent()
                .map(|parent| parent.join(&link_target))
                .unwrap_or(link_target)
        };
        if file::desymlink_path(&resolved).starts_with(&token_dir) {
            file::remove_file(target)?;
        }
    }
    Ok(())
}

fn installed_cask_version(cask: &Cask, artifacts: &CaskArtifacts) -> Result<Option<String>> {
    let Some(version) = installed_version(&cask.token) else {
        return Ok(None);
    };
    let version_dir = caskroom_version_dir(&cask.token, &version);
    match read_receipt(&version_dir)? {
        Some(receipt) => {
            let app_targets = if receipt.apps.is_empty() {
                artifacts
                    .apps
                    .iter()
                    .map(|app| app_target_path(app.target_name()))
                    .collect::<Result<Vec<_>>>()?
            } else {
                receipt.apps
            };
            let binary_targets = if receipt.binaries.is_empty() {
                artifacts
                    .binaries
                    .iter()
                    .map(BinaryArtifact::target_path)
                    .collect::<Result<Vec<_>>>()?
            } else {
                receipt.binaries
            };
            let pkgs_installed =
                artifacts.pkgs.is_empty() || pkg_ids_installed(&artifacts.pkg_ids)?;
            if app_targets.iter().all(|app| app.exists())
                && binary_targets.iter().all(|binary| binary.exists())
                && pkgs_installed
            {
                Ok(Some(receipt.version))
            } else {
                Ok(None)
            }
        }
        None => {
            for app in &artifacts.apps {
                if !app_target_path(app.target_name())?.exists() {
                    return Ok(None);
                }
            }
            for binary in &artifacts.binaries {
                if !binary.target_path()?.exists() {
                    return Ok(None);
                }
            }
            if !artifacts.pkgs.is_empty() && !pkg_ids_installed(&artifacts.pkg_ids)? {
                return Ok(None);
            }
            Ok(Some(version))
        }
    }
}

fn write_receipt(caskroom: &Path, cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    let receipt = CaskReceipt {
        version: cask.version.clone(),
        apps: artifacts
            .apps
            .iter()
            .map(|app| app_target_path(app.target_name()))
            .collect::<Result<Vec<_>>>()?,
        binaries: binary_targets(artifacts)?,
        pkg_ids: artifacts.pkg_ids.clone(),
    };
    let body = toml::to_string_pretty(&receipt)?;
    crate::file::write(caskroom.join(".mise-cask.toml"), body)?;
    Ok(())
}

fn read_receipt(caskroom: &Path) -> Result<Option<CaskReceipt>> {
    let path = caskroom.join(".mise-cask.toml");
    if !path.exists() {
        return Ok(None);
    }
    let body = crate::file::read_to_string(&path)?;
    toml::from_str(&body)
        .map(Some)
        .wrap_err_with(|| format!("failed to parse {}", path.display()))
}

fn caskroom_token_dir(token: &str) -> PathBuf {
    prefix::prefix().join("Caskroom").join(token)
}

fn caskroom_version_dir(token: &str, version: &str) -> PathBuf {
    caskroom_token_dir(token).join(version)
}

fn caskroom_tmp_dir(cask: &Cask) -> PathBuf {
    let key = format!("{}-{}", cask.token, cask.version);
    caskroom_token_dir(&cask.token).join(format!(".mise-tmp-{}", hash::hash_to_str(&key)))
}

fn remove_stale_versions(token_dir: &Path, current_version: &str) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(token_dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        if entry.file_type().is_ok_and(|ft| ft.is_dir())
            && entry.file_name().to_str() != Some(current_version)
        {
            file::remove_all(entry.path())?;
        }
    }
    Ok(())
}

fn archive_filename(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    url.path_segments()?.next_back().map(str::to_string)
}

fn split_tap_name(name: &str) -> Option<(&str, &str, &str)> {
    super::api::split_tap_name(name)
}

fn artifact_type(value: &Value) -> String {
    value
        .as_object()
        .and_then(|o| o.keys().next())
        .cloned()
        .unwrap_or_else(|| "unknown".to_string())
}

fn is_non_install_artifact(kind: &str) -> bool {
    matches!(
        kind,
        "caveats"
            | "conflicts_with"
            | "depends_on"
            | "generate_completions_from_executable"
            | "postflight"
            | "preflight"
            | "uninstall"
            | "zap"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct BrewPrefixGuard {
        previous: Option<String>,
    }

    impl BrewPrefixGuard {
        fn set(prefix: &Path) -> Self {
            let previous = crate::env::var("MISE_SYSTEM_BREW_PREFIX").ok();
            crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", prefix);
            Self { previous }
        }
    }

    impl Drop for BrewPrefixGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", previous),
                None => crate::env::remove_var("MISE_SYSTEM_BREW_PREFIX"),
            }
        }
    }

    fn test_cask(token: &str, version: &str) -> Cask {
        Cask {
            token: token.to_string(),
            version: version.to_string(),
            url: "https://example.com/example.zip".to_string(),
            sha256: Some("no_check".to_string()),
            artifacts: Vec::new(),
        }
    }

    #[test]
    fn parses_app_artifact_targets() {
        let value: Value =
            serde_json::json!({"app": ["Firefox.app", {"target": "Firefox Nightly.app"}]});
        assert_eq!(
            parse_app_artifact(&value),
            Some(AppArtifact {
                source: "Firefox.app".to_string(),
                target: Some("Firefox Nightly.app".to_string())
            })
        );
    }

    #[test]
    fn parses_binary_artifact_targets() {
        let value: Value =
            serde_json::json!({"binary": ["op"], "target": "$HOMEBREW_PREFIX/bin/op"});
        assert_eq!(
            parse_binary_artifact(&value),
            Some(BinaryArtifact {
                source: "op".to_string(),
                target: Some("$HOMEBREW_PREFIX/bin/op".to_string())
            })
        );
    }

    #[test]
    fn parses_binary_artifacts_and_ignores_completion_generation() -> Result<()> {
        let mut cask = test_cask("1password-cli", "2.34.1");
        cask.artifacts = vec![
            serde_json::json!({"binary": ["op"], "target": "$HOMEBREW_PREFIX/bin/op"}),
            serde_json::json!({
                "generate_completions_from_executable": [
                    "op",
                    "completion",
                    {"shells": ["bash", "zsh", "fish"]}
                ]
            }),
            serde_json::json!({"zap": [{"trash": "~/.config/op"}]}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                binaries: vec![BinaryArtifact {
                    source: "op".to_string(),
                    target: Some("$HOMEBREW_PREFIX/bin/op".to_string())
                }],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn parses_pkg_artifacts() {
        let value: Value = serde_json::json!({"pkg": ["OpenJDK.pkg"]});
        assert_eq!(
            parse_pkg_artifact(&value).unwrap(),
            Some(PkgArtifact {
                source: "OpenJDK.pkg".to_string()
            })
        );
    }

    #[test]
    fn rejects_pkg_installer_choices() {
        let value: Value = serde_json::json!({
            "pkg": [
                "VirtualBox.pkg",
                {"choices": [{"choiceIdentifier": "choiceVBox", "attributeSetting": 1}]}
            ]
        });
        assert!(parse_pkg_artifact(&value).is_err());
    }

    #[test]
    fn parses_uninstall_pkgutil_ids() -> Result<()> {
        let mut cask = test_cask("temurin", "26.0.1,8");
        cask.artifacts = vec![
            serde_json::json!({"uninstall": [{"pkgutil": "net.temurin.26.jdk"}]}),
            serde_json::json!({"pkg": ["OpenJDK26U-jdk.pkg"]}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                pkgs: vec![PkgArtifact {
                    source: "OpenJDK26U-jdk.pkg".to_string()
                }],
                pkg_ids: vec!["net.temurin.26.jdk".to_string()],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn parses_zap_pkgutil_ids() -> Result<()> {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({"zap": [{"pkgutil": ["com.example.pkg"]}]}),
            serde_json::json!({"pkg": ["Example.pkg"]}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                pkgs: vec![PkgArtifact {
                    source: "Example.pkg".to_string()
                }],
                pkg_ids: vec!["com.example.pkg".to_string()],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_pkg_artifacts_without_pkgutil_ids() {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![serde_json::json!({"pkg": ["Example.pkg"]})];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("pkg artifacts require pkgutil ids"));
    }

    #[test]
    fn app_only_casks_ignore_pkgutil_ids() -> Result<()> {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({"uninstall": [{"pkgutil": "com.example.helper"}]}),
            serde_json::json!({"app": "Example.app"}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                apps: vec![AppArtifact {
                    source: "Example.app".to_string(),
                    target: None,
                }],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn binary_targets_default_to_prefix_bin() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        assert_eq!(binary_target_path("op")?, tmp.path().join("bin/op"));
        assert_eq!(binary_target_path("sbin/op")?, tmp.path().join("sbin/op"));
        assert_eq!(
            binary_target_path("$HOMEBREW_PREFIX/bin/op")?,
            tmp.path().join("bin/op")
        );
        Ok(())
    }

    #[test]
    fn binary_targets_must_stay_under_prefix() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        let err = binary_target_path("/usr/local/bin/op")
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be under"));
        let err = binary_target_path("../op").unwrap_err().to_string();
        assert!(err.contains("must not contain '..'"));
        Ok(())
    }

    #[test]
    fn caskroom_binary_paths_preserve_prefix_relative_target() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/example/1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/sbin/op".to_string()),
        };

        assert_eq!(
            caskroom_binary_path(&caskroom, &binary)?,
            caskroom.join("sbin/op")
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_ignores_receipt_pkg_ids_for_app_only_casks() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("app-only", "1.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        file::create_dir_all(app_target_path(app.target_name())?)?;
        let receipt = CaskReceipt {
            version: cask.version.clone(),
            apps: vec![app_target_path(app.target_name())?],
            binaries: vec![],
            pkg_ids: vec!["com.example.helper".to_string()],
        };
        crate::file::write(
            caskroom.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;

        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    apps: vec![app],
                    ..Default::default()
                }
            )?,
            Some("1.0.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_checks_binaries_without_receipt() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("binary-only", "1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };
        file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;

        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    binaries: vec![binary.clone()],
                    ..Default::default()
                }
            )?,
            None
        );

        let target = binary.target_path()?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "binary")?;

        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    binaries: vec![binary],
                    ..Default::default()
                }
            )?,
            Some("1.0.0".to_string())
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_and_links_binary_artifact() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        crate::file::write(stage.join("op"), "binary")?;
        let caskroom = caskroom_version_dir("binary-only", "1.0.0");
        file::create_dir_all(&caskroom)?;
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };

        stage_binary(&stage, &caskroom, &binary)?;
        link_binary(&caskroom, &binary)?;

        let target = binary.target_path()?;
        assert_eq!(std::fs::read_link(&target)?, caskroom.join("bin/op"));
        assert_eq!(crate::file::read_to_string(&target)?, "binary");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_same_basename_binaries_without_collision() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(stage.join("bin"))?;
        file::create_dir_all(stage.join("sbin"))?;
        crate::file::write(stage.join("bin/op"), "bin")?;
        crate::file::write(stage.join("sbin/op"), "sbin")?;
        let caskroom = caskroom_version_dir("binary-only", "1.0.0");
        file::create_dir_all(&caskroom)?;
        let bin = BinaryArtifact {
            source: "bin/op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };
        let sbin = BinaryArtifact {
            source: "sbin/op".to_string(),
            target: Some("$HOMEBREW_PREFIX/sbin/op".to_string()),
        };

        stage_binary(&stage, &caskroom, &bin)?;
        stage_binary(&stage, &caskroom, &sbin)?;
        link_binary(&caskroom, &bin)?;
        link_binary(&caskroom, &sbin)?;

        assert_eq!(crate::file::read_to_string(bin.target_path()?)?, "bin");
        assert_eq!(crate::file::read_to_string(sbin.target_path()?)?, "sbin");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn remove_obsolete_binary_links_removes_only_caskroom_symlinks() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("binary-only", "2.0.0");
        let old_caskroom = caskroom_version_dir(&cask.token, "1.0.0");
        file::create_dir_all(old_caskroom.join("bin"))?;
        crate::file::write(old_caskroom.join("bin/old"), "old")?;
        let old_target = tmp.path().join("bin/old");
        file::create_dir_all(old_target.parent().unwrap())?;
        file::make_symlink(&old_caskroom.join("bin/old"), &old_target)?;

        let external = tmp.path().join("external/outside");
        file::create_dir_all(external.parent().unwrap())?;
        crate::file::write(&external, "outside")?;
        let external_target = tmp.path().join("bin/outside");
        file::make_symlink(&external, &external_target)?;

        remove_obsolete_binary_links(
            &cask,
            &[old_target.clone(), external_target.clone()],
            &[tmp.path().join("bin/new")],
        )?;

        assert!(old_target.symlink_metadata().is_err());
        assert!(external_target.symlink_metadata().is_ok());
        Ok(())
    }

    #[test]
    fn installed_cask_version_checks_current_pkg_ids_with_old_receipt() -> Result<()> {
        if crate::file::which("pkgutil").is_none() {
            return Ok(());
        }
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("pkg-only", "1.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            version: cask.version.clone(),
            apps: vec![],
            binaries: vec![],
            pkg_ids: vec![],
        };
        crate::file::write(
            caskroom.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;

        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    pkgs: vec![PkgArtifact {
                        source: "Example.pkg".to_string(),
                    }],
                    pkg_ids: vec!["com.example.missing".to_string()],
                    ..Default::default()
                }
            )?,
            None
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_checks_apps_without_receipt() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("actual-token", "1.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;

        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    apps: vec![app.clone()],
                    ..Default::default()
                }
            )?,
            None
        );

        file::create_dir_all(app_target_path(app.target_name())?)?;
        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    apps: vec![app],
                    ..Default::default()
                }
            )?,
            Some("1.0.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_uses_metadata_token() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("metadata-token", "2.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        file::create_dir_all(caskroom_version_dir("configured-name", &cask.version))?;
        file::create_dir_all(app_target_path(app.target_name())?)?;

        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    apps: vec![app],
                    ..Default::default()
                }
            )?,
            None
        );

        file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;
        assert_eq!(
            installed_cask_version(
                &cask,
                &CaskArtifacts {
                    apps: vec![AppArtifact {
                        source: "Example.app".to_string(),
                        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
                    }],
                    ..Default::default()
                }
            )?,
            Some("2.0.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn remove_stale_versions_keeps_current_version() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token_dir = caskroom_token_dir("actual-token");
        file::create_dir_all(token_dir.join("1.0.0"))?;
        file::create_dir_all(token_dir.join("2.0.0"))?;

        remove_stale_versions(&token_dir, "2.0.0")?;

        assert!(!token_dir.join("1.0.0").exists());
        assert!(token_dir.join("2.0.0").exists());
        Ok(())
    }
}
