use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CaskReceipt {
    version: String,
    apps: Vec<PathBuf>,
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
        let apps = app_artifacts(&cask)?;
        if installed_cask_version(&cask, &apps)?.as_deref() == Some(cask.version.as_str()) {
            info!("brew-cask:{}: already installed", cask.token);
            return Ok(cask.version);
        }
        if opts.dry_run {
            miseprintln!("install cask {}/{}", cask.token, cask.version);
            for app in &apps {
                miseprintln!("link app {}", app.target_name());
            }
            return Ok(cask.version);
        }
        prefix::bootstrap(false)?;
        let archive = fetch_archive(&cask, pr).await?;
        let stage = extract_archive(&cask, &archive, pr)?;
        let caskroom_token = caskroom_token_dir(&cask.token);
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let tmp_caskroom = caskroom_tmp_dir(&cask);
        file::remove_all(&tmp_caskroom)?;
        file::create_dir_all(&tmp_caskroom)?;
        for app in &apps {
            install_app(&stage, &tmp_caskroom, app)?;
        }
        write_receipt(&tmp_caskroom, &cask, &apps)?;
        file::remove_all(&caskroom)?;
        file::rename(&tmp_caskroom, &caskroom)?;
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
            let apps = app_artifacts(&cask)?;
            let version = installed_cask_version(&cask, &apps)?;
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

fn app_artifacts(cask: &Cask) -> Result<Vec<AppArtifact>> {
    let mut apps = Vec::new();
    for artifact in &cask.artifacts {
        let artifact_type = artifact_type(artifact);
        if is_non_install_artifact(&artifact_type) {
            continue;
        }
        let Some(app) = parse_app_artifact(artifact) else {
            warn!(
                "brew-cask:{}: unsupported artifact type {}",
                cask.token, artifact_type
            );
            continue;
        };
        apps.push(app);
    }
    if apps.is_empty() {
        bail!(
            "brew-cask:{}: no app artifact found; only app-bundle casks are supported",
            cask.token
        );
    }
    Ok(apps)
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
            let target = values
                .get(1)
                .and_then(|v| v.as_object())
                .and_then(|o| o.get("target"))
                .or_else(|| value.as_object().and_then(|o| o.get("target")))
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(AppArtifact { source, target })
        }
        _ => None,
    }
}

fn find_app(root: &Path, name: &str) -> Option<PathBuf> {
    let name_path = Path::new(name);
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry.file_type().is_dir()
                && entry
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

fn installed_cask_version(cask: &Cask, apps: &[AppArtifact]) -> Result<Option<String>> {
    let Some(version) = installed_version(&cask.token) else {
        return Ok(None);
    };
    let version_dir = caskroom_version_dir(&cask.token, &version);
    match read_receipt(&version_dir)? {
        Some(receipt) => {
            if receipt.apps.iter().all(|app| app.exists()) {
                Ok(Some(receipt.version))
            } else {
                Ok(None)
            }
        }
        None => {
            for app in apps {
                if !app_target_path(app.target_name())?.exists() {
                    return Ok(None);
                }
            }
            Ok(Some(version))
        }
    }
}

fn write_receipt(caskroom: &Path, cask: &Cask, apps: &[AppArtifact]) -> Result<()> {
    let receipt = CaskReceipt {
        version: cask.version.clone(),
        apps: apps
            .iter()
            .map(|app| app_target_path(app.target_name()))
            .collect::<Result<Vec<_>>>()?,
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
            installed_cask_version(&cask, std::slice::from_ref(&app))?,
            None
        );

        file::create_dir_all(app_target_path(app.target_name())?)?;
        assert_eq!(
            installed_cask_version(&cask, &[app])?,
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

        assert_eq!(installed_cask_version(&cask, &[app])?, None);

        file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;
        assert_eq!(
            installed_cask_version(
                &cask,
                &[AppArtifact {
                    source: "Example.app".to_string(),
                    target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
                }]
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
