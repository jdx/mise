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
    /// API field `tap` (e.g. `homebrew/cask`). Retained for API fidelity;
    /// not used while mise-owned pours stay Homebrew-invisible (Plan 010).
    #[serde(default)]
    #[allow(dead_code)]
    tap: Option<String>,
    /// Prior tokens (Homebrew cask API `old_tokens`) treated as accepted aliases.
    #[serde(default)]
    old_tokens: Vec<String>,
    /// Optional aliases when present in API payloads.
    #[serde(default)]
    aliases: Vec<String>,
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

/// Schema version for `.mise-cask.toml`.
/// - absent / 0 / 1: legacy intent-derived receipt (LegacyUnverified for handoff)
/// - 2: completed-action manifest published after activation
const CASK_RECEIPT_SCHEMA_V2: u32 = 2;
const CASK_ACTION_MANIFEST_VERSION: u32 = 1;

/// Opaque single path component (token or version). Not ordered or parsed as semver.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SafePathComponent {
    raw: String,
}

impl SafePathComponent {
    fn parse(field: &str, value: &str) -> Result<Self> {
        if value.is_empty() {
            bail!("brew-cask: {field} must not be empty");
        }
        if value.contains('\0') {
            bail!("brew-cask: {field} must not contain NUL");
        }
        if value == "." || value == ".." {
            bail!("brew-cask: invalid {field} '{value}'");
        }
        if value.contains('/') || value.contains('\\') {
            bail!("brew-cask: {field} '{value}' must be a single path component");
        }
        let path = Path::new(value);
        if path.is_absolute() {
            bail!("brew-cask: {field} '{value}' must not be absolute");
        }
        let mut components = path.components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) if name.to_str() == Some(value) => Ok(Self {
                raw: value.to_string(),
            }),
            _ => bail!("brew-cask: invalid {field} '{value}'"),
        }
    }

    fn as_str(&self) -> &str {
        &self.raw
    }
}

impl AsRef<Path> for SafePathComponent {
    fn as_ref(&self) -> &Path {
        Path::new(&self.raw)
    }
}

impl std::fmt::Display for SafePathComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Validated cask token + opaque version used for all path construction.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaskIds {
    token: SafePathComponent,
    version: SafePathComponent,
}

impl CaskIds {
    fn validate(token: &str, version: &str) -> Result<Self> {
        Ok(Self {
            token: SafePathComponent::parse("token", token)?,
            version: SafePathComponent::parse("version", version)?,
        })
    }
}

/// Join `base` with a single validated component; rejects empty/parent/root.
fn checked_join(base: &Path, component: &SafePathComponent) -> PathBuf {
    base.join(component.as_str())
}

/// Lexically normalize an absolute path without resolving symlinks.
/// Rejects empty, relative, and paths that escape via `..` above root.
fn normalize_absolute_components(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!(
            "brew-cask: expected absolute path, got '{}'",
            path.display()
        );
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || out.as_os_str().is_empty() {
                    bail!("brew-cask: path '{}' escapes via '..'", path.display());
                }
            }
            Component::Normal(name) => {
                if name.to_string_lossy().contains('\0') {
                    bail!("brew-cask: path contains NUL");
                }
                out.push(name);
            }
        }
    }
    if !out.is_absolute() {
        bail!("brew-cask: normalized path is not absolute");
    }
    Ok(out)
}

fn path_contained_in_or_eq(path: &Path, root: &Path) -> Result<bool> {
    let path = normalize_absolute_components(path)?;
    let root = normalize_absolute_components(root)?;
    Ok(path.starts_with(&root))
}

/// Reject an existing symlink in a mutation path below an allowed root.
/// Missing components are safe to create later; the final target may itself be
/// replaced, so callers pass its parent when replacement is intentional.
fn reject_symlink_components(root: &Path, path: &Path) -> Result<()> {
    let root = normalize_absolute_components(root)?;
    let path = normalize_absolute_components(path)?;
    let relative = path.strip_prefix(&root).wrap_err_with(|| {
        format!(
            "brew-cask: mutation path '{}' is outside root '{}'",
            path.display(),
            root.display()
        )
    })?;
    let mut current = root;
    if current
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        bail!(
            "brew-cask: mutation root '{}' must not be a symlink",
            current.display()
        );
    }
    for component in relative.components() {
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "brew-cask: mutation path '{}' traverses symlink '{}'",
                    path.display(),
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn validate_mutation_boundaries(ids: &CaskIds, artifacts: &CaskArtifacts) -> Result<()> {
    let prefix = prefix::prefix();
    let caskroom_root = prefix.join("Caskroom");
    reject_symlink_components(&prefix, &caskroom_root)?;
    reject_symlink_components(&caskroom_root, &caskroom_token_dir(&ids.token))?;
    for app in &artifacts.apps {
        let target = app_target_path(app.target_name())?;
        let root = allowed_appdir_roots()
            .into_iter()
            .find(|root| target.starts_with(root))
            .ok_or_else(|| {
                eyre!(
                    "brew-cask: app target '{}' has no allowed root",
                    target.display()
                )
            })?;
        reject_symlink_components(&root, target.parent().unwrap_or(&target))?;
    }
    for binary in &artifacts.binaries {
        let target = binary.target_path()?;
        let root = allowed_binary_target_roots()
            .into_iter()
            .find(|root| target.starts_with(root))
            .ok_or_else(|| {
                eyre!(
                    "brew-cask: binary target '{}' has no allowed root",
                    target.display()
                )
            })?;
        reject_symlink_components(&root, target.parent().unwrap_or(&target))?;
    }
    let fonts_root = crate::dirs::HOME.join("Library").join("Fonts");
    for font in &artifacts.fonts {
        let target = font_target_path(font)?;
        reject_symlink_components(&fonts_root, target.parent().unwrap_or(&target))?;
    }
    Ok(())
}

/// Validate artifact path fields before download/hooks/sudo/mkdir.
fn validate_artifact_paths(artifacts: &CaskArtifacts) -> Result<()> {
    for app in &artifacts.apps {
        // Source is a relative archive path; reject traversal.
        validate_relative_artifact_source("app source", &app.source)?;
        app_target_path(app.target_name())?;
    }
    for binary in &artifacts.binaries {
        if binary.source.contains("$APPDIR") {
            // Expand against allowed Applications roots and fail closed on escape.
            validate_appdir_binary_source(&binary.source)?;
        } else if Path::new(&binary.source).is_absolute() {
            validate_absolute_binary_source(&binary.source)?;
        } else {
            validate_relative_artifact_source("binary source", &binary.source)?;
        }
        binary.target_path()?;
    }
    for pkg in &artifacts.pkgs {
        validate_relative_artifact_source("pkg source", &pkg.source)?;
    }
    for font in &artifacts.fonts {
        if !Path::new(&font.source).is_absolute() {
            validate_relative_artifact_source("font source", &font.source)?;
        }
        font_target_path(font)?;
    }
    Ok(())
}

fn validate_relative_artifact_source(field: &str, source: &str) -> Result<()> {
    if source.is_empty() || source.contains('\0') {
        bail!("brew-cask: invalid {field} '{source}'");
    }
    let path = Path::new(source);
    if path.is_absolute() {
        bail!("brew-cask: {field} '{source}' must be relative");
    }
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("brew-cask: {field} '{source}' must not contain '..' or root");
    }
    if path.components().next().is_none() {
        bail!("brew-cask: invalid {field} '{source}'");
    }
    Ok(())
}

fn allowed_appdir_roots() -> [PathBuf; 2] {
    [
        PathBuf::from("/Applications"),
        prefix::prefix().join("Applications"),
    ]
}

/// `$APPDIR` must be a path prefix. Suffix is relative under Applications only.
/// Rejects `$APPDIR/../secret` and mid-path `$APPDIR` after expand/normalize.
fn validate_appdir_binary_source(source: &str) -> Result<()> {
    let _ = expand_appdir_binary_candidates(source)?;
    Ok(())
}

fn expand_appdir_binary_candidates(source: &str) -> Result<Vec<PathBuf>> {
    if source.is_empty() || source.contains('\0') {
        bail!("brew-cask: invalid $APPDIR binary source '{source}'");
    }
    let relative = if source == "$APPDIR" {
        bail!("brew-cask: binary source '$APPDIR' must name a file under Applications");
    } else if let Some(rest) = source.strip_prefix("$APPDIR/") {
        rest
    } else if source.contains("$APPDIR") {
        bail!("brew-cask: $APPDIR must be the path prefix of binary source '{source}'");
    } else {
        bail!("brew-cask: binary source '{source}' is not an $APPDIR path");
    };
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative_path.is_absolute()
        || relative_path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "brew-cask: $APPDIR binary source '{source}' must be a relative path under Applications without '..'"
        );
    }
    let mut candidates = Vec::with_capacity(2);
    for root in allowed_appdir_roots() {
        let joined = root.join(relative_path);
        let normalized = normalize_absolute_components(&joined)?;
        // Containment after lexical normalize — not raw string replace.
        if !path_contained_in_or_eq(&normalized, &root)? || normalized == root {
            bail!(
                "brew-cask: $APPDIR binary source '{source}' escapes Applications root {}",
                root.display()
            );
        }
        candidates.push(normalized);
    }
    Ok(candidates)
}

/// Resolve `$APPDIR/...` to an existing file under an allowed Applications root.
fn resolve_appdir_binary_source(source: &str) -> Result<PathBuf> {
    let candidates = expand_appdir_binary_candidates(source)?;
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            eyre!("brew-cask: binary artifact '{source}' was not found under Applications")
        })
}

/// Absolute binary sources (e.g. pkg-installed paths) must not contain `..`.
fn validate_absolute_binary_source(source: &str) -> Result<()> {
    if source.contains('\0') {
        bail!("brew-cask: binary source contains NUL");
    }
    let expanded = source.replace("$HOMEBREW_PREFIX", &prefix::prefix().to_string_lossy());
    let path = PathBuf::from(&expanded);
    if !path.is_absolute() {
        bail!("brew-cask: binary source '{source}' must be absolute");
    }
    let normalized = normalize_absolute_components(&path)?;
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("brew-cask: absolute binary source '{source}' must not contain '..'");
    }
    let _ = normalized;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaskActionKind {
    App,
    Binary,
    Font,
    Pkg,
    Hook,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaskActionOperation {
    Copy,
    Symlink,
    PackageInstall,
    Hook,
    Stage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CaskActionPhase {
    Completed,
    /// External side effects (pkg/hooks) mise cannot safely roll back.
    CompletedNonRollbackable,
}

/// One filesystem/package action mise actually completed. Not projected intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CompletedCaskAction {
    id: String,
    kind: CaskActionKind,
    operation: CaskActionOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<PathBuf>,
    phase: CaskActionPhase,
    /// True when mise created/replaced the target; false if only observed.
    mise_created: bool,
    /// Stable non-path identifiers emitted by the mutator (for example pkg ids).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identifiers: Vec<String>,
}

/// Versioned completed-action manifest (mutator truth, not CaskArtifacts intent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CompletedCaskActionManifest {
    manifest_version: u32,
    transaction_id: String,
    token: String,
    /// Opaque version string; never ordered as semver.
    version: String,
    platform: String,
    architecture: String,
    actions: Vec<CompletedCaskAction>,
}

impl CompletedCaskActionManifest {
    fn new(ids: &CaskIds, transaction_id: &str) -> Self {
        Self {
            manifest_version: CASK_ACTION_MANIFEST_VERSION,
            transaction_id: transaction_id.to_string(),
            token: ids.token.as_str().to_string(),
            version: ids.version.as_str().to_string(),
            platform: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            actions: Vec::new(),
        }
    }

    fn validate_known(&self) -> Result<()> {
        if self.manifest_version != CASK_ACTION_MANIFEST_VERSION {
            bail!(
                "brew-cask: unknown completed-action manifest version {}",
                self.manifest_version
            );
        }
        Ok(())
    }
}

fn new_cask_transaction_id(ids: &CaskIds) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hash::hash_to_str(&(ids.token.as_str(), ids.version.as_str(), stamp))
}

/// Prefix-owned recovery root outside Caskroom token/version/`.metadata`.
fn cask_recovery_root() -> PathBuf {
    prefix::prefix()
        .join("var")
        .join("mise")
        .join("cask-recovery")
}

fn action_journal_path(ids: &CaskIds, transaction_id: &str) -> Result<PathBuf> {
    let txn = SafePathComponent::parse("transaction_id", transaction_id)?;
    Ok(checked_join(&checked_join(&cask_recovery_root(), &ids.token), &txn).with_extension("json"))
}

fn write_durable_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "tmp-{}",
        hash::hash_to_str(&path.display().to_string())
    ));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)
            .wrap_err_with(|| format!("failed create {}", tmp.display()))?;
        f.write_all(contents)
            .wrap_err_with(|| format!("failed write {}", tmp.display()))?;
        f.sync_all()
            .wrap_err_with(|| format!("failed fsync {}", tmp.display()))?;
    }
    file::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        file::sync_dir(parent)
            .wrap_err_with(|| format!("failed fsync directory {}", parent.display()))?;
    }
    Ok(())
}

fn write_action_journal(ids: &CaskIds, manifest: &CompletedCaskActionManifest) -> Result<()> {
    manifest.validate_known()?;
    let path = action_journal_path(ids, &manifest.transaction_id)?;
    let body = serde_json::to_vec_pretty(manifest)?;
    write_durable_file(&path, &body)
}

fn record_completed_action(
    ids: &CaskIds,
    manifest: &mut CompletedCaskActionManifest,
    action: CompletedCaskAction,
) -> Result<()> {
    manifest.actions.push(action);
    write_action_journal(ids, manifest)
}

fn clear_action_journal(ids: &CaskIds, transaction_id: &str) -> Result<()> {
    let path = action_journal_path(ids, transaction_id)?;
    if path.exists() {
        file::remove_file(&path)?;
        if let Some(parent) = path.parent() {
            file::sync_dir(parent)
                .wrap_err_with(|| format!("failed fsync directory {}", parent.display()))?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CaskReceipt {
    version: String,
    /// 0/absent = legacy intent-only; 2 = completed-action receipt.
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    apps: Vec<PathBuf>,
    #[serde(default)]
    binaries: Vec<PathBuf>,
    #[serde(default)]
    fonts: Vec<PathBuf>,
    #[serde(default)]
    pkg_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<CompletedCaskAction>,
}

impl CaskReceipt {
    /// Legacy receipts are usable for mise-only status/uninstall facts they
    /// actually contain, but never become interop/handoff eligible automatically.
    #[allow(dead_code)] // exercised in unit tests; handoff gate uses when Plan 012 ships
    fn is_legacy_unverified(&self) -> bool {
        self.schema_version < CASK_RECEIPT_SCHEMA_V2 || self.actions.is_empty()
    }

    #[allow(dead_code)] // exercised in unit tests; handoff gate uses when Plan 012 ships
    fn handoff_eligible(&self) -> bool {
        !self.is_legacy_unverified()
    }
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
        // fetch_cask already enforces request-token equality. Validate path-safe
        // opaque identifiers before any FS I/O.
        let ids = CaskIds::validate(&cask.token, &cask.version)?;
        let artifacts = cask_artifacts(&cask)?;
        validate_artifact_paths(&artifacts)?;
        validate_mutation_boundaries(&ids, &artifacts)?;
        let installed = installed_cask_version(&cask, &artifacts)?;
        if installed.as_deref() == Some(cask.version.as_str()) {
            // Already-installed: never synthesize or repair Homebrew `.metadata`.
            // Foreign Homebrew ledgers are preserved byte-for-byte.
            info!("brew-cask:{}: already installed", cask.token);
            return Ok(cask.version);
        }
        // A Homebrew marker is lifecycle authority, not an identity hint.
        // Never pour over an older/degraded Homebrew-owned cask: doing so would
        // switch the payload to mise while leaving Homebrew's teardown ledger
        // authoritative for different files.
        if homebrew_metadata_present(&ids.token) {
            bail!(
                "brew-cask:{} is managed by Homebrew; use Homebrew to upgrade or uninstall it",
                cask.token
            );
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
        let archive = fetch_archive(&cask, &ids, pr).await?;
        let stage = extract_archive(&cask, &ids, &archive, pr)?;
        let caskroom_token = caskroom_token_dir(&ids.token);
        let caskroom = caskroom_version_dir(&ids.token, &ids.version);
        let tmp_caskroom = caskroom_tmp_dir(&ids);
        file::remove_all(&tmp_caskroom)?;
        file::create_dir_all(&tmp_caskroom)?;
        let appdir = cask_appdir(&artifacts.apps)?;
        let transaction_id = new_cask_transaction_id(&ids);
        let mut completed = CompletedCaskActionManifest::new(&ids, &transaction_id);
        // Establish durable intent before the first installation mutation.
        write_action_journal(&ids, &completed)?;
        if let Some(action) =
            execute_lifecycle_hook(&cask, &stage, &appdir, "preflight", pr).await?
        {
            record_completed_action(&ids, &mut completed, action)?;
        }
        for app in &artifacts.apps {
            let action = install_app(&stage, &tmp_caskroom, &caskroom, app)?;
            record_completed_action(&ids, &mut completed, action)?;
        }
        for pkg in &artifacts.pkgs {
            let action = install_pkg(&stage, pkg, &artifacts.pkg_ids)?;
            record_completed_action(&ids, &mut completed, action)?;
        }
        for font in &artifacts.fonts {
            let action = stage_font(&stage, &tmp_caskroom, &caskroom, font)?;
            record_completed_action(&ids, &mut completed, action)?;
        }
        if let Some(action) =
            execute_lifecycle_hook(&cask, &tmp_caskroom, &appdir, "postflight", pr).await?
        {
            record_completed_action(&ids, &mut completed, action)?;
        }
        for binary in &artifacts.binaries {
            let action = stage_binary(&stage, &tmp_caskroom, &caskroom, &cask, &ids, binary)?;
            record_completed_action(&ids, &mut completed, action)?;
        }
        // Durable journal outside Homebrew-controlled Caskroom/metadata dirs.
        // Crash before final receipt ⇒ Pending with journal, not healthy install.
        write_action_journal(&ids, &completed)?;
        file::remove_all(&caskroom)?;
        file::rename(&tmp_caskroom, &caskroom)?;
        for binary in &artifacts.binaries {
            let action = link_binary(&caskroom, binary)?;
            record_completed_action(&ids, &mut completed, action)?;
        }
        remove_obsolete_binary_links(&cask, &previous_binaries, &binary_targets(&artifacts)?)?;
        for font in &artifacts.fonts {
            let action = link_font(&caskroom, font)?;
            record_completed_action(&ids, &mut completed, action)?;
        }
        remove_obsolete_fonts(&cask, &previous_fonts, &font_target_paths(&artifacts)?)?;
        remove_stale_versions(&caskroom_token, &ids.version)?;
        // Final mise receipt only after required activation succeeds.
        // Never publish synthetic Homebrew `.metadata` — mise-owned pours stay
        // Homebrew-invisible. Existing foreign metadata is never rewritten.
        write_receipt(&caskroom, &completed)?;
        clear_action_journal(&ids, &transaction_id)?;
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
            // Mise status uses the mise/payload ledger only. Missing Homebrew
            // `.metadata` is not "package missing" — normal pours are deliberately
            // Homebrew-invisible. Foreign Homebrew metadata is never required
            // for mise-owned status and is never rewritten by this path.
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

/// Token segment from a package request (`owner/tap/token` → `token`, else bare name).
fn requested_cask_token(req: &PackageRequest) -> &str {
    match split_tap_name(&req.name) {
        Some((_, _, token)) => token,
        None => req.name.as_str(),
    }
}

/// Whether this request is served by the official Homebrew cask registry.
///
/// Only that source may use API `old_tokens`/`aliases` for request matching.
/// Third-party taps must match `cask.token` exactly — their alias lists are
/// untrusted and must not redirect pour identity (hostile-tap wrong-identity).
fn trust_homebrew_cask_api_aliases(req: &PackageRequest) -> bool {
    match split_tap_name(&req.name) {
        Some(("homebrew", "cask", _)) => true,
        Some(_) => false,
        // Bare names are fetched only from formulae.brew.sh official cask API.
        None => true,
    }
}

/// Request identity check.
///
/// - Always accept exact `cask.token == requested`.
/// - Accept API `old_tokens`/`aliases` **only** when `trust_api_aliases` is true
///   (official homebrew/cask). Never trust those lists from third-party taps.
fn cask_token_matches_request(cask: &Cask, requested: &str, trust_api_aliases: bool) -> bool {
    if cask.token == requested {
        return true;
    }
    if !trust_api_aliases {
        return false;
    }
    cask.old_tokens.iter().any(|t| t == requested) || cask.aliases.iter().any(|t| t == requested)
}

/// Fail closed before any path/FS work: response identity must answer the request.
fn ensure_cask_token_matches_request(cask: &Cask, req: &PackageRequest) -> Result<()> {
    let requested = requested_cask_token(req);
    let trust_aliases = trust_homebrew_cask_api_aliases(req);
    if cask_token_matches_request(cask, requested, trust_aliases) {
        return Ok(());
    }
    bail!(
        "brew-cask: API token '{}' does not match requested token '{}' (request '{}')",
        cask.token,
        requested,
        req.name
    );
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
    // Before any path construction or FS mutation: API identity must match request.
    ensure_cask_token_matches_request(&cask, req)?;
    Ok(cask)
}

async fn fetch_archive(
    cask: &Cask,
    ids: &CaskIds,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let filename = archive_filename(&cask.url)
        .ok_or_else(|| eyre!("brew-cask:{}: URL has no file name", cask.token))?;
    // Cache basename must be a single safe component (no path separators).
    let safe_filename = SafePathComponent::parse("archive filename", &filename)
        .map(|c| c.raw)
        .unwrap_or_else(|_| {
            // URL may encode weird names; fall back to hash-only name.
            format!("{}.bin", &hash::hash_sha256_to_str(&cask.url)[..16])
        });
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("casks");
    file::create_dir_all(&cache_dir)?;
    let url_hash = &hash::hash_sha256_to_str(&cask.url)[..12];
    let archive = cache_dir.join(format!(
        "{}-{}-{url_hash}-{safe_filename}",
        ids.token.as_str(),
        ids.version.as_str()
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

fn extract_archive(
    cask: &Cask,
    ids: &CaskIds,
    archive: &Path,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let extract_key = format!("{}-{}", ids.token.as_str(), ids.version.as_str());
    let extract_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-extract")
        .join(extract_key);
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
            // Raw executable binary — copy it using the original URL filename so
            // find_file_artifact can match against the binary stanza source name (e.g. "claude").
            let url_filename = archive_filename(&cask.url).unwrap_or_else(|| filename.to_string());
            let dest_name = Path::new(&url_filename)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("payload");
            let dest = extract_dir.join(dest_name);
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

/// Returns a fact only after the named hook executed successfully.
async fn execute_lifecycle_hook(
    cask: &Cask,
    staged_path: &Path,
    appdir: &Path,
    hook: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<Option<CompletedCaskAction>> {
    if !has_lifecycle_hook(cask, hook) {
        return Ok(None);
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
        .wrap_err_with(|| format!("brew-cask:{}: failed to run {hook}", cask.token))?;
    Ok(Some(CompletedCaskAction {
        id: format!("hook:{hook}"),
        kind: CaskActionKind::Hook,
        operation: CaskActionOperation::Hook,
        source: Some(cask_rb),
        target: None,
        phase: CaskActionPhase::CompletedNonRollbackable,
        mise_created: false,
        identifiers: Vec::new(),
    }))
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

fn install_app(
    stage: &Path,
    caskroom: &Path,
    retained_caskroom: &Path,
    app: &AppArtifact,
) -> Result<CompletedCaskAction> {
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
    let replaced = target.exists();
    if replaced {
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
    Ok(CompletedCaskAction {
        id: format!("app:{}", app.target_name()),
        kind: CaskActionKind::App,
        operation: CaskActionOperation::Copy,
        source: Some(retained_caskroom.join(app_bundle_name(app.target_name())?)),
        target: Some(target),
        phase: CaskActionPhase::Completed,
        mise_created: true,
        identifiers: Vec::new(),
    })
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

fn install_pkg(stage: &Path, pkg: &PkgArtifact, pkg_ids: &[String]) -> Result<CompletedCaskAction> {
    let source = find_file_artifact(stage, &pkg.source)
        .ok_or_else(|| eyre!("brew-cask: pkg artifact '{}' was not found", pkg.source))?;
    let args = vec![
        "-pkg".to_string(),
        source.display().to_string(),
        "-target".to_string(),
        "/".to_string(),
    ];
    sudo::run("installer", &args, &[])?;
    Ok(CompletedCaskAction {
        id: format!("pkg:{}", pkg.source),
        kind: CaskActionKind::Pkg,
        operation: CaskActionOperation::PackageInstall,
        source: Some(source),
        target: None,
        phase: CaskActionPhase::CompletedNonRollbackable,
        mise_created: true,
        identifiers: pkg_ids.to_vec(),
    })
}

fn stage_font(
    stage: &Path,
    caskroom: &Path,
    retained_caskroom: &Path,
    font: &FontArtifact,
) -> Result<CompletedCaskAction> {
    let caskroom_font = caskroom_font_path(caskroom, font)?;
    file::remove_all(&caskroom_font)?;
    if let Some(parent) = caskroom_font.parent() {
        file::create_dir_all(parent)?;
    }
    let source = find_file_artifact(stage, &font.source)
        .ok_or_else(|| eyre!("brew-cask: font artifact '{}' was not found", font.source))?;
    ditto(&source, &caskroom_font)?;
    Ok(CompletedCaskAction {
        id: format!("font-stage:{}", font.source),
        kind: CaskActionKind::Font,
        operation: CaskActionOperation::Stage,
        source: Some(source),
        target: Some(caskroom_font_path(retained_caskroom, font)?),
        phase: CaskActionPhase::Completed,
        mise_created: true,
        identifiers: Vec::new(),
    })
}

fn link_font(caskroom: &Path, font: &FontArtifact) -> Result<CompletedCaskAction> {
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
    Ok(CompletedCaskAction {
        id: format!("font:{}", font_filename(font)?),
        kind: CaskActionKind::Font,
        operation: CaskActionOperation::Copy,
        source: Some(caskroom_font),
        target: Some(target),
        phase: CaskActionPhase::Completed,
        mise_created: true,
        identifiers: Vec::new(),
    })
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
    let ids = match CaskIds::validate(&cask.token, &version) {
        Ok(ids) => ids,
        Err(_) => return Ok(Vec::new()),
    };
    let version_dir = caskroom_version_dir(&ids.token, &ids.version);
    Ok(read_receipt(&version_dir)?
        .map(|receipt| receipt.fonts)
        .unwrap_or_default())
}

fn remove_obsolete_fonts(
    cask: &Cask,
    previous_targets: &[PathBuf],
    current_targets: &[PathBuf],
) -> Result<()> {
    let token = SafePathComponent::parse("token", &cask.token)?;
    let token_dir = file::desymlink_path(&caskroom_token_dir(&token));
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

fn stage_binary(
    stage: &Path,
    caskroom: &Path,
    retained_caskroom: &Path,
    cask: &Cask,
    ids: &CaskIds,
    binary: &BinaryArtifact,
) -> Result<CompletedCaskAction> {
    let caskroom_binary = caskroom_binary_path(caskroom, binary)?;
    file::remove_all(&caskroom_binary)?;
    if let Some(parent) = caskroom_binary.parent() {
        file::create_dir_all(parent)?;
    }
    let (operation, source_path) = if binary.source.contains("$APPDIR") {
        // Expand + contain under /Applications or $HOMEBREW_PREFIX/Applications.
        // Never string-replace then trust is_file — `$APPDIR/../secret` must fail closed.
        let app_binary = resolve_appdir_binary_source(&binary.source)?;
        file::make_symlink(&app_binary, &caskroom_binary)?;
        (CaskActionOperation::Symlink, Some(app_binary))
    } else {
        let source = find_binary_source(stage, caskroom, ids, binary)?;
        if source.starts_with(stage) || source.starts_with(caskroom) {
            file::copy(&source, &caskroom_binary)?;
            file::make_executable(&caskroom_binary)?;
            (CaskActionOperation::Copy, Some(source))
        } else {
            file::make_symlink(&source, &caskroom_binary)?;
            (CaskActionOperation::Symlink, Some(source))
        }
    };
    let _ = cask; // token/version come from validated ids
    Ok(CompletedCaskAction {
        id: format!("binary-stage:{}", binary.target_name()?),
        kind: CaskActionKind::Binary,
        operation,
        source: source_path,
        target: Some(caskroom_binary_path(retained_caskroom, binary)?),
        phase: CaskActionPhase::Completed,
        mise_created: true,
        identifiers: Vec::new(),
    })
}

fn find_binary_source(
    stage: &Path,
    caskroom: &Path,
    ids: &CaskIds,
    binary: &BinaryArtifact,
) -> Result<PathBuf> {
    // Homebrew API often records preflight/postflight wrappers as
    // `$HOMEBREW_PREFIX/Caskroom/<token>/<version>/<name>`. Map that final
    // path onto:
    //   1) temp caskroom (postflight runs with staged_path = temp caskroom)
    //   2) extract stage (preflight runs with staged_path = extract stage; e.g. VLC)
    for root in [caskroom, stage] {
        if let Some(source) = generated_caskroom_artifact(root, ids, &binary.source)
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
    find_file_artifact(caskroom, &binary.source)
        .or_else(|| find_file_artifact(stage, &binary.source))
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

fn generated_caskroom_artifact(root: &Path, ids: &CaskIds, source: &str) -> Option<PathBuf> {
    let prefix = prefix::prefix();
    let source = source.replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy());
    let source = PathBuf::from(source);
    let final_caskroom = caskroom_version_dir(&ids.token, &ids.version);
    let relative = source.strip_prefix(final_caskroom).ok()?;
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return None;
    }
    Some(root.join(relative))
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

fn link_binary(caskroom: &Path, binary: &BinaryArtifact) -> Result<CompletedCaskAction> {
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
    Ok(CompletedCaskAction {
        id: format!("binary:{}", binary.target_name()?),
        kind: CaskActionKind::Binary,
        operation: CaskActionOperation::Symlink,
        source: Some(caskroom_binary),
        target: Some(target),
        phase: CaskActionPhase::Completed,
        mise_created: true,
        identifiers: Vec::new(),
    })
}

fn caskroom_binary_path(caskroom: &Path, binary: &BinaryArtifact) -> Result<PathBuf> {
    let target = binary.target_path()?;
    let roots = allowed_binary_target_roots();
    let relative = roots
        .iter()
        .find_map(|root| target.strip_prefix(root).ok())
        .ok_or_else(|| {
            eyre!(
                "brew-cask: binary target '{}' must be under {}",
                target.display(),
                allowed_binary_target_roots_display(&roots)
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
    // Directory predicate inside the walk so a same-named file cannot shadow.
    find_artifact_matching(root, name, |path| path.is_dir())
}

fn find_file_artifact(root: &Path, name: &str) -> Option<PathBuf> {
    find_artifact_matching(root, name, |path| path.is_file())
}

/// Exact path suffix match first, then ASCII case-insensitive suffix (e.g. cask
/// `yaak.app` vs DMG `Yaak.app`). `pred` runs only after a name hit.
fn find_artifact_matching(
    root: &Path,
    name: &str,
    pred: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let name_path = Path::new(name);
    let mut case_insensitive = None;
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "__MACOSX")
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        // Cheap path-string checks first; only `stat` via `pred` on name hits
        // (large .app trees have thousands of non-matching entries).
        if relative.ends_with(name_path) {
            if pred(path) {
                return Some(entry.into_path());
            }
        } else if case_insensitive.is_none()
            && path_ends_with_ignore_ascii_case(relative, name_path)
            && pred(path)
        {
            case_insensitive = Some(entry.into_path());
        }
    }
    case_insensitive
}

/// True when `path`'s trailing components match `suffix` with ASCII
/// case-insensitive comparison of normal path components.
fn path_ends_with_ignore_ascii_case(path: &Path, suffix: &Path) -> bool {
    if suffix.as_os_str().is_empty() {
        return false;
    }
    let mut path_iter = path.components().rev();
    for b in suffix.components().rev() {
        let Some(a) = path_iter.next() else {
            return false;
        };
        let matches = match (a, b) {
            (Component::Normal(a), Component::Normal(b)) => match (a.to_str(), b.to_str()) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                _ => a == b,
            },
            _ => a == b,
        };
        if !matches {
            return false;
        }
    }
    true
}

fn app_target_path(target_name: &str) -> Result<PathBuf> {
    if target_name.contains('\0') {
        bail!("brew-cask: app target contains NUL");
    }
    let system_apps = PathBuf::from("/Applications");
    let prefix_apps = prefix::prefix().join("Applications");
    if !target_name.contains('/') {
        // Bare bundle name under /Applications only.
        let name = SafePathComponent::parse("app target", target_name)?;
        return Ok(system_apps.join(name.as_str()));
    }
    let expanded = target_name.replace("$HOMEBREW_PREFIX", &prefix::prefix().to_string_lossy());
    let path = PathBuf::from(&expanded);
    if !path.is_absolute() {
        bail!("brew-cask: app target '{target_name}' must be an absolute path");
    }
    let normalized = normalize_absolute_components(&path)?;
    if path_contained_in_or_eq(&normalized, &system_apps)? && normalized != system_apps {
        return Ok(normalized);
    }
    if path_contained_in_or_eq(&normalized, &prefix_apps)? && normalized != prefix_apps {
        return Ok(normalized);
    }
    bail!(
        "brew-cask: app target '{target_name}' must be under /Applications or $HOMEBREW_PREFIX/Applications"
    );
}

fn app_bundle_name(target_name: &str) -> Result<&str> {
    Path::new(target_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("brew-cask: invalid app target '{target_name}'"))
}

/// Roots that a cask's `binary` artifact may legitimately symlink into.
///
/// The Homebrew prefix (`/opt/homebrew` on arm64, `/usr/local` on Intel) is
/// always allowed. `/usr/local` is additionally allowed even on arm64 because
/// some casks (e.g. docker-desktop) hardcode absolute `/usr/local/bin` targets
/// so their CLIs land on PATH regardless of architecture. Homebrew honors those
/// targets, so mise does too.
fn allowed_binary_target_roots() -> Vec<PathBuf> {
    let prefix = prefix::prefix();
    let mut roots = vec![prefix.clone()];
    let usr_local = PathBuf::from("/usr/local");
    if prefix != usr_local {
        roots.push(usr_local);
    }
    roots
}

fn allowed_binary_target_roots_display(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(" or ")
}

fn binary_target_path(target_name: &str) -> Result<PathBuf> {
    if target_name.contains('\0') {
        bail!("brew-cask: binary target contains NUL");
    }
    let prefix = prefix::prefix();
    let prefix_str = prefix.to_string_lossy();
    let target_name = target_name.replace("$HOMEBREW_PREFIX", prefix_str.as_ref());
    let path = PathBuf::from(&target_name);
    let target = if path.is_absolute() {
        normalize_absolute_components(&path)?
    } else if target_name.contains('/') {
        if path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!(
                "brew-cask: binary target '{}' must not contain '..' or root",
                target_name
            );
        }
        prefix.join(path)
    } else {
        let name = SafePathComponent::parse("binary target", &target_name)?;
        prefix.join("bin").join(name.as_str())
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
    let roots = allowed_binary_target_roots();
    let contained = roots
        .iter()
        .any(|root| path_contained_in_or_eq(&target, root).unwrap_or(false) && target != *root);
    if !contained {
        bail!(
            "brew-cask: binary target '{}' must be under {}",
            target.display(),
            allowed_binary_target_roots_display(&roots)
        );
    }
    Ok(target)
}

fn installed_version(token: &str) -> Option<String> {
    let token = SafePathComponent::parse("token", token).ok()?;
    let dir = caskroom_token_dir(&token);
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
            warn!(
                "brew-cask:{}: multiple Caskroom versions found; reinstall to reconcile",
                token.as_str()
            );
            None
        }
    }
}

fn homebrew_metadata_present(token: &SafePathComponent) -> bool {
    // Fail closed for any filesystem object at `.metadata`, including a
    // symlink or malformed tree. Mise never authors this path.
    caskroom_token_dir(token)
        .join(".metadata")
        .symlink_metadata()
        .is_ok()
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
    let ids = match CaskIds::validate(&cask.token, &version) {
        Ok(ids) => ids,
        Err(_) => return Ok(Vec::new()),
    };
    let version_dir = caskroom_version_dir(&ids.token, &ids.version);
    Ok(read_receipt(&version_dir)?
        .map(|receipt| receipt.binaries)
        .unwrap_or_default())
}

fn remove_obsolete_binary_links(
    cask: &Cask,
    previous_targets: &[PathBuf],
    current_targets: &[PathBuf],
) -> Result<()> {
    let token = SafePathComponent::parse("token", &cask.token)?;
    let token_dir = file::desymlink_path(&caskroom_token_dir(&token));
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
    let Ok(ids) = CaskIds::validate(&cask.token, &version) else {
        return Ok(None);
    };
    let version_dir = caskroom_version_dir(&ids.token, &ids.version);
    match read_receipt(&version_dir)? {
        Some(receipt) => {
            // V2 uses completed-action truth exclusively. Legacy receipts may use
            // only their recorded lists; empty legacy fields retain old status
            // behavior but can never become handoff eligible.
            let app_targets =
                if receipt.schema_version < CASK_RECEIPT_SCHEMA_V2 && receipt.apps.is_empty() {
                    artifacts
                        .apps
                        .iter()
                        .map(|app| app_target_path(app.target_name()))
                        .collect::<Result<Vec<_>>>()?
                } else {
                    receipt.apps.clone()
                };
            let binary_targets =
                if receipt.schema_version < CASK_RECEIPT_SCHEMA_V2 && receipt.binaries.is_empty() {
                    artifacts
                        .binaries
                        .iter()
                        .map(BinaryArtifact::target_path)
                        .collect::<Result<Vec<_>>>()?
                } else {
                    receipt.binaries.clone()
                };
            let pkg_ids: &[String] = if receipt.schema_version < CASK_RECEIPT_SCHEMA_V2 {
                if artifacts.pkgs.is_empty() {
                    &[]
                } else if receipt.pkg_ids.is_empty() {
                    &artifacts.pkg_ids
                } else {
                    &receipt.pkg_ids
                }
            } else {
                &receipt.pkg_ids
            };
            let pkgs_installed = pkg_ids.is_empty() || pkg_ids_installed(pkg_ids)?;
            let font_targets =
                if receipt.schema_version < CASK_RECEIPT_SCHEMA_V2 && receipt.fonts.is_empty() {
                    artifacts
                        .fonts
                        .iter()
                        .map(font_target_path)
                        .collect::<Result<Vec<_>>>()?
                } else {
                    receipt.fonts.clone()
                };
            if app_targets.iter().all(|app| app.exists())
                && binary_targets.iter().all(|binary| binary.exists())
                && pkgs_installed
                && font_targets.iter().all(|font| font.exists())
            {
                Ok(Some(receipt.version))
            } else {
                // Degraded/conflict: payload incomplete — not package absence for repair
                // from current API. Report missing so apply may reinstall if requested.
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

/// Publish final mise receipt only after activation. Actions come from mutators.
fn write_receipt(caskroom: &Path, completed: &CompletedCaskActionManifest) -> Result<()> {
    completed.validate_known()?;
    let activated_targets = |kind: CaskActionKind| {
        completed
            .actions
            .iter()
            .filter(|action| action.kind == kind && action.operation != CaskActionOperation::Stage)
            .filter_map(|action| action.target.clone())
            .collect::<Vec<_>>()
    };
    let receipt = CaskReceipt {
        version: completed.version.clone(),
        schema_version: CASK_RECEIPT_SCHEMA_V2,
        apps: activated_targets(CaskActionKind::App),
        binaries: activated_targets(CaskActionKind::Binary),
        fonts: activated_targets(CaskActionKind::Font),
        pkg_ids: completed
            .actions
            .iter()
            .filter(|action| action.kind == CaskActionKind::Pkg)
            .flat_map(|action| action.identifiers.iter().cloned())
            .collect(),
        transaction_id: Some(completed.transaction_id.clone()),
        actions: completed.actions.clone(),
    };
    let body = toml::to_string_pretty(&receipt)?;
    write_durable_file(&caskroom.join(".mise-cask.toml"), body.as_bytes())?;
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

fn caskroom_token_dir(token: &SafePathComponent) -> PathBuf {
    checked_join(&prefix::prefix().join("Caskroom"), token)
}

fn caskroom_version_dir(token: &SafePathComponent, version: &SafePathComponent) -> PathBuf {
    checked_join(&caskroom_token_dir(token), version)
}

fn caskroom_tmp_dir(ids: &CaskIds) -> PathBuf {
    let key = format!("{}-{}", ids.token.as_str(), ids.version.as_str());
    caskroom_token_dir(&ids.token).join(format!(".mise-tmp-{}", hash::hash_to_str(&key)))
}

fn remove_stale_versions(token_dir: &Path, current_version: &SafePathComponent) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(token_dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name();
        if entry.file_type().is_ok_and(|ft| ft.is_dir())
            && name.to_str() != Some(current_version.as_str())
            && name != ".metadata"
        {
            // Never delete `.metadata` — foreign Homebrew ledgers must survive.
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

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        // Recover from poison so one failed test does not cascade.
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
            tap: None,
            old_tokens: Vec::new(),
            aliases: Vec::new(),
            raw_base: None,
        }
    }

    fn test_request(name: &str) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: None,
            tap_url: None,
        }
    }

    fn test_ids(token: &str, version: &str) -> CaskIds {
        CaskIds::validate(token, version).expect("test token/version must be path-safe")
    }

    fn test_token(token: &str) -> SafePathComponent {
        SafePathComponent::parse("token", token).expect("test token must be path-safe")
    }

    fn test_version(version: &str) -> SafePathComponent {
        SafePathComponent::parse("version", version).expect("test version must be path-safe")
    }

    fn empty_completed(ids: &CaskIds) -> CompletedCaskActionManifest {
        CompletedCaskActionManifest::new(ids, "test-txn")
    }

    fn write_test_receipt(caskroom: &Path, cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
        let ids = test_ids(&cask.token, &cask.version);
        let mut completed = empty_completed(&ids);
        // Synthetic completed action so schema v2 receipts are handoff-truthful in tests.
        if !artifacts.binaries.is_empty()
            || !artifacts.apps.is_empty()
            || !artifacts.fonts.is_empty()
        {
            for binary in &artifacts.binaries {
                completed.actions.push(CompletedCaskAction {
                    id: format!("binary:{}", binary.target_name()?),
                    kind: CaskActionKind::Binary,
                    operation: CaskActionOperation::Symlink,
                    source: None,
                    target: Some(binary.target_path()?),
                    phase: CaskActionPhase::Completed,
                    mise_created: true,
                    identifiers: Vec::new(),
                });
            }
            for app in &artifacts.apps {
                completed.actions.push(CompletedCaskAction {
                    id: format!("app:{}", app.target_name()),
                    kind: CaskActionKind::App,
                    operation: CaskActionOperation::Copy,
                    source: None,
                    target: Some(app_target_path(app.target_name())?),
                    phase: CaskActionPhase::Completed,
                    mise_created: true,
                    identifiers: Vec::new(),
                });
            }
            for font in &artifacts.fonts {
                completed.actions.push(CompletedCaskAction {
                    id: format!("font:{}", font_filename(font)?),
                    kind: CaskActionKind::Font,
                    operation: CaskActionOperation::Copy,
                    source: None,
                    target: Some(font_target_path(font)?),
                    phase: CaskActionPhase::Completed,
                    mise_created: true,
                    identifiers: Vec::new(),
                });
            }
        }
        write_receipt(caskroom, &completed)
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
        // `Yaak.app`. Default macOS APFS is case-insensitive; exact match must
        // not be required.
        let tmp = tempfile::tempdir()?;
        let app = tmp.path().join("Yaak.app");
        file::create_dir_all(&app)?;

        assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app.clone()));
        assert_eq!(find_app(tmp.path(), "Yaak.app"), Some(app));
        assert_eq!(find_app(tmp.path(), "Other.app"), None);
        Ok(())
    }

    #[test]
    fn artifact_lookup_prefers_exact_case_over_earlier_fallback() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let fallback = tmp.path().join("Yaak.app");
        let exact = fallback.join("Contents/yaak.app");
        file::create_dir_all(&exact)?;

        assert_eq!(find_app(tmp.path(), "yaak.app"), Some(exact));
        Ok(())
    }

    #[test]
    fn artifact_lookup_skips_macos_metadata_for_case_insensitive_match() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        file::create_dir_all(tmp.path().join("__MACOSX/Yaak.app"))?;
        let app = tmp.path().join("Yaak.app");
        file::create_dir_all(&app)?;

        assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app));
        Ok(())
    }

    #[test]
    fn find_app_ignores_file_that_matches_app_name() -> Result<()> {
        // A same-named regular file must not shadow a later .app directory.
        let tmp = tempfile::tempdir()?;
        std::fs::write(tmp.path().join("yaak.app"), b"not a bundle")?;
        let app = tmp.path().join("nested/Yaak.app");
        file::create_dir_all(&app)?;

        assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app));
        Ok(())
    }

    #[test]
    fn path_ends_with_ignore_ascii_case_matches_components() {
        assert!(path_ends_with_ignore_ascii_case(
            Path::new("payload/Yaak.app"),
            Path::new("yaak.app")
        ));
        assert!(path_ends_with_ignore_ascii_case(
            Path::new("Yaak.app"),
            Path::new("yaak.app")
        ));
        assert!(!path_ends_with_ignore_ascii_case(
            Path::new("Yaak.app"),
            Path::new("Other.app")
        ));
        assert!(!path_ends_with_ignore_ascii_case(
            Path::new("Yaak.app"),
            Path::new("")
        ));
        assert!(!path_ends_with_ignore_ascii_case(
            Path::new("Yaak.app"),
            Path::new("/Yaak.app")
        ));
    }

    #[test]
    fn maps_preflight_generated_wrapper_from_extract_stage() -> Result<()> {
        // VLC: preflight writes `#{staged_path}/vlc.wrapper.sh` while preflight
        // staged_path is the extract stage, not the temp Caskroom. API binary
        // source is `$HOMEBREW_PREFIX/Caskroom/vlc/<ver>/vlc.wrapper.sh`.
        let _lock = env_lock();
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
            find_binary_source(
                &stage,
                &tmp_caskroom,
                &test_ids(&cask.token, &cask.version),
                &binary,
            )?,
            wrapper
        );
        Ok(())
    }

    #[test]
    fn prefers_temp_caskroom_wrapper_over_extract_stage() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("vlc", "3.0.23");
        let stage = tmp.path().join("extract");
        let tmp_caskroom = tmp.path().join("tmp-caskroom");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&tmp_caskroom)?;
        std::fs::write(stage.join("vlc.wrapper.sh"), "#!/bin/sh\necho stage\n")?;
        let preferred = tmp_caskroom.join("vlc.wrapper.sh");
        std::fs::write(&preferred, "#!/bin/sh\necho caskroom\n")?;

        let binary = BinaryArtifact {
            source: "$HOMEBREW_PREFIX/Caskroom/vlc/3.0.23/vlc.wrapper.sh".to_string(),
            target: Some("vlc".to_string()),
        };

        assert_eq!(
            find_binary_source(
                &stage,
                &tmp_caskroom,
                &test_ids(&cask.token, &cask.version),
                &binary,
            )?,
            preferred
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
        let _lock = env_lock();
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
            generated_caskroom_artifact(
                &tmp_caskroom,
                &test_ids(&cask.token, &cask.version),
                source,
            ),
            Some(generated)
        );
        Ok(())
    }

    #[test]
    fn rejects_generated_caskroom_binary_parent_dirs() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("gimp", "3.2.4");
        let tmp_caskroom = tmp.path().join("tmp-caskroom");
        let source = "$HOMEBREW_PREFIX/Caskroom/gimp/3.2.4/../escape";

        assert_eq!(
            generated_caskroom_artifact(
                &tmp_caskroom,
                &test_ids(&cask.token, &cask.version),
                source,
            ),
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
        let _lock = env_lock();
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
    fn binary_targets_must_stay_under_an_allowed_root() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        // Targets outside both the prefix and /usr/local are rejected.
        let err = binary_target_path("/opt/elsewhere/bin/op")
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be under"));
        let err = binary_target_path("../op").unwrap_err().to_string();
        assert!(err.contains("must not contain '..'"));
        Ok(())
    }

    #[test]
    fn binary_targets_allow_absolute_usr_local() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        // Casks like docker-desktop hardcode absolute /usr/local targets; these
        // are honored even when the prefix is elsewhere (arm64 /opt/homebrew).
        assert_eq!(
            binary_target_path("/usr/local/bin/docker")?,
            PathBuf::from("/usr/local/bin/docker")
        );
        assert_eq!(
            binary_target_path("/usr/local/cli-plugins/docker-compose")?,
            PathBuf::from("/usr/local/cli-plugins/docker-compose")
        );
        Ok(())
    }

    #[test]
    fn caskroom_binary_paths_preserve_prefix_relative_target() -> Result<()> {
        let _lock = env_lock();
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
    fn caskroom_binary_paths_strip_usr_local_root() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/docker-desktop/1.0.0");
        let binary = BinaryArtifact {
            source: "$APPDIR/Docker.app/Contents/Resources/bin/docker".to_string(),
            target: Some("/usr/local/bin/docker".to_string()),
        };

        assert_eq!(
            caskroom_binary_path(&caskroom, &binary)?,
            caskroom.join("bin/docker")
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_ignores_receipt_pkg_ids_for_app_only_casks() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("app-only", "1.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let caskroom = caskroom_version_dir(&test_token(&cask.token), &test_version(&cask.version));
        file::create_dir_all(&caskroom)?;
        file::create_dir_all(app_target_path(app.target_name())?)?;
        let receipt = CaskReceipt {
            version: cask.version.clone(),
            schema_version: 0,
            apps: vec![app_target_path(app.target_name())?],
            binaries: vec![],
            fonts: vec![],
            pkg_ids: vec!["com.example.helper".to_string()],
            transaction_id: None,
            actions: vec![],
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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("binary-only", "1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };
        file::create_dir_all(caskroom_version_dir(
            &test_token(&cask.token),
            &test_version(&cask.version),
        ))?;

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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        crate::file::write(stage.join("op"), "binary")?;
        let caskroom = caskroom_version_dir(&test_token("binary-only"), &test_version("1.0.0"));
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("binary-only", "1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };

        stage_binary(
            &stage,
            &caskroom,
            &caskroom,
            &cask,
            &test_ids(&cask.token, &cask.version),
            &binary,
        )?;
        link_binary(&caskroom, &binary)?;

        let target = binary.target_path()?;
        assert_eq!(std::fs::read_link(&target)?, caskroom.join("bin/op"));
        assert_eq!(crate::file::read_to_string(&target)?, "binary");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_same_basename_binaries_without_collision() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(stage.join("bin"))?;
        file::create_dir_all(stage.join("sbin"))?;
        crate::file::write(stage.join("bin/op"), "bin")?;
        crate::file::write(stage.join("sbin/op"), "sbin")?;
        let caskroom = caskroom_version_dir(&test_token("binary-only"), &test_version("1.0.0"));
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

        stage_binary(
            &stage,
            &caskroom,
            &caskroom,
            &cask,
            &test_ids(&cask.token, &cask.version),
            &bin,
        )?;
        stage_binary(
            &stage,
            &caskroom,
            &caskroom,
            &cask,
            &test_ids(&cask.token, &cask.version),
            &sbin,
        )?;
        link_binary(&caskroom, &bin)?;
        link_binary(&caskroom, &sbin)?;

        assert_eq!(crate::file::read_to_string(bin.target_path()?)?, "bin");
        assert_eq!(crate::file::read_to_string(sbin.target_path()?)?, "sbin");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn binary_source_prefers_hook_generated_caskroom_file() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        crate::file::write(stage.join("op"), "stage")?;
        let caskroom = caskroom_version_dir(&test_token("binary-only"), &test_version("1.0.0"));
        file::create_dir_all(&caskroom)?;
        crate::file::write(caskroom.join("op"), "hook")?;
        let cask = test_cask("binary-only", "1.0.0");
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };

        stage_binary(
            &stage,
            &caskroom,
            &caskroom,
            &cask,
            &test_ids(&cask.token, &cask.version),
            &binary,
        )?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("bin/op"))?,
            "hook"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_absolute_binary_source_from_pkg_install() -> Result<()> {
        let _lock = env_lock();
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
        let caskroom =
            caskroom_version_dir(&test_token("karabiner-elements"), &test_version("16.1.0"));
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("karabiner-elements", "16.1.0");
        let binary = BinaryArtifact {
            source: pkg_binary.to_string_lossy().to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/karabiner_cli".to_string()),
        };

        stage_binary(
            &stage,
            &caskroom,
            &caskroom,
            &cask,
            &test_ids(&cask.token, &cask.version),
            &binary,
        )?;
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
        let _lock = env_lock();
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
        let caskroom =
            caskroom_version_dir(&test_token("karabiner-elements"), &test_version("16.1.0"));
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("karabiner-elements", "16.1.0");
        let binary = BinaryArtifact {
            source: pkg_binary.to_string_lossy().to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/karabiner_cli".to_string()),
        };

        stage_binary(
            &stage,
            &caskroom,
            &caskroom,
            &cask,
            &test_ids(&cask.token, &cask.version),
            &binary,
        )?;
        file::remove_file(&pkg_binary)?;
        let err = link_binary(&caskroom, &binary).unwrap_err().to_string();

        assert!(err.contains("was staged but symlink target"));
        assert!(err.contains(&pkg_binary.to_string_lossy().to_string()));
        Ok(())
    }

    #[test]
    fn cask_appdir_uses_prefix_for_prefix_targeted_apps() -> Result<()> {
        let _lock = env_lock();
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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("binary-only", "2.0.0");
        let old_caskroom = caskroom_version_dir(&test_token(&cask.token), &test_version("1.0.0"));
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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("pkg-only", "1.0.0");
        let caskroom = caskroom_version_dir(&test_token(&cask.token), &test_version(&cask.version));
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            version: cask.version.clone(),
            schema_version: 0,
            apps: vec![],
            binaries: vec![],
            fonts: vec![],
            pkg_ids: vec![],
            transaction_id: None,
            actions: vec![],
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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("actual-token", "1.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        file::create_dir_all(caskroom_version_dir(
            &test_token(&cask.token),
            &test_version(&cask.version),
        ))?;

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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("metadata-token", "2.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        file::create_dir_all(caskroom_version_dir(
            &test_token("configured-name"),
            &test_version(&cask.version),
        ))?;
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

        file::create_dir_all(caskroom_version_dir(
            &test_token(&cask.token),
            &test_version(&cask.version),
        ))?;
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
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token_dir = caskroom_token_dir(&test_token("actual-token"));
        file::create_dir_all(token_dir.join("2.0.0"))?;
        file::create_dir_all(token_dir.join(".metadata/2.0.0/timestamp/Casks"))?;
        file::create_dir_all(token_dir.join(".mise-tmp-interrupted"))?;

        assert_eq!(installed_version("actual-token"), Some("2.0.0".to_string()));
        Ok(())
    }

    #[test]
    fn homebrew_metadata_blocks_mise_mutation_across_versions() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token = test_token("actual-token");
        let token_dir = caskroom_token_dir(&token);

        assert!(!homebrew_metadata_present(&token));
        file::create_dir_all(token_dir.join(".metadata/1.0.0/timestamp/Casks"))?;
        assert!(homebrew_metadata_present(&token));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn malformed_homebrew_metadata_symlink_fails_closed() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token = test_token("actual-token");
        let token_dir = caskroom_token_dir(&token);
        file::create_dir_all(&token_dir)?;
        std::os::unix::fs::symlink("missing", token_dir.join(".metadata"))?;

        assert!(homebrew_metadata_present(&token));
        Ok(())
    }

    #[test]
    fn remove_stale_versions_keeps_current_version_and_homebrew_metadata() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token_dir = caskroom_token_dir(&test_token("actual-token"));
        file::create_dir_all(token_dir.join("1.0.0"))?;
        file::create_dir_all(token_dir.join("2.0.0"))?;
        let metadata = token_dir.join(".metadata/2.0.0/timestamp/Casks");
        file::create_dir_all(&metadata)?;
        crate::file::write(metadata.join("actual-token.json"), "metadata")?;

        remove_stale_versions(&token_dir, &test_version("2.0.0"))?;

        assert!(!token_dir.join("1.0.0").exists());
        assert!(token_dir.join("2.0.0").exists());
        assert_eq!(
            crate::file::read_to_string(metadata.join("actual-token.json"))?,
            "metadata"
        );
        Ok(())
    }

    /// Plan 010: mise-owned pour writes `.mise-cask.toml`, never `.metadata`.
    #[test]
    fn mise_receipt_does_not_create_homebrew_metadata() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("codex", "0.145.0");
        let artifacts = CaskArtifacts {
            binaries: vec![BinaryArtifact {
                source: "codex-aarch64-apple-darwin".to_string(),
                target: Some("codex".to_string()),
            }],
            ..Default::default()
        };
        let version_dir =
            caskroom_version_dir(&test_token(&cask.token), &test_version(&cask.version));
        file::create_dir_all(&version_dir)?;
        write_test_receipt(&version_dir, &cask, &artifacts)?;

        assert!(version_dir.join(".mise-cask.toml").is_file());
        assert!(
            !caskroom_token_dir(&test_token(&cask.token))
                .join(".metadata")
                .exists()
        );
        let receipt = read_receipt(&version_dir)?.expect("receipt");
        assert_eq!(receipt.schema_version, CASK_RECEIPT_SCHEMA_V2);
        assert!(!receipt.actions.is_empty());
        assert!(receipt.handoff_eligible());
        Ok(())
    }

    /// Plan 010: healthy mise receipt must not trigger Homebrew metadata repair.
    #[test]
    fn already_installed_mise_cask_does_not_synthesize_metadata() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("codex", "0.145.0");
        let artifacts = CaskArtifacts {
            binaries: vec![BinaryArtifact {
                source: "codex".to_string(),
                target: Some("codex".to_string()),
            }],
            ..Default::default()
        };
        let version_dir =
            caskroom_version_dir(&test_token(&cask.token), &test_version(&cask.version));
        file::create_dir_all(&version_dir)?;
        // Stage binary target so installed_cask_version sees a healthy pour.
        let target = binary_target_path("codex")?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "bin")?;
        write_test_receipt(&version_dir, &cask, &artifacts)?;

        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some("0.145.0".to_string())
        );
        assert!(
            !caskroom_token_dir(&test_token(&cask.token))
                .join(".metadata")
                .exists()
        );
        Ok(())
    }

    /// Plan 010: foreign Homebrew `.metadata` is preserved byte-for-byte.
    #[test]
    fn foreign_homebrew_metadata_is_never_rewritten() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("codex", "0.145.0");
        let token_dir = caskroom_token_dir(&test_token(&cask.token));
        let version_dir =
            caskroom_version_dir(&test_token(&cask.token), &test_version(&cask.version));
        file::create_dir_all(&version_dir)?;
        let metadata = token_dir.join(".metadata/0.145.0/ts/Casks");
        file::create_dir_all(&metadata)?;
        let marker = metadata.join("codex.json");
        crate::file::write(&marker, "FOREIGN-HOMEBREW-BYTES")?;
        let tab = token_dir.join(".metadata/INSTALL_RECEIPT.json");
        crate::file::write(&tab, "brew-owned-tab")?;

        write_test_receipt(&version_dir, &cask, &CaskArtifacts::default())?;
        remove_stale_versions(&token_dir, &test_version(&cask.version))?;

        assert_eq!(
            crate::file::read_to_string(&marker)?,
            "FOREIGN-HOMEBREW-BYTES"
        );
        assert_eq!(crate::file::read_to_string(&tab)?, "brew-owned-tab");
        Ok(())
    }

    /// Plan 011: opaque token/version validation; no semver assumptions.
    #[test]
    fn safe_path_component_accepts_opaque_versions() -> Result<()> {
        for version in ["latest", "2026.07.23", "1.2,3", "preview-1", "0.145.0"] {
            let ids = CaskIds::validate("codex", version)?;
            assert_eq!(ids.version.as_str(), version);
        }
        for bad in ["", ".", "..", "a/b", "/abs", "x\\y"] {
            assert!(
                SafePathComponent::parse("token", bad).is_err(),
                "expected reject {bad}"
            );
        }
        Ok(())
    }

    /// Plan 011: app target containment is component-aware, not lexical prefix.
    #[test]
    fn app_target_path_rejects_traversal_and_prefix_lookalikes() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        assert!(app_target_path("Example.app")?.ends_with("Example.app"));
        assert_eq!(
            app_target_path("$HOMEBREW_PREFIX/Applications/Example.app")?,
            tmp.path().join("Applications/Example.app")
        );
        for bad in [
            "/Applications/../tmp/Evil.app",
            "$HOMEBREW_PREFIX/Applications/../../bin/evil",
            "/tmp/Evil.app",
            "../Evil.app",
        ] {
            let err = app_target_path(bad).unwrap_err().to_string();
            assert!(
                err.contains("must be under")
                    || err.contains("escapes")
                    || err.contains("invalid")
                    || err.contains("absolute"),
                "bad={bad} err={err}"
            );
        }
        // Prefix lookalike: /Applications-evil is not under /Applications
        let err = app_target_path("/Applications-evil/Foo.app")
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be under"), "{err}");
        Ok(())
    }

    /// Plan 011: invalid identifiers fail before any side effect on the temp prefix.
    #[test]
    fn invalid_token_fails_before_filesystem_mutation() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let before = WalkDir::new(tmp.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .count();
        assert!(CaskIds::validate("../evil", "1.0.0").is_err());
        assert!(CaskIds::validate("ok", "..").is_err());
        assert!(validate_relative_artifact_source("app source", "../etc/passwd").is_err());
        let after = WalkDir::new(tmp.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(before, after, "validation must not mutate prefix");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn caskroom_symlink_escape_is_rejected_before_mutation() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom");
        file::create_dir_all(&caskroom)?;
        std::os::unix::fs::symlink(outside.path(), caskroom.join("evil"))?;
        let ids = test_ids("evil", "1.0.0");

        let error = validate_mutation_boundaries(&ids, &CaskArtifacts::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(std::fs::read_dir(outside.path())?.count(), 0);
        Ok(())
    }

    /// Plan 011: exact token match; official API may use old_tokens/aliases.
    #[test]
    fn request_token_equality_accepts_canonical_and_trusted_aliases() -> Result<()> {
        let cask = test_cask("visual-studio-code", "1.0.0");
        ensure_cask_token_matches_request(&cask, &test_request("visual-studio-code"))?;
        ensure_cask_token_matches_request(
            &cask,
            &test_request("homebrew/cask/visual-studio-code"),
        )?;

        // Official homebrew/cask (and bare official API) may honor old_tokens/aliases.
        let mut aliased = test_cask("visual-studio-code", "1.0.0");
        aliased.old_tokens = vec!["vscode".into()];
        aliased.aliases = vec!["code-app".into()];
        ensure_cask_token_matches_request(&aliased, &test_request("vscode"))?;
        ensure_cask_token_matches_request(&aliased, &test_request("homebrew/cask/vscode"))?;
        ensure_cask_token_matches_request(&aliased, &test_request("code-app"))?;

        // Third-party tap: exact token only — self-declared aliases are ignored.
        assert!(
            ensure_cask_token_matches_request(&aliased, &test_request("evil-org/evil-tap/vscode"))
                .is_err(),
            "third-party must not accept API old_tokens/aliases"
        );
        ensure_cask_token_matches_request(
            &aliased,
            &test_request("evil-org/evil-tap/visual-studio-code"),
        )?;
        Ok(())
    }

    /// Plan 011: mismatched API identity fails closed — no path/FS use of wrong token.
    #[test]
    fn request_token_mismatch_fails_closed_without_fs_mutation() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let before = WalkDir::new(tmp.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .count();

        let hostile = test_cask("evil-payload", "9.9.9");
        let req = test_request("innocent");
        let err = ensure_cask_token_matches_request(&hostile, &req)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("does not match requested token") && err.contains("evil-payload"),
            "err={err}"
        );
        // Tap-qualified request still extracts token for equality.
        let err2 =
            ensure_cask_token_matches_request(&hostile, &test_request("homebrew/cask/innocent"))
                .unwrap_err()
                .to_string();
        assert!(err2.contains("innocent"), "err2={err2}");

        assert_eq!(requested_cask_token(&req), "innocent");
        assert!(!cask_token_matches_request(
            &hostile,
            "innocent",
            trust_homebrew_cask_api_aliases(&req)
        ));

        let after = WalkDir::new(tmp.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(before, after, "token mismatch must not mutate prefix");
        assert!(!tmp.path().join("Caskroom").exists());
        Ok(())
    }

    /// Hostile third-party body: token=evil, old_tokens=[innocent] must NOT accept.
    /// That was the wrong-identity pour hole (request innocent → Caskroom/evil-payload).
    #[test]
    fn hostile_tap_self_declared_old_tokens_fail_closed_without_fs_mutation() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let before = WalkDir::new(tmp.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .count();

        let mut hostile = test_cask("evil-payload", "9.9.9");
        hostile.old_tokens = vec!["innocent".into()];
        hostile.aliases = vec!["innocent".into()];

        // Third-party tap request (untrusted aliases).
        let third_party = test_request("evil-org/malware-tap/innocent");
        assert!(!trust_homebrew_cask_api_aliases(&third_party));
        let err = ensure_cask_token_matches_request(&hostile, &third_party)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("does not match") && err.contains("evil-payload"),
            "err={err}"
        );
        assert!(!cask_token_matches_request(
            &hostile, "innocent", /* trust_api_aliases */ false
        ));
        // install_one would use cask.token for paths only after ensure — ensure failed.
        assert_eq!(hostile.token, "evil-payload");
        assert!(!tmp.path().join("Caskroom/evil-payload").exists());
        assert!(!tmp.path().join("Caskroom/innocent").exists());

        let after = WalkDir::new(tmp.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(before, after, "hostile alias bypass must not mutate prefix");
        Ok(())
    }

    /// Plan 011 AC2: `$APPDIR` is not a free string replace — contain after expand.
    #[test]
    fn appdir_binary_source_rejects_traversal_before_io() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        for bad in [
            "$APPDIR/../secret",
            "$APPDIR/foo/../../etc/passwd",
            "$APPDIR/../tmp/evil",
            "prefix/$APPDIR/Foo.app/cli",
            "$APPDIR",
            "$APPDIR/",
        ] {
            let err = validate_appdir_binary_source(bad).unwrap_err().to_string();
            assert!(
                err.contains("$APPDIR") || err.contains("Applications") || err.contains("relative"),
                "bad={bad} err={err}"
            );
        }
        // Happy path: relative under Applications after expand (file need not exist for validate).
        validate_appdir_binary_source("$APPDIR/Foo.app/Contents/MacOS/cli")?;
        // validate_artifact_paths must fail closed on escape sources.
        let artifacts = CaskArtifacts {
            binaries: vec![BinaryArtifact {
                source: "$APPDIR/../secret".into(),
                target: Some("evil".into()),
            }],
            ..Default::default()
        };
        assert!(validate_artifact_paths(&artifacts).is_err());
        Ok(())
    }

    /// Even if a file exists at the escaped path, stage_binary must not symlink it.
    #[cfg(unix)]
    #[test]
    fn stage_binary_appdir_escape_does_not_mutate_caskroom() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        // Bait file outside Applications that naive `$APPDIR` + `../` would hit.
        let secret = tmp.path().join("secret-bin");
        crate::file::write(&secret, "pwn")?;
        // Also create under Applications so a wrong policy might still find something.
        let apps = tmp.path().join("Applications");
        file::create_dir_all(&apps)?;

        let cask = test_cask("evil-cask", "1.0.0");
        let ids = test_ids(&cask.token, &cask.version);
        let caskroom = caskroom_version_dir(&ids.token, &ids.version);
        file::create_dir_all(&caskroom)?;
        let binary = BinaryArtifact {
            source: "$APPDIR/../secret-bin".into(),
            target: Some("$HOMEBREW_PREFIX/bin/evil".into()),
        };

        let err = stage_binary(&stage, &caskroom, &caskroom, &cask, &ids, &binary)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("$APPDIR") || err.contains("Applications") || err.contains("relative"),
            "err={err}"
        );
        // No caskroom/bin mutation for the escape attempt.
        assert!(!caskroom.join("bin/evil").exists());
        assert!(!tmp.path().join("bin/evil").exists());
        Ok(())
    }

    /// Plan 013: mutators emit completed actions; journal outside Caskroom.
    #[test]
    fn completed_action_journal_lives_outside_caskroom_metadata() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let ids = test_ids("codex", "0.145.0");
        let mut completed = CompletedCaskActionManifest::new(&ids, "txn-journal-1");
        completed.actions.push(CompletedCaskAction {
            id: "binary:codex".into(),
            kind: CaskActionKind::Binary,
            operation: CaskActionOperation::Symlink,
            source: None,
            target: Some(tmp.path().join("bin/codex")),
            phase: CaskActionPhase::Completed,
            mise_created: true,
            identifiers: Vec::new(),
        });
        write_action_journal(&ids, &completed)?;
        let journal = action_journal_path(&ids, "txn-journal-1")?;
        assert!(journal.is_file());
        assert!(journal.starts_with(cask_recovery_root()));
        assert!(
            !journal
                .components()
                .any(|c| c.as_os_str() == "Caskroom" || c.as_os_str() == ".metadata")
        );
        // Round-trip
        let body: CompletedCaskActionManifest =
            serde_json::from_str(&crate::file::read_to_string(&journal)?)?;
        assert_eq!(body.actions.len(), 1);
        body.validate_known()?;
        clear_action_journal(&ids, "txn-journal-1")?;
        assert!(!journal.exists());
        Ok(())
    }

    /// Plan 013: unknown manifest version fails closed.
    #[test]
    fn completed_action_manifest_rejects_unknown_version() {
        let mut m = CompletedCaskActionManifest::new(&test_ids("t", "1"), "txn");
        m.manifest_version = 99;
        assert!(m.validate_known().is_err());
    }

    /// Plan 013: legacy receipts are LegacyUnverified for handoff.
    #[test]
    fn legacy_receipt_is_not_handoff_eligible() -> Result<()> {
        let receipt = CaskReceipt {
            version: "1.0.0".into(),
            schema_version: 0,
            apps: vec![],
            binaries: vec![],
            fonts: vec![],
            pkg_ids: vec![],
            transaction_id: None,
            actions: vec![],
        };
        assert!(receipt.is_legacy_unverified());
        assert!(!receipt.handoff_eligible());
        // Same-version "API changed" cannot auto-upgrade eligibility.
        let mut upgraded = receipt.clone();
        upgraded.schema_version = CASK_RECEIPT_SCHEMA_V2;
        // Still no actions → still not handoff-eligible.
        assert!(!upgraded.handoff_eligible());
        Ok(())
    }

    /// Plan 013: a changed same-version API cannot redefine v2 installed truth.
    #[test]
    fn completed_receipt_status_ignores_changed_api_intent() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("opaque", "same-version");
        let ids = test_ids(&cask.token, &cask.version);
        let version_dir = caskroom_version_dir(&ids.token, &ids.version);
        file::create_dir_all(&version_dir)?;
        let installed_target = tmp.path().join("bin/original");
        file::create_dir_all(installed_target.parent().unwrap())?;
        crate::file::write(&installed_target, "installed")?;
        let mut completed = CompletedCaskActionManifest::new(&ids, "txn-original");
        completed.actions.push(CompletedCaskAction {
            id: "binary:original".into(),
            kind: CaskActionKind::Binary,
            operation: CaskActionOperation::Symlink,
            source: Some(version_dir.join("bin/original")),
            target: Some(installed_target),
            phase: CaskActionPhase::Completed,
            mise_created: true,
            identifiers: Vec::new(),
        });
        write_receipt(&version_dir, &completed)?;

        let changed_api = CaskArtifacts {
            binaries: vec![BinaryArtifact {
                source: "replacement".into(),
                target: Some("replacement".into()),
            }],
            ..Default::default()
        };
        assert_eq!(
            installed_cask_version(&cask, &changed_api)?,
            Some("same-version".into())
        );
        let receipt = read_receipt(&version_dir)?.expect("v2 receipt");
        assert_eq!(receipt.binaries.len(), 1);
        assert!(!receipt.binaries[0].ends_with("replacement"));
        Ok(())
    }

    /// Plan 013: crash before final receipt leaves no healthy receipt (journal only).
    #[test]
    fn incomplete_transaction_has_journal_not_final_receipt() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let ids = test_ids("codex", "0.145.0");
        let version_dir = caskroom_version_dir(&ids.token, &ids.version);
        file::create_dir_all(&version_dir)?;
        let completed = CompletedCaskActionManifest::new(&ids, "txn-pending");
        write_action_journal(&ids, &completed)?;
        // Simulate crash: version dir exists, no final receipt.
        assert!(read_receipt(&version_dir)?.is_none());
        assert!(action_journal_path(&ids, "txn-pending")?.is_file());
        Ok(())
    }

    /// Plan 013: stage+link mutators return completed-action facts (not artifact intent).
    #[cfg(unix)]
    #[test]
    fn stage_and_link_binary_emit_completed_actions() -> Result<()> {
        let _lock = env_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        crate::file::write(stage.join("op"), "binary")?;
        let cask = test_cask("binary-only", "1.0.0");
        let ids = test_ids(&cask.token, &cask.version);
        let caskroom = caskroom_version_dir(&ids.token, &ids.version);
        file::create_dir_all(&caskroom)?;
        let binary = BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
        };
        let staged = stage_binary(&stage, &caskroom, &caskroom, &cask, &ids, &binary)?;
        assert_eq!(staged.kind, CaskActionKind::Binary);
        assert!(staged.mise_created);
        let linked = link_binary(&caskroom, &binary)?;
        assert_eq!(linked.operation, CaskActionOperation::Symlink);
        assert_eq!(linked.target, Some(binary.target_path()?));
        Ok(())
    }
}
