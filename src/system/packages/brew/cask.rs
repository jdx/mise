use std::io::Read;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use eyre::{WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use super::api::RubySourceChecksum;
use super::prefix;
use super::source;
use crate::cmd::CmdLineRunner;
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
const HOMEBREW_CASK_RAW: &str = "https://raw.githubusercontent.com/Homebrew/homebrew-cask";
const CASK_SHIM_RB: &str = include_str!("cask_shim.rb");

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
    #[serde(default)]
    ruby_source_path: Option<String>,
    #[serde(default)]
    ruby_source_checksum: Option<RubySourceChecksum>,
    #[serde(default)]
    tap_git_head: Option<String>,
    #[serde(skip)]
    raw_base: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FontArtifact {
    source: String,
    target: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CaskArtifacts {
    apps: Vec<AppArtifact>,
    binaries: Vec<BinaryArtifact>,
    pkgs: Vec<PkgArtifact>,
    fonts: Vec<FontArtifact>,
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
    fonts: Vec<PathBuf>,
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
            for font in &artifacts.fonts {
                miseprintln!("install font {}", font.source);
            }
            return Ok(cask.version);
        }
        prefix::bootstrap(false)?;
        let previous_binaries = previous_binary_targets(&cask)?;
        let previous_fonts = previous_font_targets(&cask)?;
        let archive = fetch_archive(&cask, pr).await?;
        let stage = extract_archive(&cask, &archive, pr)?;
        let caskroom_token = caskroom_token_dir(&cask.token);
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let tmp_caskroom = caskroom_tmp_dir(&cask);
        file::remove_all(&tmp_caskroom)?;
        file::create_dir_all(&tmp_caskroom)?;
        let appdir = cask_appdir(&artifacts.apps)?;
        execute_lifecycle_hook(&cask, &stage, &appdir, "preflight", pr).await?;
        for app in &artifacts.apps {
            install_app(&stage, &tmp_caskroom, app)?;
        }
        for pkg in &artifacts.pkgs {
            install_pkg(&stage, pkg)?;
        }
        for font in &artifacts.fonts {
            stage_font(&stage, &tmp_caskroom, font)?;
        }
        execute_lifecycle_hook(&cask, &tmp_caskroom, &appdir, "postflight", pr).await?;
        for binary in &artifacts.binaries {
            stage_binary(&stage, &tmp_caskroom, &cask, binary)?;
        }
        write_receipt(&tmp_caskroom, &cask, &artifacts)?;
        file::remove_all(&caskroom)?;
        file::rename(&tmp_caskroom, &caskroom)?;
        for binary in &artifacts.binaries {
            link_binary(&caskroom, binary)?;
        }
        remove_obsolete_binary_links(&cask, &previous_binaries, &binary_targets(&artifacts)?)?;
        for font in &artifacts.fonts {
            link_font(&caskroom, font)?;
        }
        remove_obsolete_fonts(&cask, &previous_fonts, &font_target_paths(&artifacts)?)?;
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
    fn name(&self) -> &str {
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
    let (url, raw_base) = match split_tap_name(name) {
        Some(("homebrew", "cask", token)) => (
            format!("{API_BASE}/cask/{token}.json"),
            Some(HOMEBREW_CASK_RAW.to_string()),
        ),
        Some((owner, tap, token)) => {
            let Some(base) = super::api::tap_raw_base(owner, tap, req.tap_url.as_deref()) else {
                bail!(
                    "brew-cask: unsupported tap URL for '{name}'; only GitHub tap URLs can be fetched directly"
                );
            };
            (
                format!("{base}/api/cask/{token}.json"),
                Some(base.trim_end_matches("/HEAD").to_string()),
            )
        }
        None => (
            format!("{API_BASE}/cask/{name}.json"),
            Some(HOMEBREW_CASK_RAW.to_string()),
        ),
    };
    let mut cask = HTTP_FETCH
        .json_cached::<Cask, _>(url)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to fetch Homebrew cask '{name}' directly. \
                 Tapped casks must publish API metadata at api/cask/<token>.json"
            )
        })?;
    cask.raw_base = raw_base;
    Ok(cask)
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
        // Strip macOS quarantine so it doesn't propagate into extracted/copied artifacts.
        let _ = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&archive)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
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
        let format = cask_extraction_format(archive, filename)?;
        if format == ExtractionFormat::Raw {
            // Raw executable binary — copy it using the original URL filename so find_artifact
            // can match against the binary stanza source name (e.g. "claude").
            let url_filename = archive_filename(&cask.url).unwrap_or_else(|| filename.to_string());
            let dest = extract_dir.join(&url_filename);
            file::copy(archive, &dest)?;
            file::make_executable(&dest)?;
        } else if !format.is_archive() {
            bail!(
                "brew-cask:{}: unsupported archive type for {}",
                cask.token,
                filename
            );
        } else {
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
    }
    Ok(extract_dir)
}

async fn execute_lifecycle_hook(
    cask: &Cask,
    staged_path: &Path,
    appdir: &Path,
    hook: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<()> {
    if !has_lifecycle_hook(cask, hook) {
        return Ok(());
    }
    let ruby = cask_ruby_bin().await?;
    let cask_rb = fetch_cask_rb(cask, pr).await?;
    let shim_path = crate::dirs::CACHE
        .join("system-brew")
        .join("casks")
        .join("mise-brew-cask-shim.rb");
    ensure_cask_shim(&shim_path)?;
    if let Some(pr) = pr {
        pr.set_message(format!("run cask {hook}"));
    }
    let runner = CmdLineRunner::new(&ruby).arg(&shim_path).envs([
        ("MISE_BREW_CASK_FILE", cask_rb.display().to_string()),
        ("MISE_BREW_CASK_TOKEN", cask.token.clone()),
        ("MISE_BREW_CASK_VERSION", cask.version.clone()),
        (
            "MISE_BREW_CASK_STAGED_PATH",
            staged_path.display().to_string(),
        ),
        ("MISE_BREW_CASK_APPDIR", appdir.display().to_string()),
        ("MISE_BREW_PREFIX", prefix::prefix().display().to_string()),
        ("MISE_BREW_CASK_HOOK", hook.to_string()),
    ]);
    let runner = match pr {
        Some(pr) => runner.with_pr(pr),
        None => runner,
    };
    runner
        .execute_async()
        .await
        .wrap_err_with(|| format!("brew-cask:{}: failed to run {hook}", cask.token))
}

async fn cask_ruby_bin() -> Result<PathBuf> {
    if let Some(brew) = file::which("brew")
        && let Ok(output) = tokio::process::Command::new(brew)
            .args(["ruby", "-e", "print RbConfig.ruby"])
            .output()
            .await
        && output.status.success()
        && let Ok(path) = String::from_utf8(output.stdout)
    {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(ruby) = file::which("ruby") {
        return Ok(ruby);
    }
    source::ruby_bin().await
}

fn ensure_cask_shim(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    if file::read_to_string(path).is_ok_and(|contents| contents == CASK_SHIM_RB) {
        return Ok(());
    }
    file::write(path, CASK_SHIM_RB)
}

async fn fetch_cask_rb(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let rb_path = cask.ruby_source_path.as_ref().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: lifecycle hooks require ruby_source_path in API metadata",
            cask.token
        )
    })?;
    let sha256 = cask
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
        .ok_or_else(|| {
            eyre!(
                "brew-cask:{}: lifecycle hooks require ruby_source_checksum in API metadata",
                cask.token
            )
        })?;
    let commit = cask.tap_git_head.as_deref().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: lifecycle hooks require tap_git_head in API metadata",
            cask.token
        )
    })?;
    let raw_base = cask.raw_base.as_deref().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: lifecycle hooks require a GitHub raw source URL",
            cask.token
        )
    })?;
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("cask-source");
    file::create_dir_all(&cache_dir)?;
    let short_sha = sha256.get(..12).unwrap_or(sha256);
    let dest = cache_dir.join(format!("{}-{short_sha}.rb", cask.token));
    if dest.exists() && hash::ensure_checksum(&dest, sha256, None, "sha256").is_ok() {
        return Ok(dest);
    }
    let url = format!("{raw_base}/{commit}/{rb_path}");
    if let Some(pr) = pr {
        pr.set_message(format!("download {rb_path}"));
    }
    HTTP_FETCH.download_file(&url, &dest, pr).await?;
    hash::ensure_checksum(&dest, sha256, pr, "sha256")?;
    Ok(dest)
}

fn cask_extraction_format(archive: &Path, filename: &str) -> Result<ExtractionFormat> {
    let format = ExtractionFormat::from_file_name(filename);
    if format != ExtractionFormat::Raw {
        return Ok(format);
    }
    Ok(detect_extraction_format(archive)?.unwrap_or(format))
}

fn detect_extraction_format(archive: &Path) -> Result<Option<ExtractionFormat>> {
    let mut file = std::fs::File::open(archive)?;
    let mut magic = [0; 8];
    let len = file.read(&mut magic)?;
    let magic = &magic[..len];
    if magic.starts_with(b"PK\x03\x04") {
        return Ok(Some(ExtractionFormat::Zip));
    }
    Ok(None)
}

fn install_app(stage: &Path, caskroom: &Path, app: &AppArtifact) -> Result<()> {
    let source = find_app(stage, &app.source)
        .ok_or_else(|| eyre!("brew-cask: app artifact '{}' was not found", app.source))?;
    let caskroom_app = caskroom.join(app_bundle_name(app.target_name())?);
    file::remove_all(&caskroom_app)?;
    ditto(&source, &caskroom_app)?;
    let target = app_target_path(app.target_name())?;
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    let tmp_target = target.with_extension(format!(
        "mise-tmp-{}",
        crate::hash::hash_to_str(&target.display().to_string())
    ));
    file::remove_all(&tmp_target)?;
    ditto(&caskroom_app, &tmp_target)?;
    // Atomic swap: rename existing target aside before putting the new one in place so that
    // a failure during rename leaves the old app intact rather than leaving nothing.
    let old_target = target.with_extension(format!(
        "mise-old-{}",
        crate::hash::hash_to_str(&target.display().to_string())
    ));
    file::remove_all(&old_target)?;
    if target.exists() {
        file::rename(&target, &old_target)?;
    }
    if let Err(e) = file::rename(&tmp_target, &target) {
        // Restore the old app if the swap failed.
        if old_target.exists() {
            let _ = file::rename(&old_target, &target);
        }
        return Err(e);
    }
    file::remove_all(&old_target)?;
    // Remove macOS quarantine attribute so Gatekeeper doesn't block the app.
    let _ = std::process::Command::new("xattr")
        .args(["-r", "-d", "com.apple.quarantine"])
        .arg(&target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

/// Copy a directory using macOS `ditto`, which preserves resource forks, extended attributes,
/// and HFS+ metadata that a plain recursive copy would strip.
fn ditto(from: &Path, to: &Path) -> Result<()> {
    let status = std::process::Command::new("ditto")
        .arg(from)
        .arg(to)
        .status()
        .wrap_err("failed to run ditto")?;
    if !status.success() {
        bail!(
            "ditto failed copying {} to {}",
            from.display(),
            to.display()
        );
    }
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

fn stage_font(stage: &Path, caskroom: &Path, font: &FontArtifact) -> Result<()> {
    let caskroom_font = caskroom_font_path(caskroom, font)?;
    file::remove_all(&caskroom_font)?;
    if let Some(parent) = caskroom_font.parent() {
        file::create_dir_all(parent)?;
    }
    let source = find_artifact(stage, &font.source)
        .filter(|path| path.is_file())
        .ok_or_else(|| eyre!("brew-cask: font artifact '{}' was not found", font.source))?;
    ditto(&source, &caskroom_font)?;
    Ok(())
}

fn link_font(caskroom: &Path, font: &FontArtifact) -> Result<()> {
    let caskroom_font = caskroom_font_path(caskroom, font)?;
    if !caskroom_font.is_file() {
        bail!("brew-cask: font artifact '{}' was not staged", font.source);
    }
    let target = font_target_path(font)?;
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    // Atomic swap: rename existing font aside before copying the new one so
    // that a failure during copy leaves the old font intact.
    let old_target = target.with_extension(format!(
        "mise-old-{}",
        crate::hash::hash_to_str(&target.display().to_string())
    ));
    file::remove_all(&old_target)?;
    if target.exists() {
        file::rename(&target, &old_target)?;
    }
    if let Err(e) = ditto(&caskroom_font, &target) {
        if old_target.exists() {
            let _ = file::rename(&old_target, &target);
        }
        return Err(e);
    }
    file::remove_all(&old_target)?;
    Ok(())
}

fn caskroom_font_path(caskroom: &Path, font: &FontArtifact) -> Result<PathBuf> {
    let name = font_filename(font)?;
    Ok(caskroom.join(name))
}

fn font_filename(font: &FontArtifact) -> Result<String> {
    match &font.target {
        Some(target) => {
            let home = crate::dirs::HOME.to_string_lossy();
            let mut expanded = target.replace("$HOME", &home);
            if let Some(rest) = expanded.strip_prefix("~/") {
                expanded = home.to_string() + "/" + rest;
            } else if expanded == "~" {
                expanded = home.to_string();
            }
            let expanded_path = Path::new(&expanded);
            if expanded_path.is_absolute() {
                let fonts_dir = crate::dirs::HOME.join("Library").join("Fonts");
                if let Ok(relative) = expanded_path.strip_prefix(&fonts_dir) {
                    return Ok(relative.to_string_lossy().to_string());
                }
                return expanded_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
                    .ok_or_else(|| eyre!("brew-cask: invalid font target '{}'", target));
            }
            Ok(expanded)
        }
        None => Path::new(&font.source)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .ok_or_else(|| eyre!("brew-cask: invalid font source '{}'", font.source)),
    }
}

fn font_target_paths(artifacts: &CaskArtifacts) -> Result<Vec<PathBuf>> {
    artifacts
        .fonts
        .iter()
        .map(font_target_path)
        .collect::<Result<Vec<_>>>()
}

fn previous_font_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    let Some(version) = installed_version(&cask.token) else {
        return Ok(Vec::new());
    };
    let version_dir = caskroom_version_dir(&cask.token, &version);
    Ok(read_receipt(&version_dir)?
        .map(|receipt| receipt.fonts)
        .unwrap_or_default())
}

fn remove_obsolete_fonts(
    cask: &Cask,
    previous_targets: &[PathBuf],
    current_targets: &[PathBuf],
) -> Result<()> {
    let token_dir = file::desymlink_path(&caskroom_token_dir(&cask.token));
    for target in previous_targets {
        if current_targets.contains(target) {
            continue;
        }
        if !target.is_file() {
            continue;
        }
        // Only remove the file if it was staged by us — check that it
        // resides under ~/Library/Fonts and the caskroom still has a
        // staged copy (from the previous version directory).
        let fonts_dir = crate::dirs::HOME.join("Library").join("Fonts");
        if !target.starts_with(&fonts_dir) {
            continue;
        }
        // Check if any version directory under the token dir contains this font
        // filename, indicating it was staged by a previous version of this cask.
        let filename = target.file_name();
        let has_staged_copy = std::fs::read_dir(&token_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
            .any(|entry| filename.is_some_and(|f| entry.path().join(f).is_file()));
        if has_staged_copy {
            file::remove_file(target)?;
        }
    }
    Ok(())
}

fn font_target_path(font: &FontArtifact) -> Result<PathBuf> {
    let name = font_filename(font)?;
    let name_path = Path::new(&name);
    if name_path.is_absolute()
        || name_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || name_path.components().next().is_none()
    {
        bail!("brew-cask: invalid font target '{}'", name);
    }
    Ok(crate::dirs::HOME
        .join("Library")
        .join("Fonts")
        .join(name_path))
}

fn stage_binary(stage: &Path, caskroom: &Path, cask: &Cask, binary: &BinaryArtifact) -> Result<()> {
    let caskroom_binary = caskroom_binary_path(caskroom, binary)?;
    file::remove_all(&caskroom_binary)?;
    if let Some(parent) = caskroom_binary.parent() {
        file::create_dir_all(parent)?;
    }
    if binary.source.contains("$APPDIR") {
        // $APPDIR is the Applications directory where install_app placed the bundle.
        // Symlink into the installed app so the CLI wrapper can trace back to find the app.
        // Check both /Applications and $HOMEBREW_PREFIX/Applications per app_target_path().
        let app_binary = [
            PathBuf::from("/Applications"),
            prefix::prefix().join("Applications"),
        ]
        .iter()
        .map(|appdir| PathBuf::from(binary.source.replace("$APPDIR", &appdir.to_string_lossy())))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            eyre!(
                "brew-cask: binary artifact '{}' was not found",
                binary.source
            )
        })?;
        file::make_symlink(&app_binary, &caskroom_binary)?;
    } else {
        let source = find_binary_source(stage, caskroom, cask, binary)?;
        if source.starts_with(stage) || source.starts_with(caskroom) {
            file::copy(&source, &caskroom_binary)?;
            file::make_executable(&caskroom_binary)?;
        } else {
            file::make_symlink(&source, &caskroom_binary)?;
        }
    }
    Ok(())
}

fn find_binary_source(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    binary: &BinaryArtifact,
) -> Result<PathBuf> {
    // Homebrew API often records preflight/postflight wrappers as
    // `$HOMEBREW_PREFIX/Caskroom/<token>/<version>/<name>`. Map that final
    // path onto:
    //   1) temp caskroom (postflight, or preflight if staged there)
    //   2) extract stage (preflight runs with staged_path = stage; e.g. VLC)
    for root in [caskroom, stage] {
        if let Some(source) = generated_caskroom_artifact(root, cask, &binary.source)
            && source.is_file()
        {
            return Ok(source);
        }
    }
    if let Some(source) = absolute_binary_source(&binary.source)
        && source.is_file()
    {
        return Ok(source);
    }
    find_artifact(caskroom, &binary.source)
        .or_else(|| find_artifact(stage, &binary.source))
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            eyre!(
                "brew-cask: binary artifact '{}' was not found",
                binary.source
            )
        })
}

fn absolute_binary_source(source: &str) -> Option<PathBuf> {
    let prefix = prefix::prefix();
    let source = source.replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy());
    let source = PathBuf::from(source);
    source.is_absolute().then_some(source)
}

fn generated_caskroom_artifact(caskroom: &Path, cask: &Cask, source: &str) -> Option<PathBuf> {
    let prefix = prefix::prefix();
    let source = source.replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy());
    let source = PathBuf::from(source);
    let final_caskroom = caskroom_version_dir(&cask.token, &cask.version);
    let relative = source.strip_prefix(final_caskroom).ok()?;
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    Some(caskroom.join(relative))
}

fn cask_appdir(apps: &[AppArtifact]) -> Result<PathBuf> {
    let prefix_app_dir = prefix::prefix().join("Applications");
    for app in apps {
        if app_target_path(app.target_name())?.starts_with(&prefix_app_dir) {
            return Ok(prefix_app_dir);
        }
    }
    Ok(PathBuf::from("/Applications"))
}

fn link_binary(caskroom: &Path, binary: &BinaryArtifact) -> Result<()> {
    let caskroom_binary = caskroom_binary_path(caskroom, binary)?;
    if !caskroom_binary.is_file() {
        if caskroom_binary
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            let target = std::fs::read_link(&caskroom_binary)?;
            bail!(
                "brew-cask: binary artifact '{}' was staged but symlink target '{}' does not exist",
                binary.source,
                target.display()
            );
        }
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
        if let Some(font) = parse_font_artifact(artifact) {
            artifacts.fonts.push(font);
            continue;
        }
        bail!(
            "brew-cask:{}: unsupported artifact type {}",
            cask.token,
            artifact_type
        );
    }
    if artifacts.apps.is_empty()
        && artifacts.binaries.is_empty()
        && artifacts.pkgs.is_empty()
        && artifacts.fonts.is_empty()
    {
        bail!(
            "brew-cask:{}: no app, binary, pkg, or font artifact found; only app-bundle, binary, pkg, and font casks are supported",
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

fn parse_font_artifact(value: &Value) -> Option<FontArtifact> {
    let font = value.as_object()?.get("font")?;
    match font {
        Value::String(source) => Some(FontArtifact {
            source: source.clone(),
            target: None,
        }),
        Value::Array(values) => {
            let source = values.first()?.as_str()?.to_string();
            let target = artifact_target(value, values);
            Some(FontArtifact { source, target })
        }
        _ => None,
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
    // Prefer exact path match (preserves Homebrew's declared casing).
    if let Some(path) = WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "__MACOSX")
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .is_ok_and(|relative| relative.ends_with(name_path))
        })
        .map(|entry| entry.into_path())
    {
        return Some(path);
    }
    // macOS APFS is usually case-insensitive; Homebrew succeeds when the
    // cask declares `app "yaak.app"` but the DMG ships `Yaak.app`.
    // Match case-insensitively only as a fallback so we do not change
    // case-sensitive filesystems' exact-match preference.
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "__MACOSX")
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .is_ok_and(|relative| path_ends_with_ignore_ascii_case(relative, name_path))
        })
        .map(|entry| entry.into_path())
}

/// True when `path`'s trailing components match `suffix` with ASCII
/// case-insensitive comparison of normal path components.
fn path_ends_with_ignore_ascii_case(path: &Path, suffix: &Path) -> bool {
    let path: Vec<_> = path.components().collect();
    let suffix: Vec<_> = suffix.components().collect();
    if suffix.is_empty() || suffix.len() > path.len() {
        return false;
    }
    path[path.len() - suffix.len()..]
        .iter()
        .zip(suffix.iter())
        .all(|(a, b)| match (a, b) {
            (std::path::Component::Normal(a), std::path::Component::Normal(b)) => a
                .to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy()),
            _ => a == b,
        })
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
            let name = entry.file_name().into_string().ok()?;
            entry
                .file_type()
                .ok()
                .filter(|ft| ft.is_dir() && name != ".metadata" && !name.starts_with(".mise-tmp-"))
                .map(|_| name)
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
            let font_targets = if receipt.fonts.is_empty() {
                artifacts
                    .fonts
                    .iter()
                    .map(font_target_path)
                    .collect::<Result<Vec<_>>>()?
            } else {
                receipt.fonts
            };
            if app_targets.iter().all(|app| app.exists())
                && binary_targets.iter().all(|binary| binary.exists())
                && pkgs_installed
                && font_targets.iter().all(|font| font.exists())
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
            for font in &artifacts.fonts {
                if !font_target_path(font)?.exists() {
                    return Ok(None);
                }
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
        fonts: artifacts
            .fonts
            .iter()
            .map(font_target_path)
            .collect::<Result<Vec<_>>>()?,
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
        let name = entry.file_name();
        if entry.file_type().is_ok_and(|ft| ft.is_dir())
            && name.to_str() != Some(current_version)
            && name != ".metadata"
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
        "bash_completion"
            | "caveats"
            | "conflicts_with"
            | "depends_on"
            | "fish_completion"
            | "generate_completions_from_executable"
            | "manpage"
            | "postflight"
            | "preflight"
            | "uninstall"
            | "uninstall_postflight"
            | "uninstall_preflight"
            | "zap"
            | "zsh_completion"
    )
}

fn has_lifecycle_hook(cask: &Cask, hook: &str) -> bool {
    cask.artifacts
        .iter()
        .any(|artifact| artifact_type(artifact) == hook)
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

    fn run_cask_shim(
        ruby: &Path,
        shim: &Path,
        cask: &Path,
        staged_path: &Path,
        version: &str,
    ) -> std::io::Result<std::process::Output> {
        std::process::Command::new(ruby)
            .arg(shim)
            .env("LANG", "zz_ZZ.UTF-8")
            .env("MISE_BREW_CASK_FILE", cask)
            .env("MISE_BREW_CASK_TOKEN", "example")
            .env("MISE_BREW_CASK_VERSION", version)
            .env("MISE_BREW_CASK_STAGED_PATH", staged_path)
            .env("MISE_BREW_CASK_APPDIR", staged_path)
            .env("MISE_BREW_PREFIX", staged_path)
            .env("MISE_BREW_CASK_HOOK", "preflight")
            .output()
    }

    fn test_cask(token: &str, version: &str) -> Cask {
        Cask {
            token: token.to_string(),
            version: version.to_string(),
            url: "https://example.com/example.zip".to_string(),
            sha256: Some("no_check".to_string()),
            artifacts: Vec::new(),
            ruby_source_path: None,
            ruby_source_checksum: None,
            tap_git_head: None,
            raw_base: None,
        }
    }

    #[test]
    fn ensure_cask_shim_creates_parent_dir() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let shim_path = tmp.path().join("missing").join("cask_shim.rb");

        ensure_cask_shim(&shim_path)?;

        assert_eq!(file::read_to_string(&shim_path)?, CASK_SHIM_RB);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cask_shim_supports_language_and_system_conditionals() -> Result<()> {
        let Some(ruby) = file::which("ruby") else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let shim = tmp.path().join("cask_shim.rb");
        let cask = tmp.path().join("example.rb");
        let result = tmp.path().join("result");
        file::write(&shim, CASK_SHIM_RB)?;
        file::write(
            &cask,
            r##"cask "example" do
  version "1.0.0"
  language "fr" do
    "fr"
  end
  language "en", default: true do
    "en-US"
  end
  suffix = on_system_conditional linux: "-linux", macos: "-macos"
  preflight do
    File.write staged_path/"result", "#{language}#{suffix}"
  end
end
"##,
        )?;

        let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let suffix = if cfg!(target_os = "macos") {
            "-macos"
        } else {
            "-linux"
        };
        assert_eq!(file::read_to_string(result)?, format!("en-US{suffix}"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cask_shim_supports_csv_version_array_helpers() -> Result<()> {
        let Some(ruby) = file::which("ruby") else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let shim = tmp.path().join("cask_shim.rb");
        let cask = tmp.path().join("example.rb");
        let result = tmp.path().join("result");
        file::write(&shim, CASK_SHIM_RB)?;
        file::write(
            &cask,
            r#"cask "example" do
  version "2.2.1,20628"
  url "https://example.com/OrbStack_v#{version.csv.first}_#{version.csv.second}.dmg"
  auto_updates true
  preflight do
    File.write staged_path/"result", version.csv.second
  end
end
"#,
        )?;

        let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "2.2.1,20628")?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(file::read_to_string(result)?, "20628");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cask_shim_reports_missing_system_conditional() -> Result<()> {
        let Some(ruby) = file::which("ruby") else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let shim = tmp.path().join("cask_shim.rb");
        let cask = tmp.path().join("example.rb");
        let (conditional, platform) = if cfg!(target_os = "macos") {
            ("linux: \"-linux\"", "macos")
        } else {
            ("macos: \"-macos\"", "linux")
        };
        file::write(&shim, CASK_SHIM_RB)?;
        file::write(
            &cask,
            format!("cask \"example\" do\n  on_system_conditional {conditional}\nend\n"),
        )?;

        let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
            "Error: cask uses `on_system_conditional without {platform}`"
        )));
        Ok(())
    }

    #[test]
    fn detects_suffixless_zip_archives() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("stable");
        std::fs::write(&archive, b"PK\x03\x04suffixless zip")?;

        assert_eq!(
            cask_extraction_format(&archive, "visual-studio-code-1.127.0-stable")?,
            ExtractionFormat::Zip
        );
        Ok(())
    }

    #[test]
    fn leaves_suffixless_raw_binaries_raw() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("claude");
        std::fs::write(&archive, b"#!/bin/sh\necho raw\n")?;

        assert_eq!(
            cask_extraction_format(&archive, "claude-1.0.0-claude")?,
            ExtractionFormat::Raw
        );
        Ok(())
    }

    #[test]
    fn artifact_lookup_ignores_macos_metadata_directories() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let metadata_app = tmp.path().join("__MACOSX/Pearcleaner.app");
        file::create_dir_all(&metadata_app)?;

        assert_eq!(find_app(tmp.path(), "Pearcleaner.app"), None);

        let app = tmp.path().join("Pearcleaner.app");
        file::create_dir_all(&app)?;

        assert_eq!(find_app(tmp.path(), "Pearcleaner.app"), Some(app));
        Ok(())
    }

    #[test]
    fn artifact_lookup_matches_app_bundle_case_insensitively() -> Result<()> {
        // Homebrew cask `yaak` declares `app "yaak.app"` but the DMG ships
        // `Yaak.app`. APFS is case-insensitive; exact match must not be required.
        let tmp = tempfile::tempdir()?;
        let app = tmp.path().join("Yaak.app");
        file::create_dir_all(&app)?;

        assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app.clone()));
        assert_eq!(find_app(tmp.path(), "Yaak.app"), Some(app));
        assert_eq!(find_app(tmp.path(), "Other.app"), None);
        Ok(())
    }

    #[test]
    fn maps_preflight_generated_wrapper_from_extract_stage() -> Result<()> {
        // VLC: preflight writes `#{staged_path}/vlc.wrapper.sh` while preflight
        // staged_path is the extract stage, not the temp Caskroom. API binary
        // source is `$HOMEBREW_PREFIX/Caskroom/vlc/<ver>/vlc.wrapper.sh`.
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("vlc", "3.0.23");
        let stage = tmp.path().join("extract");
        let tmp_caskroom = tmp.path().join("tmp-caskroom");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&tmp_caskroom)?;
        let wrapper = stage.join("vlc.wrapper.sh");
        std::fs::write(&wrapper, "#!/bin/sh\n")?;

        let binary = BinaryArtifact {
            source: "$HOMEBREW_PREFIX/Caskroom/vlc/3.0.23/vlc.wrapper.sh".to_string(),
            target: Some("vlc".to_string()),
        };

        assert_eq!(
            find_binary_source(&stage, &tmp_caskroom, &cask, &binary)?,
            wrapper
        );
        Ok(())
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
    fn detects_lifecycle_hooks() {
        let mut cask = test_cask("gimp", "3.2.4");
        cask.artifacts = vec![
            serde_json::json!({"preflight": null}),
            serde_json::json!({"app": ["GIMP.app"]}),
        ];

        assert!(has_lifecycle_hook(&cask, "preflight"));
        assert!(!has_lifecycle_hook(&cask, "postflight"));
    }

    #[test]
    fn maps_generated_caskroom_binary_to_temp_caskroom() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("gimp", "3.2.4");
        let tmp_caskroom = tmp.path().join("tmp-caskroom");
        let generated = tmp_caskroom.join("gimp.wrapper.sh");
        file::create_dir_all(&tmp_caskroom)?;
        std::fs::write(&generated, "#!/bin/sh\n")?;

        let source = "$HOMEBREW_PREFIX/Caskroom/gimp/3.2.4/gimp.wrapper.sh";

        assert_eq!(
            generated_caskroom_artifact(&tmp_caskroom, &cask, source),
            Some(generated)
        );
        Ok(())
    }

    #[test]
    fn rejects_generated_caskroom_binary_parent_dirs() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("gimp", "3.2.4");
        let tmp_caskroom = tmp.path().join("tmp-caskroom");
        let source = "$HOMEBREW_PREFIX/Caskroom/gimp/3.2.4/../escape";

        assert_eq!(
            generated_caskroom_artifact(&tmp_caskroom, &cask, source),
            None
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
    fn parses_font_artifact() {
        let value: Value = serde_json::json!({"font": "SauceCodeProNerdFont-Regular.ttf"});
        assert_eq!(
            parse_font_artifact(&value),
            Some(FontArtifact {
                source: "SauceCodeProNerdFont-Regular.ttf".to_string(),
                target: None,
            })
        );
    }

    #[test]
    fn parses_font_artifact_with_target() {
        let value: Value = serde_json::json!({"font": ["SauceCodeProNerdFont-Regular.ttf", {"target": "CustomName.ttf"}]});
        assert_eq!(
            parse_font_artifact(&value),
            Some(FontArtifact {
                source: "SauceCodeProNerdFont-Regular.ttf".to_string(),
                target: Some("CustomName.ttf".to_string()),
            })
        );
    }

    #[test]
    fn parses_font_cask_artifacts() -> Result<()> {
        let mut cask = test_cask("font-sauce-code-pro-nerd-font", "3.4.0");
        cask.artifacts = vec![
            serde_json::json!({"font": "SauceCodeProNerdFont-Regular.ttf"}),
            serde_json::json!({"font": "SauceCodeProNerdFont-Bold.ttf"}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                fonts: vec![
                    FontArtifact {
                        source: "SauceCodeProNerdFont-Regular.ttf".to_string(),
                        target: None,
                    },
                    FontArtifact {
                        source: "SauceCodeProNerdFont-Bold.ttf".to_string(),
                        target: None,
                    },
                ],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn skips_bash_completion_and_manpage_artifacts() -> Result<()> {
        let mut cask = test_cask("ghostty", "1.2.0");
        cask.artifacts = vec![
            serde_json::json!({"app": "Ghostty.app"}),
            serde_json::json!({"manpage": ["ghostty.1"]}),
            serde_json::json!({"bash_completion": ["ghostty"]}),
            serde_json::json!({"fish_completion": ["ghostty"]}),
            serde_json::json!({"zsh_completion": ["ghostty"]}),
        ];

        let artifacts = cask_artifacts(&cask)?;
        assert_eq!(artifacts.apps.len(), 1);
        assert_eq!(artifacts.fonts.len(), 0);
        Ok(())
    }

    #[test]
    fn font_only_cask_is_valid() -> Result<()> {
        let mut cask = test_cask("font-test", "1.0.0");
        cask.artifacts = vec![serde_json::json!({"font": "TestFont.ttf"})];

        let artifacts = cask_artifacts(&cask)?;
        assert_eq!(artifacts.fonts.len(), 1);
        Ok(())
    }

    #[test]
    fn font_filename_from_source() -> Result<()> {
        let font = FontArtifact {
            source: "MyFont-Regular.ttf".to_string(),
            target: None,
        };
        assert_eq!(font_filename(&font)?, "MyFont-Regular.ttf");
        Ok(())
    }

    #[test]
    fn font_filename_simple_target() -> Result<()> {
        let font = FontArtifact {
            source: "MyFont.ttf".to_string(),
            target: Some("RenamedFont.ttf".to_string()),
        };
        assert_eq!(font_filename(&font)?, "RenamedFont.ttf");
        Ok(())
    }

    #[test]
    fn font_filename_target_with_home_and_absolute_fonts_path() -> Result<()> {
        // Simulates the JetBrainsMono pattern:
        // target: "/$HOME/Library/Fonts/JetBrainsMonoNerdFontPropo-ThinItalic.ttf"
        let target = "/$HOME/Library/Fonts/JetBrainsMonoNerdFontPropo-ThinItalic.ttf".to_string();
        let font = FontArtifact {
            source: "JetBrainsMonoNerdFontPropo-ThinItalic.ttf".to_string(),
            target: Some(target),
        };
        assert_eq!(
            font_filename(&font)?,
            "JetBrainsMonoNerdFontPropo-ThinItalic.ttf"
        );
        Ok(())
    }

    #[test]
    fn font_filename_target_with_home_expansion() -> Result<()> {
        // $HOME without leading slash: "$HOME/Library/Fonts/Font.ttf"
        let target = "$HOME/Library/Fonts/SomeFont.ttf";
        let font = FontArtifact {
            source: "SomeFont.ttf".to_string(),
            target: Some(target.to_string()),
        };
        assert_eq!(font_filename(&font)?, "SomeFont.ttf");
        Ok(())
    }

    #[test]
    fn font_filename_target_with_tilde_expansion() -> Result<()> {
        // ~/Library/Fonts/Font.ttf should expand to <home>/Library/Fonts/Font.ttf
        let target = "~/Library/Fonts/TildeFont.ttf";
        let font = FontArtifact {
            source: "TildeFont.ttf".to_string(),
            target: Some(target.to_string()),
        };
        assert_eq!(font_filename(&font)?, "TildeFont.ttf");
        Ok(())
    }

    #[test]
    fn font_target_path_from_simple_target() -> Result<()> {
        let font = FontArtifact {
            source: "MyFont.ttf".to_string(),
            target: Some("MyFont.ttf".to_string()),
        };
        let expected = crate::dirs::HOME
            .join("Library")
            .join("Fonts")
            .join("MyFont.ttf");
        assert_eq!(font_target_path(&font)?, expected);
        Ok(())
    }

    #[test]
    fn font_target_path_from_source_only() -> Result<()> {
        let font = FontArtifact {
            source: "FontAwesome.otf".to_string(),
            target: None,
        };
        let expected = crate::dirs::HOME
            .join("Library")
            .join("Fonts")
            .join("FontAwesome.otf");
        assert_eq!(font_target_path(&font)?, expected);
        Ok(())
    }

    #[test]
    fn font_target_path_with_home_absolute_target() -> Result<()> {
        // Regression: absolute target with $HOME under ~/Library/Fonts
        // should resolve to the correct path
        let target = "/$HOME/Library/Fonts/JetBrainsMono.ttf".to_string();
        let font = FontArtifact {
            source: "JetBrainsMono.ttf".to_string(),
            target: Some(target),
        };
        let expected = crate::dirs::HOME
            .join("Library")
            .join("Fonts")
            .join("JetBrainsMono.ttf");
        assert_eq!(font_target_path(&font)?, expected);
        Ok(())
    }

    #[test]
    fn font_target_path_with_tilde_target() -> Result<()> {
        // ~/Library/Fonts/Font.ttf should resolve to correct path
        let target = "~/Library/Fonts/TildeFont.ttf".to_string();
        let font = FontArtifact {
            source: "TildeFont.ttf".to_string(),
            target: Some(target),
        };
        let expected = crate::dirs::HOME
            .join("Library")
            .join("Fonts")
            .join("TildeFont.ttf");
        assert_eq!(font_target_path(&font)?, expected);
        Ok(())
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
            fonts: vec![],
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
        let cask = test_cask("binary-only", "1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &binary)?;
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
        let cask = test_cask("binary-only", "1.0.0");
        let bin = BinaryArtifact {
            source: "bin/op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };
        let sbin = BinaryArtifact {
            source: "sbin/op".to_string(),
            target: Some("$HOMEBREW_PREFIX/sbin/op".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &bin)?;
        stage_binary(&stage, &caskroom, &cask, &sbin)?;
        link_binary(&caskroom, &bin)?;
        link_binary(&caskroom, &sbin)?;

        assert_eq!(crate::file::read_to_string(bin.target_path()?)?, "bin");
        assert_eq!(crate::file::read_to_string(sbin.target_path()?)?, "sbin");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn binary_source_prefers_hook_generated_caskroom_file() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        crate::file::write(stage.join("op"), "stage")?;
        let caskroom = caskroom_version_dir("binary-only", "1.0.0");
        file::create_dir_all(&caskroom)?;
        crate::file::write(caskroom.join("op"), "hook")?;
        let cask = test_cask("binary-only", "1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &binary)?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("bin/op"))?,
            "hook"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_absolute_binary_source_from_pkg_install() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        let pkg_binary = tmp
            .path()
            .join("Library/Application Support/org.pqrs/Karabiner-Elements/bin/karabiner_cli");
        if let Some(parent) = pkg_binary.parent() {
            file::create_dir_all(parent)?;
        }
        crate::file::write(&pkg_binary, "pkg binary")?;
        let caskroom = caskroom_version_dir("karabiner-elements", "16.1.0");
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("karabiner-elements", "16.1.0");
        let binary = BinaryArtifact {
            source: pkg_binary.to_string_lossy().to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/karabiner_cli".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &binary)?;
        link_binary(&caskroom, &binary)?;

        let staged = caskroom.join("bin/karabiner_cli");
        assert_eq!(std::fs::read_link(&staged)?, pkg_binary);
        let target = binary.target_path()?;
        assert_eq!(std::fs::read_link(&target)?, staged);
        assert_eq!(crate::file::read_to_string(&target)?, "pkg binary");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn reports_missing_target_for_dangling_staged_binary_symlink() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        let pkg_binary = tmp
            .path()
            .join("Library/Application Support/org.pqrs/Karabiner-Elements/bin/karabiner_cli");
        if let Some(parent) = pkg_binary.parent() {
            file::create_dir_all(parent)?;
        }
        crate::file::write(&pkg_binary, "pkg binary")?;
        let caskroom = caskroom_version_dir("karabiner-elements", "16.1.0");
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("karabiner-elements", "16.1.0");
        let binary = BinaryArtifact {
            source: pkg_binary.to_string_lossy().to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/karabiner_cli".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &binary)?;
        file::remove_file(&pkg_binary)?;
        let err = link_binary(&caskroom, &binary).unwrap_err().to_string();

        assert!(err.contains("was staged but symlink target"));
        assert!(err.contains(&pkg_binary.to_string_lossy().to_string()));
        Ok(())
    }

    #[test]
    fn cask_appdir_uses_prefix_for_prefix_targeted_apps() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };

        assert_eq!(cask_appdir(&[app])?, tmp.path().join("Applications"));
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
            fonts: vec![],
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
    fn installed_version_ignores_homebrew_metadata() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token_dir = caskroom_token_dir("actual-token");
        file::create_dir_all(token_dir.join("2.0.0"))?;
        file::create_dir_all(token_dir.join(".metadata/2.0.0/timestamp/Casks"))?;
        file::create_dir_all(token_dir.join(".mise-tmp-interrupted"))?;

        assert_eq!(installed_version("actual-token"), Some("2.0.0".to_string()));
        Ok(())
    }

    #[test]
    fn remove_stale_versions_keeps_current_version_and_homebrew_metadata() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token_dir = caskroom_token_dir("actual-token");
        file::create_dir_all(token_dir.join("1.0.0"))?;
        file::create_dir_all(token_dir.join("2.0.0"))?;
        let metadata = token_dir.join(".metadata/2.0.0/timestamp/Casks");
        file::create_dir_all(&metadata)?;
        crate::file::write(metadata.join("actual-token.json"), "metadata")?;

        remove_stale_versions(&token_dir, "2.0.0")?;

        assert!(!token_dir.join("1.0.0").exists());
        assert!(token_dir.join("2.0.0").exists());
        assert_eq!(
            crate::file::read_to_string(metadata.join("actual-token.json"))?,
            "metadata"
        );
        Ok(())
    }
}
