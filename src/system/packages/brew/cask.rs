use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

use async_trait::async_trait;
use eyre::{WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::api::RubySourceChecksum;
use super::prefix;
use super::source;
use crate::cmd::CmdLineRunner;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::git::{CloneOptions, Git};
use crate::hash;
use crate::http::{HTTP, HTTP_FETCH};
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

#[derive(Debug, Clone, Default, Deserialize)]
struct CaskUrlSpecs {
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    only_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Cask {
    token: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    old_tokens: Vec<String>,
    version: String,
    url: String,
    #[serde(default)]
    url_specs: CaskUrlSpecs,
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
struct CommandWrapperArtifact {
    name: String,
    target: Option<String>,
    content: Option<String>,
    executable: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionShell {
    Bash,
    Fish,
    Zsh,
    Pwsh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionArtifact {
    shell: CompletionShell,
    source: String,
    target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedCompletionArtifact {
    executable: String,
    args: Vec<String>,
    base_name: Option<String>,
    shell_parameter_format: Option<String>,
    shells: Vec<CompletionShell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FlightStep {
    Move {
        source: FlightPath,
        target: FlightPath,
        source_glob: bool,
    },
    Remove {
        paths: Vec<FlightPath>,
        recursive: bool,
    },
    Run {
        command: FlightPath,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        sudo: bool,
        guards: Vec<FlightGuard>,
    },
    TerminateProcess {
        name: String,
        match_mode: ProcessMatch,
        sudo: bool,
        attempts: usize,
        must_succeed: bool,
        notices: Vec<String>,
        failure_message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessMatch {
    Name,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlightPathBase {
    StagedPath,
    AppDir,
    HomebrewPrefix,
    Absolute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlightPath {
    base: FlightPathBase,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FlightGuard {
    OnMacos,
    OnLinux,
    IfExists(FlightPath),
    UnlessExists(FlightPath),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CaskArtifacts {
    apps: Vec<AppArtifact>,
    binaries: Vec<BinaryArtifact>,
    command_wrappers: Vec<CommandWrapperArtifact>,
    pkgs: Vec<PkgArtifact>,
    fonts: Vec<FontArtifact>,
    completions: Vec<CompletionArtifact>,
    generated_completions: Vec<GeneratedCompletionArtifact>,
    preflight_steps: Vec<FlightStep>,
    postflight_steps: Vec<FlightStep>,
    pkg_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct CaskReceipt {
    #[serde(default)]
    schema_version: u8,
    version: String,
    #[serde(default)]
    apps: Vec<PathBuf>,
    #[serde(default)]
    binaries: Vec<PathBuf>,
    #[serde(default)]
    fonts: Vec<PathBuf>,
    #[serde(default)]
    completions: Vec<PathBuf>,
    #[serde(default)]
    pkg_ids: Vec<String>,
    #[serde(default)]
    targets: Vec<CaskTargetRecord>,
    #[serde(default)]
    prune_safe: bool,
    #[serde(default)]
    prune_blocker: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct CaskTargetRecord {
    path: PathBuf,
    fingerprint: CaskTargetFingerprint,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct CaskTargetFingerprint {
    kind: CaskTargetKind,
    digest: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CaskTargetKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Serialize)]
struct CaskTransactionJournal<'a> {
    schema_version: u8,
    token: &'a str,
    version: &'a str,
    completed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CaskPruneCandidate {
    pub token: String,
    pub version: String,
    version_dir: PathBuf,
    receipt: CaskReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaskPruneSkip {
    pub token: String,
    pub reason: String,
}

#[derive(Debug, Default, Clone)]
pub struct CaskPrunePlan {
    pub remove: Vec<CaskPruneCandidate>,
    pub skipped: Vec<CaskPruneSkip>,
}

impl CaskPrunePlan {
    pub fn is_empty(&self) -> bool {
        self.remove.is_empty()
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
        let artifacts = cask_artifacts(&cask)?;
        validate_platform_support(&cask, &artifacts)?;
        if homebrew_metadata_present(&cask.token) {
            bail!(
                "brew-cask:{}: Homebrew owns this cask; remove it with Homebrew before installing it with mise",
                cask.token
            );
        }
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
            for wrapper in &artifacts.command_wrappers {
                miseprintln!("link command wrapper {}", wrapper.target_name()?);
            }
            for pkg in &artifacts.pkgs {
                miseprintln!("install pkg {}", pkg.source);
            }
            for font in &artifacts.fonts {
                miseprintln!("install font {}", font.source);
            }
            for completion in &artifacts.completions {
                miseprintln!(
                    "install {} completion {}",
                    completion.shell.name(),
                    completion.source
                );
            }
            for generated in &artifacts.generated_completions {
                miseprintln!("generate completions from {}", generated.executable);
            }
            return Ok(cask.version);
        }
        prefix::bootstrap(false)?;
        let previous_binaries = previous_binary_targets(&cask)?;
        let previous_fonts = previous_font_targets(&cask)?;
        let previous_completions = previous_completion_targets(&cask)?;
        let stage = fetch_and_stage(&cask, pr).await?;
        let caskroom_token = caskroom_token_dir(&cask.token);
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let tmp_caskroom = caskroom_tmp_dir(&cask);
        file::remove_all(&tmp_caskroom)?;
        file::create_dir_all(&tmp_caskroom)?;
        let appdir = cask_appdir(&artifacts.apps)?;
        let mut journal = CaskTransactionJournal {
            schema_version: 1,
            token: &cask.token,
            version: &cask.version,
            completed: Vec::new(),
        };
        write_cask_journal(&journal)?;
        let current_completions = completion_target_paths(&cask, &artifacts)?;
        for target in &current_completions {
            ensure_completion_target_replaceable(&cask, &artifacts, target)?;
        }
        // Match Homebrew's artifact phases: preflight runs before app installation.
        // An appdir-based preflight command therefore sees only a previously installed app.
        execute_flight_steps_recording(
            &cask,
            &artifacts.preflight_steps,
            &stage,
            &appdir,
            "preflight_steps",
            &mut journal,
        )?;
        execute_lifecycle_hook(&cask, &stage, &appdir, "preflight", pr).await?;
        if has_lifecycle_hook(&cask, "preflight") {
            record_cask_action(&mut journal, "preflight_hook")?;
        }
        for (index, app) in artifacts.apps.iter().enumerate() {
            install_app(&stage, &tmp_caskroom, app)?;
            record_cask_action(&mut journal, &format!("app[{index}]"))?;
        }
        for (index, pkg) in artifacts.pkgs.iter().enumerate() {
            install_pkg(&stage, pkg)?;
            record_cask_action(&mut journal, &format!("pkg[{index}]"))?;
        }
        for (index, font) in artifacts.fonts.iter().enumerate() {
            stage_font(&stage, &tmp_caskroom, font)?;
            record_cask_action(&mut journal, &format!("font[{index}]"))?;
        }
        for (index, wrapper) in artifacts.command_wrappers.iter().enumerate() {
            stage_command_wrapper(&tmp_caskroom, &appdir, &cask, wrapper)?;
            record_cask_action(&mut journal, &format!("command_wrapper[{index}]"))?;
        }
        execute_flight_steps_recording(
            &cask,
            &artifacts.postflight_steps,
            &tmp_caskroom,
            &appdir,
            "postflight_steps",
            &mut journal,
        )?;
        execute_lifecycle_hook(&cask, &tmp_caskroom, &appdir, "postflight", pr).await?;
        if has_lifecycle_hook(&cask, "postflight") {
            record_cask_action(&mut journal, "postflight_hook")?;
        }
        for (index, binary) in artifacts.binaries.iter().enumerate() {
            stage_binary(&stage, &tmp_caskroom, &cask, &artifacts.apps, binary)?;
            record_cask_action(&mut journal, &format!("binary[{index}]"))?;
        }
        for (index, completion) in artifacts.completions.iter().enumerate() {
            stage_completion(&stage, &tmp_caskroom, &cask, &artifacts.apps, completion)?;
            record_cask_action(&mut journal, &format!("completion[{index}]"))?;
        }
        for (index, generated) in artifacts.generated_completions.iter().enumerate() {
            stage_generated_completions(&stage, &tmp_caskroom, &cask, &artifacts.apps, generated)?;
            record_cask_action(&mut journal, &format!("generated_completion[{index}]"))?;
        }
        let current_binaries = binary_targets(&artifacts)?;
        let current_fonts = font_target_paths(&artifacts)?;
        let mut current_targets = current_binaries.clone();
        current_targets.extend(current_completions.iter().cloned());
        current_targets.extend(current_fonts.iter().cloned());
        let mut link_transaction = ArtifactLinkTransaction::begin(current_targets)?;
        let activation = replace_caskroom(&cask, &tmp_caskroom, &caskroom, || {
            for binary in &artifacts.binaries {
                link_binary(&caskroom, &appdir, binary)?;
            }
            for wrapper in &artifacts.command_wrappers {
                link_command_wrapper(&caskroom, wrapper)?;
            }
            for target in &current_completions {
                link_completion(&cask, &artifacts, &caskroom, target)?;
            }
            for font in &artifacts.fonts {
                link_font(&caskroom, font)?;
            }
            write_receipt(&caskroom, &cask, &artifacts)?;
            Ok(())
        });
        if let Err(err) = activation {
            if let Err(rollback_err) = link_transaction.rollback() {
                return Err(err.wrap_err(format!(
                    "failed to restore external cask artifacts: {rollback_err:#}"
                )));
            }
            return Err(err);
        }
        link_transaction.commit()?;
        record_cask_action(&mut journal, "activated")?;
        remove_obsolete_binary_links(&cask, &previous_binaries, &current_binaries)?;
        remove_obsolete_completions(&cask, &previous_completions, &current_completions)?;
        remove_obsolete_fonts(&cask, &previous_fonts, &current_fonts)?;
        remove_stale_versions(&caskroom_token, &cask.version)?;
        remove_cask_journals(&cask.token)?;
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

    fn target_path(&self, appdir: &Path) -> Result<PathBuf> {
        binary_target_path(&self.target_name()?, appdir)
    }
}

impl CommandWrapperArtifact {
    fn target_name(&self) -> Result<String> {
        match &self.target {
            Some(target) => Ok(target.clone()),
            None => Ok(self.name.clone()),
        }
    }

    fn target_path(&self) -> Result<PathBuf> {
        binary_target_path(&self.target_name()?, Path::new("/Applications"))
    }

    fn caskroom_path(&self, caskroom: &Path) -> PathBuf {
        caskroom.join(".homebrew-command-wrappers").join(&self.name)
    }
}

impl CompletionShell {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            "zsh" => Some(Self::Zsh),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Fish => "fish",
            Self::Zsh => "zsh",
            Self::Pwsh => "pwsh",
        }
    }

    fn parameter_name(self) -> &'static str {
        match self {
            Self::Pwsh => "powershell",
            _ => self.name(),
        }
    }
}

impl CompletionArtifact {
    fn target_name(&self) -> Result<String> {
        match &self.target {
            Some(target) => Ok(target.clone()),
            None => Path::new(&self.source)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .ok_or_else(|| eyre!("brew-cask: invalid completion source '{}'", self.source)),
        }
    }

    fn target_path(&self) -> Result<PathBuf> {
        completion_target_path(self.shell, &self.target_name()?)
    }
}

impl GeneratedCompletionArtifact {
    fn resolved_base_name(&self, cask: &Cask) -> String {
        let name = self.base_name.clone().unwrap_or_else(|| {
            Path::new(&self.executable)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&cask.token)
                .to_string()
        });
        if name.is_empty() {
            cask.token.clone()
        } else {
            name
        }
    }

    fn target_paths(&self, cask: &Cask) -> Result<Vec<PathBuf>> {
        let base_name = self.resolved_base_name(cask);
        self.shells
            .iter()
            .map(|shell| generated_completion_target_path(*shell, &base_name))
            .collect()
    }
}

#[async_trait(?Send)]
impl SystemPackageManager for BrewCaskManager {
    fn name(&self) -> &str {
        "brew-cask"
    }

    fn is_available(&self) -> bool {
        cfg!(any(target_os = "macos", target_os = "linux"))
    }

    fn unavailable_reason(&self) -> String {
        "only available on macos and linux".to_string()
    }

    fn supports_version_pins(&self) -> bool {
        false
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let mut statuses = Vec::with_capacity(pkgs.len());
        for req in pkgs {
            let cask = fetch_cask(req).await?;
            let artifacts = cask_artifacts(&cask)?;
            validate_platform_support(&cask, &artifacts)?;
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
    let (requested_token, official_api) = match split_tap_name(name) {
        Some(("homebrew", "cask", token)) => (token, true),
        Some((_, _, token)) => (token, false),
        None => (name.as_str(), true),
    };
    validate_cask_path_component("requested token", requested_token)?;
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
    validate_cask_identity(&cask, requested_token, official_api)?;
    Ok(cask)
}

fn validate_cask_identity(cask: &Cask, requested_token: &str, official_api: bool) -> Result<()> {
    validate_cask_path_component("API token", &cask.token)?;
    validate_cask_path_component("version", &cask.version)?;
    let trusted_alias = official_api
        && cask
            .aliases
            .iter()
            .chain(&cask.old_tokens)
            .any(|alias| alias == requested_token);
    if cask.token != requested_token && !trusted_alias {
        bail!(
            "brew-cask: requested token '{requested_token}' does not match API token '{}'",
            cask.token
        );
    }
    Ok(())
}

fn validate_cask_path_component(kind: &str, value: &str) -> Result<()> {
    let mut components = Path::new(value).components();
    let valid = !value.is_empty()
        && !value.contains('\0')
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && value != ".metadata"
        && !value.starts_with(".mise-");
    if !valid {
        bail!("brew-cask: invalid {kind} '{value}'");
    }
    Ok(())
}

async fn fetch_and_stage(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    if cask.url.ends_with(".git") {
        return fetch_git_clone_and_stage(cask, pr).await;
    }
    let archive = fetch_archive(cask, pr).await?;
    extract_archive(cask, &archive, pr)
}

async fn fetch_git_clone_and_stage(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let extract_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-extract")
        .join(format!("{}-{}", cask.token, cask.version));
    file::remove_all(&extract_dir)?;
    file::create_dir_all(&extract_dir)?;
    let clone_dir = crate::dirs::CACHE
        .join("system-brew")
        .join("cask-git-clone")
        .join(format!("{}-{}", cask.token, cask.version));
    file::remove_all(&clone_dir)?;
    let mut clone_opts = CloneOptions::default();
    if let Some(branch) = cask.url_specs.branch.as_deref() {
        clone_opts = clone_opts.branch(branch);
    }
    if let Some(pr) = pr {
        clone_opts = clone_opts.pr(pr);
    }
    Git::new(&clone_dir)
        .clone(&cask.url, clone_opts)
        .wrap_err_with(|| format!("brew-cask:{}: failed to clone {}", cask.token, cask.url))?;
    if let Some(only_path) = &cask.url_specs.only_path {
        let source = clone_dir.join(only_path);
        if source.is_dir() {
            for entry in std::fs::read_dir(&source)? {
                let entry = entry?;
                let dest = extract_dir.join(entry.file_name());
                file::rename(entry.path(), &dest)?;
            }
        }
    } else {
        for entry in std::fs::read_dir(&clone_dir)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let dest = extract_dir.join(entry.file_name());
            file::rename(entry.path(), &dest)?;
        }
    }
    file::remove_all(&clone_dir)?;
    Ok(extract_dir)
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
        HTTP.download_file(&cask.url, &archive, pr).await?;
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
    if is_dmg_archive(archive, filename)? {
        file::un_dmg(archive, &extract_dir)?;
    } else {
        let format = cask_extraction_format(archive, filename)?;
        if format == ExtractionFormat::Raw {
            // Raw executable binary — copy it using the original URL filename so
            // find_file_artifact can match against the binary stanza source name (e.g. "claude").
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
        ("MISE_BREW_CASK_SUDO", sudo::subprocess_mode().to_string()),
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

fn is_dmg_archive(archive: &Path, filename: &str) -> Result<bool> {
    if filename.ends_with(".dmg") {
        return Ok(true);
    }
    if ExtractionFormat::from_file_name(filename) != ExtractionFormat::Raw {
        return Ok(false);
    }

    // UDIF images end with a 512-byte resource footer containing this prefix.
    const UDIF_TRAILER_SIZE: i64 = 512;
    const UDIF_TRAILER_PREFIX: &[u8; 12] = b"koly\0\0\0\x04\0\0\x02\0";
    let mut file = std::fs::File::open(archive)?;
    if file.metadata()?.len() < UDIF_TRAILER_SIZE as u64 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-UDIF_TRAILER_SIZE))?;
    let mut prefix = [0; UDIF_TRAILER_PREFIX.len()];
    file.read_exact(&mut prefix)?;
    Ok(&prefix == UDIF_TRAILER_PREFIX)
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
    swap_app(&target, &tmp_target)?;
    // Remove macOS quarantine attribute so Gatekeeper doesn't block the app.
    let _ = std::process::Command::new("xattr")
        .args(["-r", "-d", "com.apple.quarantine"])
        .arg(&target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

/// Atomically replace an app, restoring the previous bundle if activation fails.
fn swap_app(target: &Path, tmp_target: &Path) -> Result<()> {
    // Atomic swap: rename existing target aside before putting the new one in place so that
    // a failure during rename leaves the old app intact rather than leaving nothing.
    let old_target = target.with_extension(format!(
        "mise-old-{}",
        crate::hash::hash_to_str(&target.display().to_string())
    ));
    remove_app(&old_target)?;
    if target.exists() {
        file::rename(target, &old_target)?;
    }
    if let Err(e) = file::rename(tmp_target, target) {
        // Restore the old app if the swap failed.
        if old_target.exists() {
            let _ = file::rename(&old_target, target);
        }
        return Err(e);
    }
    // The replacement is already live. A cleanup failure must not report the
    // install as failed or prevent install_app from removing quarantine.
    if let Err(err) = remove_app(&old_target) {
        warn!(
            "brew-cask: failed to remove old app backup {}: {err:#}",
            old_target.display()
        );
    }
    Ok(())
}

/// Remove an app bundle, repairing protected contents before escalating ownership.
fn remove_app(path: &Path) -> Result<()> {
    match file::remove_all(path) {
        Ok(()) => return Ok(()),
        Err(err) if !is_permission_denied(&err) => return Err(err),
        Err(_) => {}
    }

    repair_app_permissions(path);
    match file::remove_all(path) {
        Ok(()) => return Ok(()),
        Err(err) if !is_permission_denied(&err) => return Err(err),
        Err(_) => {}
    }

    let user = nix::unistd::User::from_uid(nix::unistd::geteuid())?
        .map(|user| user.name)
        .ok_or_else(|| eyre!("brew-cask: could not determine current user"))?;
    // Match Homebrew's final ownership-recovery step. sudo::run applies the
    // system_packages.sudo setting and refuses to prompt without a TTY.
    sudo::run(
        "chown",
        &[
            "-R".to_string(),
            "--".to_string(),
            user,
            path.display().to_string(),
        ],
        &[],
    )?;
    repair_app_permissions(path);
    file::remove_all(path)
}

/// Clear flags, restore owner permissions, and remove ACLs from an app bundle.
fn repair_app_permissions(path: &Path) {
    let run = |program: &str, args: &[&str]| {
        let _ = std::process::Command::new(program)
            .args(args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    run("/usr/bin/chflags", &["-R", "--", "000"]);
    run("/bin/chmod", &["-R", "--", "u+rwx"]);
    run("/bin/chmod", &["-R", "-N"]);
}

/// Return whether an eyre chain originated from an I/O permission error.
fn is_permission_denied(err: &eyre::Report) -> bool {
    err.downcast_ref::<std::io::Error>()
        .is_some_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied)
}

/// Retry a failed artifact-target mutation with sudo on permission errors.
///
/// Cask artifact targets can live in root-owned directories — most commonly
/// `/usr/local/bin` on Apple Silicon, where casks like docker-desktop
/// hardcode absolute binary targets. Homebrew elevates its cask
/// `ln`/`mkdir`/`rm`/`mv` calls when the target directory is not writable;
/// this matches that behavior. sudo::run honors `system_packages.sudo` and
/// never prompts for a password without a TTY.
fn with_sudo_fallback(result: Result<()>, program: &str, args: &[String]) -> Result<()> {
    match result {
        Err(err) if is_permission_denied(&err) => sudo::run(program, args, &[]),
        other => other,
    }
}

fn create_dir_all_elevating(dir: &Path) -> Result<()> {
    with_sudo_fallback(
        file::create_dir_all(dir),
        "mkdir",
        &["-p".into(), "--".into(), dir.display().to_string()],
    )
}

fn make_symlink_elevating(source: &Path, link: &Path) -> Result<()> {
    with_sudo_fallback(
        file::make_symlink(source, link).map(|_| ()),
        "ln",
        &[
            "-s".into(),
            "-f".into(),
            "-h".into(),
            "--".into(),
            source.display().to_string(),
            link.display().to_string(),
        ],
    )
}

fn rename_elevating(from: &Path, to: &Path) -> Result<()> {
    with_sudo_fallback(
        file::rename(from, to),
        "mv",
        &[
            "-f".into(),
            "--".into(),
            from.display().to_string(),
            to.display().to_string(),
        ],
    )
}

fn remove_artifact_target_elevating(path: &Path) -> Result<()> {
    let Ok(metadata) = path.symlink_metadata() else {
        return Ok(());
    };
    let (result, args) = if metadata.file_type().is_symlink() {
        (
            file::remove_file(path),
            vec!["-f".into(), "--".into(), path.display().to_string()],
        )
    } else {
        (
            file::remove_all(path),
            vec![
                "-r".into(),
                "-f".into(),
                "--".into(),
                path.display().to_string(),
            ],
        )
    };
    with_sudo_fallback(result, "rm", &args)
}

/// Copy a cask artifact while preserving macOS metadata where it matters.
///
/// Linux cask support currently covers font files, which do not need the
/// resource-fork and extended-attribute handling provided by `ditto`.
fn copy_cask_artifact(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        ditto(from, to)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file::copy(from, to)
    }
}

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
    let source = find_file_artifact(stage, &pkg.source)
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
    let source = find_file_artifact(stage, &font.source)
        .ok_or_else(|| eyre!("brew-cask: font artifact '{}' was not found", font.source))?;
    copy_cask_artifact(&source, &caskroom_font)?;
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
    if let Err(e) = copy_cask_artifact(&caskroom_font, &target) {
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
                let macos_fonts_dir = crate::dirs::HOME.join("Library").join("Fonts");
                for fonts_dir in [font_dir(), macos_fonts_dir] {
                    if let Ok(relative) = expanded_path.strip_prefix(fonts_dir) {
                        return Ok(relative.to_string_lossy().to_string());
                    }
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
        // resides under the platform font directory and the caskroom still has a
        // staged copy (from the previous version directory).
        let fonts_dir = font_dir();
        if !target.starts_with(&fonts_dir) {
            continue;
        }
        // Check if any version directory under the token dir contains this font
        // path, indicating it was staged by a previous version of this cask.
        let relative = target.strip_prefix(&fonts_dir).ok();
        let has_staged_copy = std::fs::read_dir(&token_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
            .any(|entry| relative.is_some_and(|path| entry.path().join(path).is_file()));
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
    Ok(font_dir().join(name_path))
}

fn font_dir() -> PathBuf {
    if cfg!(target_os = "linux") {
        crate::env::XDG_DATA_HOME.join("fonts")
    } else {
        crate::dirs::HOME.join("Library").join("Fonts")
    }
}

#[cfg(test)]
fn execute_flight_steps(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
) -> Result<()> {
    execute_flight_steps_with_completion(cask, steps, staged_path, appdir, kind, |_, _| Ok(()))
}

fn execute_flight_steps_recording(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    journal: &mut CaskTransactionJournal<'_>,
) -> Result<()> {
    execute_flight_steps_with_completion(cask, steps, staged_path, appdir, kind, |index, step| {
        record_cask_action(journal, &format!("{kind}[{index}]:{}", step.kind()))
    })
}

fn execute_flight_steps_with_completion(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    mut completed: impl FnMut(usize, &FlightStep) -> Result<()>,
) -> Result<()> {
    for (index, step) in steps.iter().enumerate() {
        execute_flight_step(cask, step, staged_path, appdir).wrap_err_with(|| {
            format!("brew-cask:{}: failed to run structured {kind}", cask.token)
        })?;
        completed(index, step)?;
    }
    Ok(())
}

impl FlightStep {
    fn kind(&self) -> &'static str {
        match self {
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::Run { .. } => "run",
            Self::TerminateProcess { .. } => "terminate_process",
        }
    }
}

fn execute_flight_step(
    cask: &Cask,
    step: &FlightStep,
    staged_path: &Path,
    appdir: &Path,
) -> Result<()> {
    match step {
        FlightStep::Move {
            source,
            target,
            source_glob,
        } => {
            let sources = flight_sources(staged_path, source, *source_glob)?;
            let target = resolve_flight_path(staged_path, target)?;
            if sources.len() > 1 && !target.is_dir() {
                bail!(
                    "brew-cask: structured move with multiple sources requires a directory target"
                );
            }
            for source in sources {
                let target = if target.is_dir() {
                    target.join(source.file_name().ok_or_else(|| {
                        eyre!(
                            "brew-cask: structured move source '{}' has no file name",
                            source.display()
                        )
                    })?)
                } else {
                    target.clone()
                };
                if let Some(parent) = target.parent()
                    && !parent.as_os_str().is_empty()
                {
                    file::create_dir_all(parent)?;
                }
                file::remove_all(&target)?;
                file::rename(&source, &target)?;
            }
        }
        FlightStep::Remove { paths, recursive } => {
            for path in paths {
                for path in flight_paths(staged_path, path)? {
                    if *recursive {
                        file::remove_all(&path)?;
                    } else if path.symlink_metadata().is_ok() {
                        file::remove_file_or_dir(&path)?;
                    }
                }
            }
        }
        FlightStep::Run {
            command,
            args,
            env,
            sudo,
            guards,
        } => {
            if !guards
                .iter()
                .map(|guard| flight_guard_matches(guard, staged_path, appdir))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .all(|matches| matches)
            {
                return Ok(());
            }
            let command = resolve_flight_path_with_context(command, staged_path, appdir)?;
            let command = expand_cask_template(
                &command.to_string_lossy(),
                staged_path,
                appdir,
                Some(&cask.version),
            );
            let args = args
                .iter()
                .map(|arg| expand_cask_template(arg, staged_path, appdir, Some(&cask.version)))
                .collect::<Vec<_>>();
            let env = env
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        expand_cask_template(value, staged_path, appdir, Some(&cask.version)),
                    )
                })
                .collect::<Vec<_>>();
            if *sudo {
                sudo::run(&command, &args, &env)?;
            } else {
                let mut runner = CmdLineRunner::new(&command);
                for arg in &args {
                    runner = runner.arg(arg);
                }
                for (key, value) in &env {
                    runner = runner.env(key, value);
                }
                runner.raw(true).execute()?;
            }
        }
        FlightStep::TerminateProcess { .. } => {
            execute_terminate_process(
                step,
                staged_path,
                appdir,
                &cask.version,
                |command, args, sudo| {
                    if sudo {
                        sudo::run(&command.to_string_lossy(), args, &[])
                    } else {
                        let mut runner = CmdLineRunner::new(command);
                        for arg in args {
                            runner = runner.arg(arg);
                        }
                        runner.raw(true).execute()
                    }
                },
                std::thread::sleep,
            )?;
        }
    }
    Ok(())
}

fn execute_terminate_process(
    step: &FlightStep,
    staged_path: &Path,
    appdir: &Path,
    version: &str,
    mut run: impl FnMut(&Path, &[String], bool) -> Result<()>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<()> {
    let FlightStep::TerminateProcess {
        name,
        match_mode,
        sudo,
        attempts,
        must_succeed,
        notices,
        failure_message,
    } = step
    else {
        bail!("brew-cask: internal non-terminate flight step");
    };
    let expand = |value: &str| expand_cask_template(value, staged_path, appdir, Some(version));
    for notice in notices {
        miseprintln!("{}", expand(notice));
    }
    let name = expand(name);
    let (command, args) = match *match_mode {
        ProcessMatch::Name => (Path::new("/usr/bin/killall"), vec![name]),
        ProcessMatch::Full => (Path::new("/usr/bin/pkill"), vec!["-f".to_string(), name]),
    };
    let mut last_error = None;
    for attempt in 0..*attempts {
        match run(command, &args, *sudo) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
        if attempt + 1 < *attempts {
            sleep(std::time::Duration::from_secs(1));
        }
    }
    if let Some(message) = failure_message.as_deref() {
        warn!("{}", expand(message));
    }
    if *must_succeed {
        return Err(last_error.unwrap_or_else(|| eyre!("failed to terminate process")));
    }
    Ok(())
}

fn flight_guard_matches(guard: &FlightGuard, staged_path: &Path, appdir: &Path) -> Result<bool> {
    match guard {
        FlightGuard::OnMacos => Ok(cfg!(target_os = "macos")),
        FlightGuard::OnLinux => Ok(cfg!(target_os = "linux")),
        FlightGuard::IfExists(path) => {
            Ok(resolve_flight_path_with_context(path, staged_path, appdir)?.exists())
        }
        FlightGuard::UnlessExists(path) => {
            Ok(!resolve_flight_path_with_context(path, staged_path, appdir)?.exists())
        }
    }
}

fn flight_sources(
    staged_path: &Path,
    source: &FlightPath,
    source_glob: bool,
) -> Result<Vec<PathBuf>> {
    if !source_glob {
        let source = resolve_flight_path(staged_path, source)?;
        if !source.exists() {
            bail!(
                "brew-cask: structured move source '{}' was not found",
                source.display()
            );
        }
        return Ok(vec![source]);
    }
    // Homebrew marks move sources as globs explicitly; non-glob move sources
    // may contain literal glob-like characters and should be resolved literally.
    let sources = expand_staged_glob(staged_path, &source.path)?;
    if sources.is_empty() {
        bail!(
            "brew-cask: structured move source '{}' was not found",
            source.path
        );
    }
    Ok(sources)
}

fn flight_paths(staged_path: &Path, path: &FlightPath) -> Result<Vec<PathBuf>> {
    if !is_flight_glob(&path.path) {
        return Ok(vec![resolve_flight_path(staged_path, path)?]);
    }
    // Remove steps do not have a `source_glob` flag, so path globs are detected
    // from the path syntax instead.
    expand_staged_glob(staged_path, &path.path)
}

fn expand_staged_glob(staged_path: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();
    let escaped_root = glob::Pattern::escape(staged_path.to_string_lossy().as_ref());
    for pattern in expand_braces(pattern) {
        validate_flight_relative_path(&pattern)?;
        let rooted_pattern = Path::new(&escaped_root)
            .join(Path::new(&pattern))
            .to_string_lossy()
            .to_string();
        for path in glob::glob_with(
            &rooted_pattern,
            glob::MatchOptions {
                require_literal_separator: true,
                ..Default::default()
            },
        )
        .wrap_err_with(|| format!("brew-cask: invalid structured flight glob '{pattern}'"))?
        {
            let path = path?;
            if !path.starts_with(staged_path) {
                bail!(
                    "brew-cask: structured flight glob '{}' matched outside staged path",
                    pattern
                );
            }
            matches.push(path);
        }
    }
    matches.sort();
    matches.dedup();
    Ok(matches)
}

fn is_flight_glob(path: &str) -> bool {
    path.chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
}

fn resolve_flight_path(staged_path: &Path, path: &FlightPath) -> Result<PathBuf> {
    match path.base {
        FlightPathBase::StagedPath => {}
        _ => bail!("brew-cask: structured file operation must use staged_path"),
    }
    let relative = Path::new(&path.path);
    validate_flight_relative_path(&path.path)?;
    Ok(staged_path.join(relative))
}

fn resolve_flight_path_with_context(
    path: &FlightPath,
    staged_path: &Path,
    appdir: &Path,
) -> Result<PathBuf> {
    let expanded = expand_cask_template(&path.path, staged_path, appdir, None);
    match path.base {
        FlightPathBase::StagedPath => Ok(staged_path.join(expanded)),
        FlightPathBase::AppDir => Ok(appdir.join(expanded)),
        FlightPathBase::HomebrewPrefix => Ok(prefix::prefix().join(expanded)),
        FlightPathBase::Absolute => Ok(PathBuf::from(expanded)),
    }
}

fn expand_cask_template(
    value: &str,
    staged_path: &Path,
    appdir: &Path,
    version: Option<&str>,
) -> String {
    let prefix = prefix::prefix();
    let mut value = value
        .replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy())
        .replace("$APPDIR", &appdir.to_string_lossy())
        .replace("$HOME", &crate::dirs::HOME.to_string_lossy())
        .replace("{{HOMEBREW_PREFIX}}", &prefix.to_string_lossy())
        .replace("{{staged_path}}", &staged_path.to_string_lossy())
        .replace("{{appdir}}", &appdir.to_string_lossy());
    if let Some(version) = version {
        value = value.replace("{{version}}", version);
    }
    if let Some(rest) = value.strip_prefix("~/") {
        value = crate::dirs::HOME.join(rest).to_string_lossy().to_string();
    }
    value
}

fn validate_flight_relative_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: invalid structured flight path '{}'",
            path.display()
        );
    }
    Ok(())
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(end_offset) = pattern[start + 1..].find('}') else {
        return vec![pattern.to_string()];
    };
    let end = start + 1 + end_offset;
    let prefix = &pattern[..start];
    let suffix = &pattern[end + 1..];
    let mut expanded = Vec::new();
    for alternative in pattern[start + 1..end].split(',') {
        for suffix in expand_braces(suffix) {
            expanded.push(format!("{prefix}{alternative}{suffix}"));
        }
    }
    expanded
}

fn stage_completion(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    completion: &CompletionArtifact,
) -> Result<()> {
    let target = completion.target_path()?;
    let caskroom_completion = caskroom_completion_path(caskroom, &target)?;
    let source = find_completion_source(stage, caskroom, cask, apps, &completion.source)?
        .ok_or_else(|| {
            eyre!(
                "brew-cask: {} completion artifact '{}' was not found",
                completion.shell.name(),
                completion.source
            )
        })?;
    if !file::same_file(&source, &caskroom_completion) {
        file::remove_all(&caskroom_completion)?;
        if let Some(parent) = caskroom_completion.parent() {
            file::create_dir_all(parent)?;
        }
        file::copy(&source, &caskroom_completion)?;
    }
    Ok(())
}

fn stage_generated_completions(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    completion: &GeneratedCompletionArtifact,
) -> Result<()> {
    let executable = find_generated_completion_executable(stage, caskroom, cask, apps, completion)?;
    if executable.starts_with(stage) || executable.starts_with(caskroom) {
        file::make_executable(&executable)?;
    }
    let base_name = completion.resolved_base_name(cask);
    for shell in &completion.shells {
        let target = generated_completion_target_path(*shell, &base_name)?;
        let caskroom_completion = caskroom_completion_path(caskroom, &target)?;
        if let Some(parent) = caskroom_completion.parent() {
            file::create_dir_all(parent)?;
        }
        let output = generate_completion_output(&executable, completion, *shell)?;
        crate::file::write(caskroom_completion, output)?;
    }
    Ok(())
}

fn link_completion(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    caskroom: &Path,
    target: &Path,
) -> Result<()> {
    let caskroom_completion = caskroom_completion_path(caskroom, target)?;
    if !caskroom_completion.is_file() {
        bail!(
            "brew-cask: completion artifact '{}' was not staged",
            target.display()
        );
    }
    if let Some(parent) = target.parent() {
        create_dir_all_elevating(parent)?;
    }
    ensure_completion_target_replaceable(cask, artifacts, target)?;
    make_symlink_elevating(&caskroom_completion, target)?;
    Ok(())
}

fn ensure_completion_target_replaceable(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    target: &Path,
) -> Result<()> {
    let Ok(metadata) = target.symlink_metadata() else {
        return Ok(());
    };
    if !metadata.file_type().is_symlink() {
        bail!(
            "brew-cask: completion target '{}' already exists and is not owned by cask '{}'",
            target.display(),
            cask.token
        );
    }
    let link_target = std::fs::read_link(target)?;
    let resolved = resolve_symlink_target(target, link_target);
    let token_dir = caskroom_token_dir(&cask.token);
    if path_starts_with_resolved_root(&resolved, &token_dir) {
        return Ok(());
    }
    for completion in &artifacts.completions {
        if completion.target_path()? != target {
            continue;
        }
        if let Some(source) = appdir_artifact_source(&completion.source, &artifacts.apps)?
            && file::same_file(&resolved, &source)
        {
            return Ok(());
        }
    }
    bail!(
        "brew-cask: completion target '{}' already points to '{}' and is not owned by cask '{}'",
        target.display(),
        resolved.display(),
        cask.token
    )
}

fn find_completion_source(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    source: &str,
) -> Result<Option<PathBuf>> {
    for root in [caskroom, stage] {
        if let Some(source) = generated_caskroom_artifact(root, cask, source)
            && source.is_file()
        {
            return Ok(Some(source));
        }
    }
    if let Some(source) = appdir_artifact_source(source, apps)? {
        return Ok(Some(source));
    }
    Ok(absolute_prefixed_source(source)
        .filter(|source| source.is_file())
        .or_else(|| find_file_artifact(caskroom, source))
        .or_else(|| find_file_artifact(stage, source)))
}

fn find_generated_completion_executable(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    completion: &GeneratedCompletionArtifact,
) -> Result<PathBuf> {
    let executable = &completion.executable;
    if let Some(source) = generated_caskroom_artifact(caskroom, cask, executable)
        && source.is_file()
    {
        return Ok(source);
    }
    if let Some(source) = generated_caskroom_artifact(stage, cask, executable)
        && source.is_file()
    {
        return Ok(source);
    }
    if let Some(source) = appdir_artifact_source(executable, apps)? {
        return Ok(source);
    }
    if let Some(source) = absolute_prefixed_source(executable) {
        if let Ok(relative) = source.strip_prefix(prefix::prefix()) {
            let caskroom_source = caskroom.join(relative);
            if caskroom_source.is_file() {
                return Ok(caskroom_source);
            }
        }
        if source.is_file() {
            return Ok(source);
        }
    }
    if let Some(source) = find_generated_completion_file(caskroom, executable)? {
        return Ok(source);
    }
    if let Some(source) = find_generated_completion_file(stage, executable)? {
        return Ok(source);
    }
    Err(eyre!(
        "brew-cask: completion executable '{}' was not found",
        executable
    ))
}

fn appdir_artifact_source(source: &str, apps: &[AppArtifact]) -> Result<Option<PathBuf>> {
    let Some(relative) = source.strip_prefix("$APPDIR/") else {
        return Ok(None);
    };
    let relative = Path::new(relative);
    if relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("brew-cask: APPDIR artifact '{source}' must stay below Applications");
    }
    let Some(Component::Normal(bundle)) = relative.components().next() else {
        return Ok(None);
    };
    let suffix = relative.components().skip(1).collect::<PathBuf>();
    let mut matches = Vec::new();
    for app in apps {
        let target = app_target_path(app.target_name())?;
        let bundle = Path::new(bundle);
        if !path_ends_with_ignore_ascii_case(Path::new(&app.source), bundle)
            && !path_ends_with_ignore_ascii_case(&target, bundle)
        {
            continue;
        }
        let path = target.join(&suffix);
        if path.is_file() {
            matches.push(path);
        }
    }
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => bail!(
            "brew-cask: APPDIR artifact '{}' is ambiguous: {}",
            source,
            matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn find_generated_completion_file(root: &Path, executable: &str) -> Result<Option<PathBuf>> {
    let executable_path = Path::new(executable);
    let direct = root.join(executable_path);
    if direct.is_file() {
        return Ok(Some(direct));
    }
    let matches = find_file_artifacts(root, executable_path);
    match matches.as_slice() {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => bail!(
            "brew-cask: completion executable '{}' is ambiguous: {}",
            executable,
            matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn find_file_artifacts(root: &Path, name: &Path) -> Vec<PathBuf> {
    let mut matches = WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "__MACOSX")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.strip_prefix(root)
                .is_ok_and(|relative| relative.ends_with(name))
                && path.is_file()
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    matches
}

fn generate_completion_output(
    executable: &Path,
    completion: &GeneratedCompletionArtifact,
    shell: CompletionShell,
) -> Result<String> {
    let mut command = std::process::Command::new(executable);
    command.args(&completion.args);
    command.env("SHELL", shell.name());
    let (shell_args, shell_env) = completion_shell_parameter(
        completion.shell_parameter_format.as_deref(),
        shell,
        executable,
    );
    command.args(shell_args);
    for (key, value) in shell_env {
        command.env(key, value);
    }
    let output = command.output().wrap_err_with(|| {
        format!(
            "failed to generate {} completions from {}",
            shell.name(),
            executable.display()
        )
    })?;
    if !output.status.success() {
        bail!(
            "brew-cask: failed to generate {} completions from {}: {}",
            shell.name(),
            executable.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn completion_shell_parameter(
    format: Option<&str>,
    shell: CompletionShell,
    executable: &Path,
) -> (Vec<String>, Vec<(String, String)>) {
    let shell_parameter = shell.parameter_name().to_string();
    match format {
        None => (vec![shell_parameter], Vec::new()),
        Some("arg") => (vec![format!("--shell={shell_parameter}")], Vec::new()),
        Some("clap") => (Vec::new(), vec![("COMPLETE".to_string(), shell_parameter)]),
        Some("click") => {
            let program = executable
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_uppercase()
                .replace('-', "_");
            (
                Vec::new(),
                vec![(
                    format!("_{program}_COMPLETE"),
                    format!("{shell_parameter}_source"),
                )],
            )
        }
        Some("cobra") => (vec!["completion".to_string(), shell_parameter], Vec::new()),
        Some("flag") => (vec![format!("--{shell_parameter}")], Vec::new()),
        Some("none") => (Vec::new(), Vec::new()),
        Some("typer") => (
            vec!["--show-completion".to_string(), shell_parameter],
            vec![(
                "_TYPER_COMPLETE_TEST_DISABLE_SHELL_DETECTION".to_string(),
                "1".to_string(),
            )],
        ),
        Some(format) => (vec![format!("{format}{shell_parameter}")], Vec::new()),
    }
}

fn absolute_prefixed_source(source: &str) -> Option<PathBuf> {
    let prefix = prefix::prefix();
    let source = source.replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy());
    let source = PathBuf::from(source);
    source.is_absolute().then_some(source)
}

fn completion_target_path(shell: CompletionShell, target_name: &str) -> Result<PathBuf> {
    let prefix = prefix::prefix();
    let prefix_str = prefix.to_string_lossy();
    let target_name = target_name.replace("$HOMEBREW_PREFIX", prefix_str.as_ref());
    let path = PathBuf::from(&target_name);
    let target = if path.is_absolute() {
        path
    } else if target_name.contains('/') {
        prefix.join(path)
    } else {
        default_completion_dir(shell).join(completion_filename(shell, &target_name)?)
    };
    if target
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: completion target '{}' must not contain '..'",
            target.display()
        );
    }
    if !target.starts_with(&prefix) {
        bail!(
            "brew-cask: completion target '{}' must be under {}",
            target.display(),
            prefix.display()
        );
    }
    Ok(target)
}

fn generated_completion_target_path(shell: CompletionShell, base_name: &str) -> Result<PathBuf> {
    match shell {
        CompletionShell::Pwsh => {
            let name = format!("_{}.ps1", base_name);
            completion_target_path(shell, &name)
        }
        _ => completion_target_path(shell, base_name),
    }
}

fn default_completion_dir(shell: CompletionShell) -> PathBuf {
    let prefix = prefix::prefix();
    match shell {
        CompletionShell::Bash => prefix.join("etc/bash_completion.d"),
        CompletionShell::Fish => prefix.join("share/fish/vendor_completions.d"),
        CompletionShell::Zsh => prefix.join("share/zsh/site-functions"),
        CompletionShell::Pwsh => prefix.join("share/pwsh/completions"),
    }
}

fn completion_filename(shell: CompletionShell, target_name: &str) -> Result<String> {
    let filename = Path::new(target_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("brew-cask: invalid completion target '{target_name}'"))?;
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(filename);
    let normalized = match shell {
        CompletionShell::Bash => stem.to_string(),
        CompletionShell::Fish => {
            if filename.ends_with(".fish") {
                filename.to_string()
            } else {
                format!("{stem}.fish")
            }
        }
        CompletionShell::Zsh => {
            if filename.starts_with('_') {
                filename.to_string()
            } else {
                format!("_{stem}")
            }
        }
        CompletionShell::Pwsh => {
            if filename.ends_with(".ps1") {
                filename.to_string()
            } else {
                format!("{stem}.ps1")
            }
        }
    };
    if normalized.is_empty() {
        bail!("brew-cask: invalid completion target '{target_name}'");
    }
    Ok(normalized)
}

fn caskroom_completion_path(caskroom: &Path, target: &Path) -> Result<PathBuf> {
    let prefix = prefix::prefix();
    let relative = target.strip_prefix(&prefix).map_err(|_| {
        eyre!(
            "brew-cask: completion target '{}' must be under {}",
            target.display(),
            prefix.display()
        )
    })?;
    if relative.components().next().is_none() {
        bail!(
            "brew-cask: invalid completion target '{}'",
            target.display()
        );
    }
    Ok(caskroom.join(relative))
}

fn completion_target_paths(cask: &Cask, artifacts: &CaskArtifacts) -> Result<Vec<PathBuf>> {
    let mut targets = artifacts
        .completions
        .iter()
        .map(CompletionArtifact::target_path)
        .collect::<Result<Vec<_>>>()?;
    for generated in &artifacts.generated_completions {
        targets.extend(generated.target_paths(cask)?);
    }
    targets.sort();
    targets.dedup();
    Ok(targets)
}

fn previous_completion_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    let Some(version) = installed_version(&cask.token) else {
        return Ok(Vec::new());
    };
    let version_dir = caskroom_version_dir(&cask.token, &version);
    Ok(read_receipt(&version_dir)?
        .map(|receipt| receipt.completions)
        .unwrap_or_default())
}

fn remove_obsolete_completions(
    cask: &Cask,
    previous_targets: &[PathBuf],
    current_targets: &[PathBuf],
) -> Result<()> {
    let token_dir = caskroom_token_dir(&cask.token);
    let prefix = prefix::prefix();
    for target in previous_targets {
        if current_targets.contains(target) || !target.starts_with(&prefix) {
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
        let resolved = resolve_symlink_target(target, link_target);
        if path_starts_with_resolved_root(&resolved, &token_dir) {
            file::remove_file(target)?;
        }
    }
    Ok(())
}

fn stage_binary(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    binary: &BinaryArtifact,
) -> Result<()> {
    let appdir = cask_appdir(apps)?;
    let caskroom_binary = caskroom_binary_path(caskroom, &appdir, binary)?;
    file::remove_all(&caskroom_binary)?;
    if let Some(parent) = caskroom_binary.parent() {
        file::create_dir_all(parent)?;
    }
    if binary.source.contains("$APPDIR") {
        // $APPDIR is the Applications directory where install_app placed the bundle.
        // Symlink into the installed app so the CLI wrapper can trace back to find the app.
        let app_binary = appdir_artifact_source(&binary.source, apps)?.ok_or_else(|| {
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

fn stage_command_wrapper(
    caskroom: &Path,
    appdir: &Path,
    cask: &Cask,
    wrapper: &CommandWrapperArtifact,
) -> Result<()> {
    let target = wrapper.caskroom_path(caskroom);
    file::remove_all(&target)?;
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    let content = match (&wrapper.content, &wrapper.executable) {
        (Some(content), None) => expand_command_wrapper_content(content, appdir),
        (None, Some(executable)) => {
            let executable = expand_command_wrapper_value(executable, appdir, cask);
            let args = wrapper
                .args
                .iter()
                .map(|arg| expand_command_wrapper_value(arg, appdir, cask))
                .map(|arg| shell_escape::unix::escape(Cow::Owned(arg)).into_owned())
                .collect::<Vec<_>>();
            let env = wrapper
                .env
                .iter()
                .map(|(key, value)| {
                    let value = expand_command_wrapper_value(value, appdir, cask);
                    Ok(format!(
                        "{key}={}",
                        shell_escape::unix::escape(Cow::Owned(value))
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut command = Vec::new();
            command.extend(env);
            command.push("exec".to_string());
            command.push(shell_escape::unix::escape(Cow::Owned(executable)).into_owned());
            command.extend(args);
            command.push("\"$@\"".to_string());
            format!("#!/bin/bash\n{}\n", command.join(" "))
        }
        _ => bail!(
            "brew-cask: command_wrapper '{}' must set exactly one of content or executable",
            wrapper.name
        ),
    };
    file::write(&target, content)?;
    file::make_executable(&target)?;
    Ok(())
}

fn expand_command_wrapper_content(value: &str, appdir: &Path) -> String {
    value
        .replace("$HOMEBREW_PREFIX", &prefix::prefix().to_string_lossy())
        .replace("$APPDIR", &appdir.to_string_lossy())
}

fn expand_command_wrapper_value(value: &str, appdir: &Path, cask: &Cask) -> String {
    let staged_path = caskroom_version_dir(&cask.token, &cask.version);
    expand_cask_template(value, &staged_path, appdir, Some(&cask.version))
}

fn is_shell_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
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
    //   1) temp caskroom (postflight runs with staged_path = temp caskroom)
    //   2) extract stage (preflight runs with staged_path = extract stage; e.g. VLC)
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

fn generated_caskroom_artifact(root: &Path, cask: &Cask, source: &str) -> Option<PathBuf> {
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
    Some(root.join(relative))
}

fn resolve_symlink_target(link: &Path, target: PathBuf) -> PathBuf {
    if target.is_absolute() {
        target
    } else {
        link.parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target)
    }
}

fn path_starts_with_resolved_root(path: &Path, root: &Path) -> bool {
    path_with_resolved_existing_ancestor(path).starts_with(file::desymlink_path(root))
}

fn path_with_resolved_existing_ancestor(path: &Path) -> PathBuf {
    let mut base = path;
    let mut suffix = PathBuf::new();
    loop {
        if base.symlink_metadata().is_ok() {
            return file::desymlink_path(base).join(suffix);
        }
        let Some(name) = base.file_name() else {
            return path.to_path_buf();
        };
        suffix = Path::new(name).join(suffix);
        let Some(parent) = base.parent() else {
            return path.to_path_buf();
        };
        base = parent;
    }
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

fn link_binary(caskroom: &Path, appdir: &Path, binary: &BinaryArtifact) -> Result<()> {
    let caskroom_binary = caskroom_binary_path(caskroom, appdir, binary)?;
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
    let target = binary.target_path(appdir)?;
    if let Some(parent) = target.parent() {
        create_dir_all_elevating(parent)?;
    }
    make_symlink_elevating(&caskroom_binary, &target)?;
    Ok(())
}

fn link_command_wrapper(caskroom: &Path, wrapper: &CommandWrapperArtifact) -> Result<()> {
    let source = wrapper.caskroom_path(caskroom);
    if !source.is_file() {
        bail!(
            "brew-cask: command wrapper '{}' was not staged",
            wrapper.name
        );
    }
    let target = wrapper.target_path()?;
    if let Some(parent) = target.parent() {
        create_dir_all_elevating(parent)?;
    }
    make_symlink_elevating(&source, &target)?;
    Ok(())
}

fn caskroom_binary_path(
    caskroom: &Path,
    appdir: &Path,
    binary: &BinaryArtifact,
) -> Result<PathBuf> {
    let target = binary.target_path(appdir)?;
    let roots = if is_appdir_binary_target(&binary.target_name()?) {
        let mut roots = allowed_appdir_roots();
        roots.extend(allowed_binary_target_roots());
        roots
    } else {
        allowed_binary_target_roots()
    };
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
        if let Some(steps) = parse_flight_steps(cask, artifact, "preflight_steps")? {
            artifacts.preflight_steps.extend(steps);
            continue;
        }
        if let Some(steps) = parse_flight_steps(cask, artifact, "postflight_steps")? {
            artifacts.postflight_steps.extend(steps);
            continue;
        }
        if is_non_install_artifact(&artifact_type) {
            collect_pkg_receipt_ids(artifact, &mut artifacts.pkg_ids);
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
        if let Some(wrapper) = parse_command_wrapper_artifact(artifact)? {
            artifacts.command_wrappers.push(wrapper);
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
        if let Some(completion) = parse_completion_artifact(artifact)? {
            artifacts.completions.push(completion);
            continue;
        }
        if let Some(generated) = parse_generated_completion_artifact(artifact)? {
            artifacts.generated_completions.push(generated);
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
        && artifacts.command_wrappers.is_empty()
        && artifacts.pkgs.is_empty()
        && artifacts.fonts.is_empty()
        && artifacts.completions.is_empty()
        && artifacts.generated_completions.is_empty()
    {
        bail!(
            "brew-cask:{}: no app, binary, command wrapper, pkg, font, or completion artifact found; only app-bundle, binary, command-wrapper, pkg, font, and completion casks are supported",
            cask.token
        );
    }
    artifacts.pkg_ids.sort();
    artifacts.pkg_ids.dedup();
    if artifacts.pkgs.is_empty() {
        artifacts.pkg_ids.clear();
    } else if artifacts.pkg_ids.is_empty() {
        bail!(
            "brew-cask:{}: pkg artifacts require pkgutil ids in uninstall metadata",
            cask.token
        );
    }
    Ok(artifacts)
}

fn validate_platform_support(cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let font_only = !artifacts.fonts.is_empty()
            && artifacts.apps.is_empty()
            && artifacts.binaries.is_empty()
            && artifacts.command_wrappers.is_empty()
            && artifacts.pkgs.is_empty()
            && artifacts.completions.is_empty()
            && artifacts.generated_completions.is_empty()
            && artifacts.preflight_steps.is_empty()
            && artifacts.postflight_steps.is_empty()
            && !has_lifecycle_hook(cask, "preflight")
            && !has_lifecycle_hook(cask, "postflight");
        if !font_only {
            bail!(
                "brew-cask:{}: only font-only casks without lifecycle hooks are supported on linux",
                cask.token
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (cask, artifacts);
    Ok(())
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

fn parse_command_wrapper_artifact(value: &Value) -> Result<Option<CommandWrapperArtifact>> {
    let Some(wrapper) = value.as_object().and_then(|o| o.get("command_wrapper")) else {
        return Ok(None);
    };
    let values = wrapper
        .as_array()
        .ok_or_else(|| eyre!("brew-cask: command_wrapper metadata must be an array"))?;
    let name = values
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("brew-cask: command_wrapper requires a command name"))?;
    let name_path = Path::new(name);
    if name_path.file_name().and_then(|name| name.to_str()) != Some(name)
        || matches!(name, "." | "..")
    {
        bail!("brew-cask: command_wrapper requires a command name without path components");
    }
    let options = values
        .get(1)
        .and_then(Value::as_object)
        .ok_or_else(|| eyre!("brew-cask: command_wrapper requires options"))?;
    let mut unsupported = options
        .keys()
        .filter(|key| !matches!(key.as_str(), "content" | "executable" | "args" | "env"))
        .cloned()
        .collect::<Vec<_>>();
    unsupported.sort();
    if !unsupported.is_empty() {
        bail!(
            "brew-cask: command_wrapper has unsupported option {}",
            unsupported.join(", ")
        );
    }
    let content = options
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string);
    let executable = options
        .get("executable")
        .and_then(Value::as_str)
        .map(str::to_string);
    match (content.is_some(), executable.is_some()) {
        (false, false) => {
            bail!("brew-cask: command_wrapper requires content or executable")
        }
        (true, true) => {
            bail!("brew-cask: command_wrapper requires content or executable, not both")
        }
        _ => {}
    }
    let args = options
        .get("args")
        .map(|args| {
            args.as_array()
                .ok_or_else(|| eyre!("brew-cask: command_wrapper args must be an array"))?
                .iter()
                .map(|arg| {
                    arg.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| eyre!("brew-cask: command_wrapper args must be strings"))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let env = options
        .get("env")
        .map(|env| {
            env.as_object()
                .ok_or_else(|| eyre!("brew-cask: command_wrapper env must be an object"))?
                .iter()
                .map(|(key, value)| {
                    if !is_shell_env_name(key) {
                        bail!("brew-cask: invalid command_wrapper environment name '{key}'");
                    }
                    value
                        .as_str()
                        .map(|value| (key.clone(), value.to_string()))
                        .ok_or_else(|| {
                            eyre!("brew-cask: command_wrapper environment values must be strings")
                        })
                })
                .collect::<Result<BTreeMap<_, _>>>()
        })
        .transpose()?
        .unwrap_or_default();
    if content.is_some() && (!args.is_empty() || !env.is_empty()) {
        bail!("brew-cask: command_wrapper args and env require executable");
    }
    Ok(Some(CommandWrapperArtifact {
        name: name.to_string(),
        target: artifact_target(value, values),
        content,
        executable,
        args,
        env,
    }))
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

fn parse_completion_artifact(value: &Value) -> Result<Option<CompletionArtifact>> {
    for (key, shell) in [
        ("bash_completion", CompletionShell::Bash),
        ("fish_completion", CompletionShell::Fish),
        ("zsh_completion", CompletionShell::Zsh),
    ] {
        let Some(completion) = value.as_object().and_then(|o| o.get(key)) else {
            continue;
        };
        return parse_declared_completion_artifact(value, completion, shell);
    }
    Ok(None)
}

fn parse_declared_completion_artifact(
    value: &Value,
    completion: &Value,
    shell: CompletionShell,
) -> Result<Option<CompletionArtifact>> {
    match completion {
        Value::String(source) => Ok(Some(CompletionArtifact {
            shell,
            source: source.clone(),
            target: value
                .as_object()
                .and_then(|o| o.get("target"))
                .and_then(Value::as_str)
                .map(str::to_string),
        })),
        Value::Array(values) => {
            let Some(source) = values.first().and_then(Value::as_str) else {
                return Ok(None);
            };
            Ok(Some(CompletionArtifact {
                shell,
                source: source.to_string(),
                target: artifact_target(value, values),
            }))
        }
        _ => Ok(None),
    }
}

fn parse_generated_completion_artifact(
    value: &Value,
) -> Result<Option<GeneratedCompletionArtifact>> {
    let Some(generated) = value
        .as_object()
        .and_then(|o| o.get("generate_completions_from_executable"))
    else {
        return Ok(None);
    };
    let Value::Array(values) = generated else {
        return Ok(None);
    };
    if values.is_empty() {
        bail!("brew-cask: generate_completions_from_executable requires an executable");
    }
    let options = values.last().and_then(Value::as_object);
    let command_values = if options.is_some() {
        &values[..values.len() - 1]
    } else {
        values.as_slice()
    };
    let executable = command_values
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| {
            eyre!("brew-cask: generate_completions_from_executable requires an executable")
        })?
        .to_string();
    let args = command_values
        .iter()
        .skip(1)
        .map(|value| {
            value.as_str().map(str::to_string).ok_or_else(|| {
                eyre!("brew-cask: generate_completions_from_executable arguments must be strings")
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let shell_parameter_format = options
        .and_then(|o| o.get("shell_parameter_format"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let shells = options
        .and_then(|o| o.get("shells"))
        .and_then(Value::as_array)
        .map(|shells| {
            shells
                .iter()
                .map(|shell| {
                    let shell = shell.as_str().ok_or_else(|| {
                        eyre!("brew-cask: completion shell names must be strings")
                    })?;
                    CompletionShell::parse(shell)
                        .ok_or_else(|| eyre!("brew-cask: unsupported completion shell '{shell}'"))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_else(|| default_generated_completion_shells(shell_parameter_format.as_deref()));
    if shells.is_empty() {
        bail!("brew-cask: generate_completions_from_executable requires at least one shell");
    }
    Ok(Some(GeneratedCompletionArtifact {
        executable,
        args,
        base_name: options
            .and_then(|o| o.get("base_name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        shell_parameter_format,
        shells,
    }))
}

fn default_generated_completion_shells(format: Option<&str>) -> Vec<CompletionShell> {
    match format {
        Some("cobra") | Some("typer") => vec![
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
            CompletionShell::Pwsh,
        ],
        _ => vec![
            CompletionShell::Bash,
            CompletionShell::Zsh,
            CompletionShell::Fish,
        ],
    }
}

fn parse_flight_steps(cask: &Cask, value: &Value, kind: &str) -> Result<Option<Vec<FlightStep>>> {
    let Some(metadata) = value.as_object().and_then(|o| o.get(kind)) else {
        return Ok(None);
    };
    let groups = metadata.as_array().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} metadata format",
            cask.token
        )
    })?;
    let mut steps = Vec::new();
    for group in groups {
        let group = group.as_object().ok_or_else(|| {
            eyre!(
                "brew-cask:{}: unsupported {kind} metadata format",
                cask.token
            )
        })?;
        reject_unsupported_flight_fields(cask, kind, "step group", group, &["steps"])?;
        let group_steps = group
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: unsupported {kind} metadata format",
                    cask.token
                )
            })?;
        for step in group_steps {
            steps.push(parse_flight_step(cask, kind, step)?);
        }
    }
    Ok(Some(steps))
}

fn parse_flight_step(cask: &Cask, kind: &str, value: &Value) -> Result<FlightStep> {
    let object = value.as_object().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} step metadata format",
            cask.token
        )
    })?;
    let step_type = object.get("type").and_then(Value::as_str).ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} step metadata format",
            cask.token
        )
    })?;
    match step_type {
        "move" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "move step",
                object,
                &["type", "source", "target", "source_glob"],
            )?;
            Ok(FlightStep::Move {
                source: parse_flight_path(cask, kind, "source", object.get("source"))?,
                target: parse_flight_path(cask, kind, "target", object.get("target"))?,
                source_glob: object
                    .get("source_glob")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
        "remove" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "remove step",
                object,
                &["type", "paths", "recursive"],
            )?;
            let paths = object
                .get("paths")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: unsupported {kind} remove step metadata format",
                        cask.token
                    )
                })?
                .iter()
                .map(|path| parse_flight_path(cask, kind, "paths", Some(path)))
                .collect::<Result<Vec<_>>>()?;
            Ok(FlightStep::Remove {
                paths,
                recursive: object
                    .get("recursive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        }
        "run" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "run step",
                object,
                &["type", "command", "args", "env", "sudo", "guards"],
            )?;
            let args = object
                .get("args")
                .map(|args| {
                    args.as_array()
                        .ok_or_else(|| {
                            eyre!(
                                "brew-cask:{}: unsupported {kind} run args metadata format",
                                cask.token
                            )
                        })?
                        .iter()
                        .map(|arg| {
                            arg.as_str().map(str::to_string).ok_or_else(|| {
                                eyre!(
                                    "brew-cask:{}: unsupported {kind} run argument metadata format",
                                    cask.token
                                )
                            })
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            let env = object
                .get("env")
                .map(|env| {
                    env.as_object()
                        .ok_or_else(|| {
                            eyre!(
                                "brew-cask:{}: unsupported {kind} run env metadata format",
                                cask.token
                            )
                        })?
                        .iter()
                        .map(|(key, value)| {
                            value
                                .as_str()
                                .map(|value| (key.clone(), value.to_string()))
                                .ok_or_else(|| {
                                    eyre!(
                                        "brew-cask:{}: unsupported {kind} run env value metadata format",
                                        cask.token
                                    )
                                })
                        })
                        .collect::<Result<BTreeMap<_, _>>>()
                })
                .transpose()?
                .unwrap_or_default();
            let guards = object
                .get("guards")
                .map(|guards| {
                    guards
                        .as_array()
                        .ok_or_else(|| {
                            eyre!(
                                "brew-cask:{}: unsupported {kind} run guards metadata format",
                                cask.token
                            )
                        })?
                        .iter()
                        .map(|guard| parse_flight_guard(cask, kind, guard))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok(FlightStep::Run {
                command: parse_run_command(cask, kind, object.get("command"))?,
                args,
                env,
                sudo: object.get("sudo").and_then(Value::as_bool).unwrap_or(false),
                guards,
            })
        }
        "terminate_process" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "terminate_process step",
                object,
                &[
                    "type",
                    "name",
                    "match",
                    "sudo",
                    "attempts",
                    "must_succeed",
                    "notices",
                    "failure_message",
                ],
            )?;
            let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: {kind} terminate_process name must be a string",
                    cask.token
                )
            })?;
            if name.is_empty() {
                bail!(
                    "brew-cask:{}: {kind} terminate_process name must not be empty",
                    cask.token
                );
            }
            let match_mode = match object.get("match") {
                None => ProcessMatch::Name,
                Some(Value::String(value)) if value == "name" => ProcessMatch::Name,
                Some(Value::String(value)) if value == "full" => ProcessMatch::Full,
                _ => bail!(
                    "brew-cask:{}: {kind} terminate_process match must be name or full",
                    cask.token
                ),
            };
            let sudo = parse_optional_flight_bool(cask, kind, object, "sudo", false)?;
            let must_succeed =
                parse_optional_flight_bool(cask, kind, object, "must_succeed", false)?;
            let attempts = match object.get("attempts") {
                None => 1,
                Some(value) => value
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        eyre!(
                            "brew-cask:{}: {kind} terminate_process attempts must be a positive integer",
                            cask.token
                        )
                    })?,
            };
            let notices = match object.get("notices") {
                None => Vec::new(),
                Some(Value::Array(values)) => values
                    .iter()
                    .map(|value| {
                        value.as_str().map(str::to_string).ok_or_else(|| {
                            eyre!(
                                "brew-cask:{}: {kind} terminate_process notices must be strings",
                                cask.token
                            )
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                Some(_) => bail!(
                    "brew-cask:{}: {kind} terminate_process notices must be an array",
                    cask.token
                ),
            };
            let failure_message = match object.get("failure_message") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) => Some(value.clone()),
                Some(_) => bail!(
                    "brew-cask:{}: {kind} terminate_process failure_message must be a string",
                    cask.token
                ),
            };
            Ok(FlightStep::TerminateProcess {
                name: name.to_string(),
                match_mode,
                sudo,
                attempts,
                must_succeed,
                notices,
                failure_message,
            })
        }
        _ => bail!(
            "brew-cask:{}: unsupported {kind} step type {}",
            cask.token,
            step_type
        ),
    }
}

fn parse_optional_flight_bool(
    cask: &Cask,
    kind: &str,
    object: &serde_json::Map<String, Value>,
    field: &str,
    default: bool,
) -> Result<bool> {
    match object.get(field) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!(
            "brew-cask:{}: {kind} terminate_process {field} must be a boolean",
            cask.token
        ),
    }
}

fn parse_run_command(cask: &Cask, kind: &str, value: Option<&Value>) -> Result<FlightPath> {
    let object = value.and_then(Value::as_object).ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} run command metadata format",
            cask.token
        )
    })?;
    reject_unsupported_flight_fields(cask, kind, "run command", object, &["base", "path"])?;
    let path = object.get("path").and_then(Value::as_str).ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} run command path",
            cask.token
        )
    })?;
    let base = match object.get("base").and_then(Value::as_str) {
        Some("staged_path") => FlightPathBase::StagedPath,
        Some("appdir") => FlightPathBase::AppDir,
        Some("homebrew_prefix") => FlightPathBase::HomebrewPrefix,
        Some(base) => bail!(
            "brew-cask:{}: unsupported {kind} run command base {}",
            cask.token,
            base
        ),
        None => FlightPathBase::Absolute,
    };
    let path_value = Path::new(path);
    let invalid_absolute_path = base == FlightPathBase::Absolute
        && !path_value.is_absolute()
        && path_value.components().count() > 1;
    let invalid_based_path = matches!(
        base,
        FlightPathBase::StagedPath | FlightPathBase::AppDir | FlightPathBase::HomebrewPrefix
    ) && (path_value.is_absolute()
        || path_value
            .components()
            .any(|component| matches!(component, Component::ParentDir)));
    if invalid_absolute_path || invalid_based_path {
        bail!(
            "brew-cask:{}: invalid {kind} run command path {}",
            cask.token,
            path
        );
    }
    Ok(FlightPath {
        base,
        path: path.to_string(),
    })
}

fn parse_flight_guard(cask: &Cask, kind: &str, value: &Value) -> Result<FlightGuard> {
    let object = value.as_object().ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} run guard metadata format",
            cask.token
        )
    })?;
    reject_unsupported_flight_fields(
        cask,
        kind,
        "run guard",
        object,
        &["condition", "value", "base", "path", "id"],
    )?;
    match object.get("condition").and_then(Value::as_str) {
        Some("on") => match object.get("value").and_then(Value::as_str) {
            Some("macos") => Ok(FlightGuard::OnMacos),
            Some("linux") => Ok(FlightGuard::OnLinux),
            Some(value) => bail!(
                "brew-cask:{}: unsupported {kind} run guard platform {}",
                cask.token,
                value
            ),
            None => bail!(
                "brew-cask:{}: unsupported {kind} run guard platform",
                cask.token
            ),
        },
        Some(condition @ ("if_exists" | "unless_exists")) => {
            let path = parse_context_flight_path(cask, kind, "run guard", object)?;
            if condition == "if_exists" {
                Ok(FlightGuard::IfExists(path))
            } else {
                Ok(FlightGuard::UnlessExists(path))
            }
        }
        Some(condition) => bail!(
            "brew-cask:{}: unsupported {kind} run guard condition {}",
            cask.token,
            condition
        ),
        None => bail!(
            "brew-cask:{}: unsupported {kind} run guard condition",
            cask.token
        ),
    }
}

fn parse_context_flight_path(
    cask: &Cask,
    kind: &str,
    field: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<FlightPath> {
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("brew-cask:{}: unsupported {kind} {field} path", cask.token))?;
    let base = match object.get("base").and_then(Value::as_str) {
        Some("staged_path") => FlightPathBase::StagedPath,
        Some("appdir") => FlightPathBase::AppDir,
        Some("homebrew_prefix") => FlightPathBase::HomebrewPrefix,
        Some(base) => bail!(
            "brew-cask:{}: unsupported {kind} {field} base {}",
            cask.token,
            base
        ),
        None => FlightPathBase::Absolute,
    };
    Ok(FlightPath {
        base,
        path: path.to_string(),
    })
}

fn reject_unsupported_flight_fields(
    cask: &Cask,
    kind: &str,
    context: &str,
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<()> {
    let mut unsupported = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unsupported.sort();
    if !unsupported.is_empty() {
        bail!(
            "brew-cask:{}: unsupported {kind} {context} field {}",
            cask.token,
            unsupported.join(", ")
        );
    }
    Ok(())
}

fn parse_flight_path(
    cask: &Cask,
    kind: &str,
    field: &str,
    value: Option<&Value>,
) -> Result<FlightPath> {
    let object = value.and_then(Value::as_object).ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} {field} metadata format",
            cask.token
        )
    })?;
    let base = match object.get("base").and_then(Value::as_str) {
        Some("staged_path") => FlightPathBase::StagedPath,
        Some(base) => bail!(
            "brew-cask:{}: unsupported {kind} {field} base {}",
            cask.token,
            base
        ),
        None => bail!("brew-cask:{}: unsupported {kind} {field} base", cask.token),
    };
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("brew-cask:{}: unsupported {kind} {field} path", cask.token))?;
    if validate_flight_relative_path(path).is_err() {
        bail!(
            "brew-cask:{}: invalid {kind} {field} path {}",
            cask.token,
            path
        )
    }
    Ok(FlightPath {
        base,
        path: path.to_string(),
    })
}

fn collect_pkg_receipt_ids(value: &Value, pkg_ids: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        return;
    };
    let Some(metadata) = object.get("uninstall") else {
        return;
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
    if target_name.contains('/') {
        let target = target_name.replace("$HOMEBREW_PREFIX", &prefix::prefix().to_string_lossy());
        let path = PathBuf::from(target);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            bail!("brew-cask: app target '{target_name}' must not contain '..'");
        }
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

fn allowed_appdir_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        prefix::prefix().join("Applications"),
    ]
}

fn is_appdir_binary_target(target_name: &str) -> bool {
    target_name.starts_with("$APPDIR/")
}

fn allowed_binary_target_roots_display(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(" or ")
}

fn binary_target_path(target_name: &str, appdir: &Path) -> Result<PathBuf> {
    if target_name.contains('\0') {
        bail!("brew-cask: binary target contains NUL");
    }
    if let Some(relative) = target_name.strip_prefix("$APPDIR/") {
        let relative = Path::new(relative);
        if relative.components().next().is_none()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("brew-cask: binary $APPDIR target '{target_name}' must stay below Applications");
        }
        if !allowed_appdir_roots().iter().any(|root| root == appdir) {
            bail!("brew-cask: invalid appdir '{}'", appdir.display());
        }
        return Ok(appdir.join(relative));
    }
    if target_name.contains("$APPDIR") {
        bail!("brew-cask: $APPDIR must prefix a binary target");
    }
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
    let roots = allowed_binary_target_roots();
    if !roots.iter().any(|root| target.starts_with(root)) {
        bail!(
            "brew-cask: binary target '{}' must be under {}",
            target.display(),
            allowed_binary_target_roots_display(&roots)
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

fn homebrew_metadata_present(token: &str) -> bool {
    caskroom_token_dir(token)
        .join(".metadata")
        .symlink_metadata()
        .is_ok()
}

fn pkg_id_installed(pkg_id: &str) -> Result<bool> {
    #[cfg(not(target_os = "macos"))]
    bail!("brew-cask: pkgutil receipt check for '{pkg_id}' is only available on macOS");

    #[cfg(target_os = "macos")]
    let output = std::process::Command::new("pkgutil")
        .arg("--pkg-info")
        .arg(pkg_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    #[cfg(target_os = "macos")]
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
    let appdir = cask_appdir(&artifacts.apps)?;
    artifacts
        .binaries
        .iter()
        .map(|binary| binary.target_path(&appdir))
        .chain(
            artifacts
                .command_wrappers
                .iter()
                .map(CommandWrapperArtifact::target_path),
        )
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
            remove_artifact_target_elevating(target)?;
        }
    }
    Ok(())
}

#[cfg(not(test))]
fn installed_cask_version(cask: &Cask, artifacts: &CaskArtifacts) -> Result<Option<String>> {
    installed_cask_version_in(cask, artifacts, &crate::dirs::STATE)
}

#[cfg(test)]
fn installed_cask_version(cask: &Cask, artifacts: &CaskArtifacts) -> Result<Option<String>> {
    installed_cask_version_in(cask, artifacts, &prefix::prefix().join(".mise-test-state"))
}

fn installed_cask_version_in(
    cask: &Cask,
    _artifacts: &CaskArtifacts,
    state_dir: &Path,
) -> Result<Option<String>> {
    if cask_journal_pending_in(state_dir, &cask.token) {
        return Ok(None);
    }
    let Some(version) = installed_version(&cask.token) else {
        return Ok(None);
    };
    let version_dir = caskroom_version_dir(&cask.token, &version);
    match read_receipt(&version_dir)? {
        Some(receipt) => {
            if receipt.schema_version > 3 {
                return Ok(None);
            }
            if receipt.schema_version >= 2 {
                let targets_match = receipt
                    .targets
                    .iter()
                    .map(cask_target_record_matches)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .all(|matches| matches);
                let pkgs_installed = pkg_ids_installed(&receipt.pkg_ids)?;
                return Ok((targets_match && pkgs_installed).then_some(receipt.version));
            }

            // Legacy receipts remain usable only from the historical facts they
            // actually contain. Never fill omitted fields from today's API.
            let targets_exist = receipt
                .apps
                .iter()
                .chain(&receipt.binaries)
                .chain(&receipt.fonts)
                .chain(&receipt.completions)
                .all(|target| target.exists());
            let pkgs_installed = pkg_ids_installed(&receipt.pkg_ids)?;
            Ok((targets_exist && pkgs_installed).then_some(receipt.version))
        }
        None => Ok(None),
    }
}

fn cask_prune_blocker(cask: &Cask, artifacts: &CaskArtifacts) -> Option<String> {
    if !artifacts.pkgs.is_empty() {
        return Some("pkg artifacts require uninstall support".to_string());
    }
    if !artifacts.command_wrappers.is_empty() {
        return Some("command wrapper artifacts are not supported for pruning".to_string());
    }
    if !artifacts.preflight_steps.is_empty()
        || !artifacts.postflight_steps.is_empty()
        || has_lifecycle_hook(cask, "preflight")
        || has_lifecycle_hook(cask, "postflight")
    {
        return Some("install lifecycle actions may have untracked side effects".to_string());
    }
    if cask.artifacts.iter().any(|artifact| {
        matches!(
            artifact_type(artifact).as_str(),
            "uninstall"
                | "uninstall_preflight"
                | "uninstall_preflight_steps"
                | "uninstall_postflight"
                | "uninstall_postflight_steps"
        )
    }) {
        return Some("uninstall lifecycle actions are not supported".to_string());
    }
    None
}

fn write_receipt(caskroom: &Path, cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    let mut target_paths = artifacts
        .apps
        .iter()
        .map(|app| app_target_path(app.target_name()))
        .collect::<Result<Vec<_>>>()?;
    target_paths.extend(binary_targets(artifacts)?);
    target_paths.extend(
        artifacts
            .fonts
            .iter()
            .map(font_target_path)
            .collect::<Result<Vec<_>>>()?,
    );
    target_paths.extend(completion_target_paths(cask, artifacts)?);
    target_paths.sort();
    target_paths.dedup();
    let targets = target_paths
        .iter()
        .map(|path| {
            Ok(CaskTargetRecord {
                path: path.clone(),
                fingerprint: cask_target_fingerprint(path)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let prune_blocker = cask_prune_blocker(cask, artifacts);
    let receipt = CaskReceipt {
        schema_version: 3,
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
        completions: completion_target_paths(cask, artifacts)?,
        pkg_ids: artifacts.pkg_ids.clone(),
        targets,
        prune_safe: prune_blocker.is_none(),
        prune_blocker,
    };
    let body = toml::to_string_pretty(&receipt)?;
    write_durable_file(&caskroom.join(".mise-cask.toml"), body.as_bytes())?;
    Ok(())
}

fn cask_target_record_matches(record: &CaskTargetRecord) -> Result<bool> {
    let Ok(actual) = cask_target_fingerprint(&record.path) else {
        return Ok(false);
    };
    Ok(actual == record.fingerprint)
}

fn cask_target_fingerprint(path: &Path) -> Result<CaskTargetFingerprint> {
    let metadata = path
        .symlink_metadata()
        .wrap_err_with(|| format!("failed to fingerprint {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        return Ok(CaskTargetFingerprint {
            kind: CaskTargetKind::Symlink,
            digest: hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
        });
    }
    if metadata.is_file() {
        return Ok(CaskTargetFingerprint {
            kind: CaskTargetKind::File,
            digest: hash::file_hash_sha256(path, None)?,
        });
    }
    if metadata.is_dir() {
        return Ok(CaskTargetFingerprint {
            kind: CaskTargetKind::Directory,
            digest: cask_directory_digest(path)?,
        });
    }
    bail!("brew-cask: unsupported target type '{}'", path.display())
}

/// Content identity intentionally excludes timestamps, ownership, and modes.
/// It hashes stable relative paths, entry kinds, file bytes, and link targets
/// without following symlinks.
fn cask_directory_digest(root: &Path) -> Result<String> {
    let mut entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    let mut digest = Sha256::new();
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let metadata = path.symlink_metadata()?;
        digest.update([if metadata.file_type().is_symlink() {
            b'l'
        } else if metadata.is_dir() {
            b'd'
        } else if metadata.is_file() {
            b'f'
        } else {
            bail!(
                "brew-cask: unsupported directory entry '{}'",
                path.display()
            );
        }]);
        hash_digest_field(&mut digest, relative.as_os_str().as_encoded_bytes());
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(path)?;
            hash_digest_field(&mut digest, target.as_os_str().as_encoded_bytes());
        } else if metadata.is_dir() {
        } else if metadata.is_file() {
            hash_digest_field(&mut digest, hash::file_hash_sha256(path, None)?.as_bytes());
        }
    }
    Ok(hex::encode(digest.finalize()))
}

fn hash_digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

fn cask_journal_path_in(state_dir: &Path, token: &str, version: &str) -> PathBuf {
    state_dir
        .join("brew-cask")
        .join(token)
        .join(format!("{version}.json"))
}

fn cask_journal_pending_in(state_dir: &Path, token: &str) -> bool {
    state_dir
        .join("brew-cask")
        .join(token)
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_some())
}

fn write_cask_journal(journal: &CaskTransactionJournal<'_>) -> Result<()> {
    write_cask_journal_in(&crate::dirs::STATE, journal)
}

fn write_cask_journal_in(state_dir: &Path, journal: &CaskTransactionJournal<'_>) -> Result<()> {
    let path = cask_journal_path_in(state_dir, journal.token, journal.version);
    let body = serde_json::to_vec_pretty(journal)?;
    write_durable_file(&path, &body)
}

fn record_cask_action(journal: &mut CaskTransactionJournal<'_>, action: &str) -> Result<()> {
    record_cask_action_in(&crate::dirs::STATE, journal, action)
}

fn record_cask_action_in(
    state_dir: &Path,
    journal: &mut CaskTransactionJournal<'_>,
    action: &str,
) -> Result<()> {
    journal.completed.push(action.to_string());
    write_cask_journal_in(state_dir, journal)
}

fn remove_cask_journals(token: &str) -> Result<()> {
    remove_cask_journals_in(&crate::dirs::STATE, token)
}

fn remove_cask_journals_in(state_dir: &Path, token: &str) -> Result<()> {
    let path = state_dir.join("brew-cask").join(token);
    if path.symlink_metadata().is_ok() {
        file::remove_all(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

fn write_durable_file(path: &Path, body: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("brew-cask: durable file has no parent"))?;
    file::create_dir_all(parent)?;
    let tmp = path.with_extension("tmp");
    {
        let mut output = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut output, body)?;
        output.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(parent)?.sync_all()?;
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

pub async fn cask_prune_plan(configured: &[PackageRequest]) -> Result<CaskPrunePlan> {
    let mut keep = BTreeSet::new();
    for request in configured {
        keep.insert(fetch_cask(request).await?.token);
    }
    cask_prune_plan_from_tokens(&keep, &crate::dirs::STATE)
}

fn cask_prune_plan_from_tokens(keep: &BTreeSet<String>, state_dir: &Path) -> Result<CaskPrunePlan> {
    let mut plan = CaskPrunePlan::default();
    let mut candidates = Vec::new();
    let mut claims = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    let mut claims_complete = true;
    let caskroom = prefix::prefix().join("Caskroom");
    let Ok(tokens) = std::fs::read_dir(&caskroom) else {
        return Ok(plan);
    };

    for entry in tokens.filter_map(|entry| entry.ok()) {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Some(token) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if token.starts_with('.') {
            continue;
        }
        let configured = keep.contains(&token);
        let Ok(version_entries) = std::fs::read_dir(entry.path()) else {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "Caskroom directory could not be read".to_string(),
                });
            }
            continue;
        };
        let versions = version_entries
            .filter_map(|version| version.ok())
            .filter(|version| {
                version.file_type().is_ok_and(|kind| kind.is_dir())
                    && !version.file_name().to_string_lossy().starts_with('.')
            })
            .collect::<Vec<_>>();
        let mut receipts = BTreeMap::new();
        let mut receipt_error = None;
        for version in &versions {
            match read_receipt(&version.path()) {
                Ok(Some(receipt)) => {
                    for target in &receipt.targets {
                        claims
                            .entry(target.path.clone())
                            .or_default()
                            .insert(token.clone());
                    }
                    receipts.insert(version.path(), receipt);
                }
                Ok(None) => {}
                Err(err) => {
                    claims_complete = false;
                    receipt_error =
                        Some(format!("mise ownership receipt could not be read: {err:#}"));
                }
            }
        }
        if let Some(reason) = receipt_error {
            if !configured {
                plan.skipped.push(CaskPruneSkip { token, reason });
            }
            continue;
        }
        if entry.path().join(".metadata").symlink_metadata().is_ok() {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "Homebrew owns this cask".to_string(),
                });
            }
            continue;
        }
        let [version] = versions.as_slice() else {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "expected exactly one installed Caskroom version".to_string(),
                });
            }
            continue;
        };
        let version_dir = version.path();
        let Some(receipt) = receipts.remove(&version_dir) else {
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "mise ownership receipt is missing".to_string(),
                });
            }
            continue;
        };
        if configured {
            continue;
        }
        if cask_journal_pending_in(state_dir, &token) {
            plan.skipped.push(CaskPruneSkip {
                token,
                reason: "an incomplete cask transaction is pending".to_string(),
            });
            continue;
        }
        if receipt.schema_version != 3 {
            plan.skipped.push(CaskPruneSkip {
                token,
                reason: "receipt predates safe prune metadata; upgrade or reinstall first"
                    .to_string(),
            });
            continue;
        }
        if !receipt.prune_safe {
            let reason = receipt
                .prune_blocker
                .clone()
                .unwrap_or_else(|| "receipt does not permit pruning".to_string());
            plan.skipped.push(CaskPruneSkip { token, reason });
            continue;
        }
        let version = version.file_name().to_string_lossy().to_string();
        let candidate = CaskPruneCandidate {
            token,
            version,
            version_dir,
            receipt,
        };
        if let Err(reason) = validate_cask_prune_candidate(&candidate) {
            plan.skipped.push(CaskPruneSkip {
                token: candidate.token,
                reason: format!("recorded artifacts cannot be removed safely: {reason:#}"),
            });
            continue;
        }
        candidates.push(candidate);
    }

    for candidate in candidates {
        if !claims_complete {
            plan.skipped.push(CaskPruneSkip {
                token: candidate.token,
                reason: "cask ownership receipts could not be indexed completely".to_string(),
            });
            continue;
        }
        let shared = candidate
            .receipt
            .targets
            .iter()
            .filter_map(|target| {
                claims
                    .get(&target.path)
                    .filter(|tokens| tokens.len() > 1)
                    .map(|_| target.path.clone())
            })
            .collect::<Vec<_>>();
        if shared.is_empty() {
            plan.remove.push(candidate);
        } else {
            plan.skipped.push(CaskPruneSkip {
                token: candidate.token,
                reason: format!(
                    "recorded artifact target is also claimed by another cask: {}",
                    shared
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }
    plan.remove.sort_by(|a, b| a.token.cmp(&b.token));
    plan.skipped.sort_by(|a, b| a.token.cmp(&b.token));
    Ok(plan)
}

pub fn apply_cask_prune_plan(plan: &CaskPrunePlan, dry_run: bool) -> Result<usize> {
    apply_cask_prune_plan_in(plan, dry_run, &crate::dirs::STATE)
}

fn apply_cask_prune_plan_in(
    plan: &CaskPrunePlan,
    dry_run: bool,
    state_dir: &Path,
) -> Result<usize> {
    if dry_run {
        for candidate in &plan.remove {
            miseprintln!("remove brew-cask:{}@{}", candidate.token, candidate.version);
        }
        return Ok(0);
    }

    let mut removed = 0;
    for candidate in &plan.remove {
        if let Err(reason) = validate_cask_prune_candidate(candidate)
            .and_then(|_| validate_cask_prune_claims(candidate))
        {
            warn!(
                "brew-cask:{}: skipped because recorded artifacts changed after planning: {reason:#}",
                candidate.token
            );
            continue;
        }
        let mut journal = CaskTransactionJournal {
            schema_version: 1,
            token: &candidate.token,
            version: &candidate.version,
            completed: Vec::new(),
        };
        write_cask_journal_in(state_dir, &journal)?;
        for (index, target) in candidate.receipt.targets.iter().enumerate() {
            remove_artifact_target_elevating(&target.path)?;
            record_cask_action_in(state_dir, &mut journal, &format!("prune_target[{index}]"))?;
        }
        file::remove_all(&candidate.version_dir)?;
        record_cask_action_in(state_dir, &mut journal, "prune_caskroom")?;
        if let Some(token_dir) = candidate.version_dir.parent() {
            file::remove_dir(token_dir)?;
        }
        remove_cask_journals_in(state_dir, &candidate.token)?;
        removed += 1;
    }
    Ok(removed)
}

fn validate_cask_prune_claims(candidate: &CaskPruneCandidate) -> Result<()> {
    let caskroom = prefix::prefix().join("Caskroom");
    let mut claims = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for entry in std::fs::read_dir(&caskroom)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let token = entry.file_name().to_string_lossy().to_string();
        for version in std::fs::read_dir(entry.path())? {
            let version = version?;
            if !version.file_type()?.is_dir()
                || version.file_name().to_string_lossy().starts_with('.')
            {
                continue;
            }
            if let Some(receipt) = read_receipt(&version.path())? {
                for target in receipt.targets {
                    claims.entry(target.path).or_default().insert(token.clone());
                }
            }
        }
    }
    for target in &candidate.receipt.targets {
        if claims
            .get(&target.path)
            .is_some_and(|tokens| tokens.iter().any(|token| token != &candidate.token))
        {
            bail!(
                "artifact target is now claimed by another cask: {}",
                target.path.display()
            );
        }
    }
    Ok(())
}

fn validate_cask_prune_candidate(candidate: &CaskPruneCandidate) -> Result<()> {
    let receipt = &candidate.receipt;
    if read_receipt(&candidate.version_dir)?.as_ref() != Some(receipt) {
        bail!("ownership receipt has changed");
    }
    if receipt.schema_version != 3 || !receipt.prune_safe || !receipt.pkg_ids.is_empty() {
        bail!("receipt is not marked safe for direct-artifact pruning");
    }
    let records = receipt
        .targets
        .iter()
        .map(|record| (record.path.clone(), record))
        .collect::<BTreeMap<_, _>>();
    let expected = receipt
        .apps
        .iter()
        .chain(&receipt.binaries)
        .chain(&receipt.fonts)
        .chain(&receipt.completions)
        .cloned()
        .collect::<BTreeSet<_>>();
    if expected.is_empty()
        || records.len() != receipt.targets.len()
        || records.len() != expected.len()
    {
        bail!("receipt target inventory is incomplete or duplicated");
    }
    if records.keys().any(|path| !expected.contains(path)) {
        bail!("receipt target inventory contains an unclassified path");
    }

    for path in &receipt.apps {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing app target record"))?;
        if record.fingerprint.kind != CaskTargetKind::Directory
            || !allowed_appdir_roots()
                .iter()
                .any(|root| path_is_below(path, root))
            || !path.file_name().is_some_and(|name| {
                staged_target_matches(record, &candidate.version_dir.join(name))
            })
        {
            bail!(
                "app target is outside an allowed Applications directory: {}",
                path.display()
            );
        }
    }
    for path in &receipt.binaries {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing binary target record"))?;
        if record.fingerprint.kind != CaskTargetKind::Symlink
            || !allowed_binary_target_roots()
                .iter()
                .any(|root| path_is_below(path, root))
            || !symlink_resolves_below(path, &candidate.version_dir)
        {
            bail!(
                "binary target is not an owned Caskroom symlink: {}",
                path.display()
            );
        }
    }
    for path in &receipt.fonts {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing font target record"))?;
        let fonts = font_dir();
        if record.fingerprint.kind != CaskTargetKind::File
            || !path_is_below(path, &fonts)
            || !path.strip_prefix(&fonts).is_ok_and(|relative| {
                staged_target_matches(record, &candidate.version_dir.join(relative))
            })
        {
            bail!(
                "font target is outside the platform font directory: {}",
                path.display()
            );
        }
    }
    let completion_roots = [
        CompletionShell::Bash,
        CompletionShell::Fish,
        CompletionShell::Zsh,
        CompletionShell::Pwsh,
    ]
    .map(default_completion_dir);
    for path in &receipt.completions {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing completion target record"))?;
        if record.fingerprint.kind != CaskTargetKind::Symlink
            || !completion_roots
                .iter()
                .any(|root| path_is_below(path, root))
            || !symlink_resolves_below(path, &candidate.version_dir)
        {
            bail!(
                "completion target is not an owned Caskroom symlink: {}",
                path.display()
            );
        }
    }
    for record in &receipt.targets {
        if !cask_target_record_matches(record)? {
            bail!("artifact target has changed: {}", record.path.display());
        }
    }
    Ok(())
}

fn path_is_below(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .is_ok_and(|relative| relative.components().next().is_some())
}

fn staged_target_matches(record: &CaskTargetRecord, staged: &Path) -> bool {
    cask_target_fingerprint(staged).is_ok_and(|fingerprint| fingerprint == record.fingerprint)
}

fn symlink_resolves_below(path: &Path, root: &Path) -> bool {
    let Ok(target) = std::fs::read_link(path) else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or_else(|| Path::new("/")).join(target)
    };
    path_starts_with_resolved_root(&target, root)
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

fn caskroom_backup_dir(cask: &Cask) -> PathBuf {
    let key = format!("{}-{}", cask.token, cask.version);
    caskroom_token_dir(&cask.token).join(format!(".mise-backup-{}", hash::hash_to_str(&key)))
}

#[derive(Debug)]
struct ArtifactLinkBackup {
    target: PathBuf,
    backup: Option<PathBuf>,
}

#[derive(Debug)]
struct ArtifactLinkTransaction {
    backups: Vec<ArtifactLinkBackup>,
}

impl ArtifactLinkTransaction {
    fn begin(mut targets: Vec<PathBuf>) -> Result<Self> {
        targets.sort();
        targets.dedup();
        let mut transaction = Self {
            backups: Vec::with_capacity(targets.len()),
        };
        for target in targets {
            let entry = (|| -> Result<ArtifactLinkBackup> {
                let backup = if target.symlink_metadata().is_ok() {
                    let parent = target
                        .parent()
                        .ok_or_else(|| eyre!("brew-cask: artifact target has no parent"))?;
                    let backup = parent.join(format!(
                        ".mise-link-backup-{}",
                        hash::hash_to_str(&target.display().to_string())
                    ));
                    remove_artifact_target_elevating(&backup)?;
                    rename_elevating(&target, &backup)?;
                    Some(backup)
                } else {
                    None
                };
                Ok(ArtifactLinkBackup { target, backup })
            })();
            match entry {
                Ok(entry) => transaction.backups.push(entry),
                Err(err) => {
                    if let Err(rollback_err) = transaction.rollback() {
                        return Err(err.wrap_err(format!(
                            "failed to restore artifact targets after backup failed: {rollback_err:#}"
                        )));
                    }
                    return Err(err);
                }
            }
        }
        Ok(transaction)
    }

    fn rollback(&mut self) -> Result<()> {
        let mut first_error = None;
        for entry in self.backups.iter().rev() {
            match remove_artifact_target_elevating(&entry.target) {
                Ok(()) => {
                    if let Some(backup) = &entry.backup
                        && let Err(err) = rename_elevating(backup, &entry.target)
                    {
                        first_error.get_or_insert(err);
                    }
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            self.backups.clear();
            Ok(())
        }
    }

    fn commit(&mut self) -> Result<()> {
        for entry in &self.backups {
            if let Some(backup) = &entry.backup {
                remove_artifact_target_elevating(backup)?;
            }
        }
        self.backups.clear();
        Ok(())
    }
}

fn replace_caskroom(
    cask: &Cask,
    staged: &Path,
    destination: &Path,
    link_artifacts: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let backup = caskroom_backup_dir(cask);
    file::remove_all(&backup)?;
    let had_previous = destination.symlink_metadata().is_ok();
    if had_previous {
        file::rename(destination, &backup)?;
    }
    if let Err(err) = file::rename(staged, destination) {
        if had_previous {
            file::rename(&backup, destination)?;
        }
        return Err(err);
    }
    if let Err(err) = link_artifacts() {
        let rollback = (|| -> Result<()> {
            file::remove_all(destination)?;
            if had_previous {
                file::rename(&backup, destination)?;
            }
            Ok(())
        })();
        if let Err(rollback_err) = rollback {
            return Err(err.wrap_err(format!(
                "failed to restore previous cask after activation failed: {rollback_err:#}"
            )));
        }
        return Err(err);
    }
    file::remove_all(backup)?;
    Ok(())
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
        "caveats"
            | "conflicts_with"
            | "depends_on"
            | "manpage"
            | "postflight"
            | "preflight"
            | "uninstall_postflight_steps"
            | "uninstall_preflight_steps"
            | "uninstall"
            | "uninstall_postflight"
            | "uninstall_preflight"
            | "zap"
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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
        run_cask_shim_hook(ruby, shim, cask, staged_path, version, "preflight")
    }

    fn run_cask_shim_hook(
        ruby: &Path,
        shim: &Path,
        cask: &Path,
        staged_path: &Path,
        version: &str,
        hook: &str,
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
            .env("MISE_BREW_CASK_HOOK", hook)
            .output()
    }

    fn test_cask(token: &str, version: &str) -> Cask {
        Cask {
            token: token.to_string(),
            aliases: Vec::new(),
            old_tokens: Vec::new(),
            version: version.to_string(),
            url: "https://example.com/example.zip".to_string(),
            url_specs: CaskUrlSpecs::default(),
            sha256: Some("no_check".to_string()),
            artifacts: Vec::new(),
            ruby_source_path: None,
            ruby_source_checksum: None,
            tap_git_head: None,
            raw_base: None,
        }
    }

    fn write_test_app_receipt(cask: &Cask, app_name: &str) -> Result<PathBuf> {
        let app = AppArtifact {
            source: app_name.to_string(),
            target: Some(format!("$HOMEBREW_PREFIX/Applications/{app_name}")),
        };
        let target = app_target_path(app.target_name())?;
        file::create_dir_all(&target)?;
        file::write(target.join("version"), "1.0.0")?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(version_dir.join(app_name))?;
        file::write(version_dir.join(app_name).join("version"), "1.0.0")?;
        write_receipt(
            &version_dir,
            cask,
            &CaskArtifacts {
                apps: vec![app],
                ..Default::default()
            },
        )?;
        Ok(target)
    }

    #[test]
    fn validates_requested_cask_identity_and_trusted_aliases() -> Result<()> {
        let cask = test_cask("current", "1.0.0");
        validate_cask_identity(&cask, "current", true)?;
        assert!(validate_cask_identity(&cask, "different", true).is_err());

        let mut aliased = cask.clone();
        aliased.old_tokens = vec!["old-name".to_string()];
        validate_cask_identity(&aliased, "old-name", true)?;
        assert!(validate_cask_identity(&aliased, "old-name", false).is_err());
        Ok(())
    }

    #[test]
    fn rejects_unsafe_cask_identity_components() {
        for value in ["", ".", "..", ".metadata", ".mise-tmp-x", "a/b", "a\0b"] {
            assert!(validate_cask_path_component("token", value).is_err());
        }
        assert!(validate_cask_path_component("token", "zed@preview").is_ok());
        assert!(validate_cask_path_component("version", "1.2.3,456").is_ok());
    }

    #[test]
    fn detects_any_foreign_homebrew_metadata_object() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let metadata = tmp.path().join("Caskroom/example/.metadata");
        assert!(!homebrew_metadata_present("example"));
        file::create_dir_all(&metadata)?;
        assert!(homebrew_metadata_present("example"));
        file::remove_all(&metadata)?;
        crate::file::write(&metadata, "foreign")?;
        assert!(homebrew_metadata_present("example"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn directory_fingerprint_tracks_tree_content_and_links() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("Example.app");
        file::create_dir_all(root.join("Contents/Resources"))?;
        crate::file::write(root.join("Contents/app"), "one")?;
        crate::file::write(root.join("Contents/Resources/config"), "config")?;
        std::os::unix::fs::symlink("app", root.join("Contents/current"))?;
        let original = cask_target_fingerprint(&root)?;
        assert_eq!(original, cask_target_fingerprint(&root)?);

        crate::file::write(root.join("Contents/app"), "two")?;
        assert_ne!(original, cask_target_fingerprint(&root)?);
        crate::file::write(root.join("Contents/app"), "one")?;
        assert_eq!(original, cask_target_fingerprint(&root)?);

        crate::file::write(root.join("Contents/added"), "added")?;
        assert_ne!(original, cask_target_fingerprint(&root)?);
        file::remove_file(root.join("Contents/added"))?;
        assert_eq!(original, cask_target_fingerprint(&root)?);

        file::remove_file(root.join("Contents/current"))?;
        std::os::unix::fs::symlink("Resources/config", root.join("Contents/current"))?;
        assert_ne!(original, cask_target_fingerprint(&root)?);

        file::remove_all(&root)?;
        file::create_dir_all(root.join("Contents/Resources"))?;
        crate::file::write(root.join("Contents/app"), "replacement")?;
        assert_ne!(original, cask_target_fingerprint(&root)?);
        Ok(())
    }

    #[test]
    fn completed_receipt_detects_app_bundle_drift() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("example", "1.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let artifacts = CaskArtifacts {
            apps: vec![app.clone()],
            ..Default::default()
        };
        let app_target = app_target_path(app.target_name())?;
        file::create_dir_all(app_target.join("Contents"))?;
        crate::file::write(app_target.join("Contents/app"), "original")?;
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        write_receipt(&caskroom, &cask, &artifacts)?;
        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some(cask.version.clone())
        );
        crate::file::write(app_target.join("Contents/app"), "changed")?;
        assert_eq!(installed_cask_version(&cask, &artifacts)?, None);
        Ok(())
    }

    #[test]
    fn parses_firefox_command_wrapper_artifact() -> Result<()> {
        let mut cask = test_cask("firefox", "153.0.1");
        cask.artifacts = vec![
            serde_json::json!({
                "app": ["Firefox.app"],
                "target": "/Applications/Firefox.app"
            }),
            serde_json::json!({
                "command_wrapper": [
                    "firefox",
                    {"executable": "$APPDIR/Firefox.app/Contents/MacOS/firefox"}
                ],
                "target": "$HOMEBREW_PREFIX/bin/firefox"
            }),
        ];

        let artifacts = cask_artifacts(&cask)?;
        assert_eq!(
            artifacts.command_wrappers,
            vec![CommandWrapperArtifact {
                name: "firefox".to_string(),
                target: Some("$HOMEBREW_PREFIX/bin/firefox".to_string()),
                content: None,
                executable: Some("$APPDIR/Firefox.app/Contents/MacOS/firefox".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
            }]
        );
        Ok(())
    }

    #[test]
    fn rejects_command_wrapper_invalid_environment_name() {
        let value = serde_json::json!({
            "command_wrapper": [
                "example",
                {
                    "executable": "/usr/bin/example",
                    "env": {"INVALID-NAME": "value"}
                }
            ]
        });

        let err = parse_command_wrapper_artifact(&value)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid command_wrapper environment name 'INVALID-NAME'"));
    }

    #[test]
    #[cfg(unix)]
    fn stages_command_wrapper_with_args_env_and_expanded_paths() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("firefox", "153.0.1");
        let caskroom = prefix.join("Caskroom/firefox/.mise-tmp");
        let final_caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let appdir = tmp.path().join("Applications");
        let wrapper = CommandWrapperArtifact {
            name: "firefox".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/firefox".to_string()),
            content: None,
            executable: Some("$APPDIR/Firefox.app/Contents/MacOS/firefox".to_string()),
            args: vec![
                "--profile".to_string(),
                "two words".to_string(),
                "{{version}}".to_string(),
            ],
            env: BTreeMap::from([
                ("FIREFOX_MODE".to_string(), "mise test".to_string()),
                ("FIREFOX_ROOT".to_string(), "{{staged_path}}".to_string()),
            ]),
        };

        stage_command_wrapper(&caskroom, &appdir, &cask, &wrapper)?;
        file::rename(&caskroom, &final_caskroom)?;
        link_command_wrapper(&final_caskroom, &wrapper)?;

        let staged = final_caskroom.join(".homebrew-command-wrappers/firefox");
        let contents = file::read_to_string(&staged)?;
        assert!(contents.starts_with("#!/bin/bash\n"));
        assert!(contents.contains("FIREFOX_MODE='mise test'"));
        assert!(contents.contains(&format!(
            "FIREFOX_ROOT={}",
            final_caskroom.to_string_lossy()
        )));
        let executable = appdir.join("Firefox.app/Contents/MacOS/firefox");
        assert!(contents.contains(executable.to_string_lossy().as_ref()));
        assert!(contents.contains("--profile 'two words' 153.0.1 \"$@\""));
        assert!(staged.metadata()?.permissions().mode() & 0o111 != 0);
        assert_eq!(std::fs::read_link(wrapper.target_path()?)?, staged);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn stages_command_wrapper_with_literal_content() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("example", "1.0.0");
        let caskroom = prefix.join("Caskroom/example/1.0.0");
        let wrapper = CommandWrapperArtifact {
            name: "example".to_string(),
            target: None,
            content: Some(
                "#!/bin/sh\nHOME=$HOME\nSTAGE={{staged_path}}\nexec '$HOMEBREW_PREFIX/bin/example' \"$@\"\n"
                    .to_string(),
            ),
            executable: None,
            args: Vec::new(),
            env: BTreeMap::new(),
        };

        stage_command_wrapper(&caskroom, Path::new("/Applications"), &cask, &wrapper)?;

        assert_eq!(
            file::read_to_string(caskroom.join(".homebrew-command-wrappers/example"))?,
            format!(
                "#!/bin/sh\nHOME=$HOME\nSTAGE={{{{staged_path}}}}\nexec '{}/bin/example' \"$@\"\n",
                prefix.display()
            )
        );
        Ok(())
    }

    #[test]
    fn parses_structured_flight_steps() -> Result<()> {
        let mut cask = test_cask("wezterm@nightly", "latest");
        cask.artifacts = vec![
            serde_json::json!({
                "preflight_steps": [{
                    "steps": [
                        {
                            "type": "move",
                            "source_glob": true,
                            "source": {
                                "base": "staged_path",
                                "path": "{WezTerm-*,wezterm-*}/WezTerm.app"
                            },
                            "target": {
                                "base": "staged_path",
                                "path": "."
                            }
                        },
                        {
                            "type": "remove",
                            "recursive": true,
                            "paths": [
                                {"base": "staged_path", "path": "WezTerm-*"},
                                {"base": "staged_path", "path": "wezterm-*"}
                            ]
                        }
                    ]
                }]
            }),
            serde_json::json!({"app": "WezTerm.app"}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                apps: vec![AppArtifact {
                    source: "WezTerm.app".to_string(),
                    target: None,
                }],
                preflight_steps: vec![
                    FlightStep::Move {
                        source: FlightPath {
                            base: FlightPathBase::StagedPath,
                            path: "{WezTerm-*,wezterm-*}/WezTerm.app".to_string(),
                        },
                        target: FlightPath {
                            base: FlightPathBase::StagedPath,
                            path: ".".to_string(),
                        },
                        source_glob: true,
                    },
                    FlightStep::Remove {
                        paths: vec![
                            FlightPath {
                                base: FlightPathBase::StagedPath,
                                path: "WezTerm-*".to_string(),
                            },
                            FlightPath {
                                base: FlightPathBase::StagedPath,
                                path: "wezterm-*".to_string(),
                            }
                        ],
                        recursive: true,
                    }
                ],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn parses_orbstack_structured_run_step() -> Result<()> {
        let mut cask = test_cask("orbstack", "2.2.1,20628");
        cask.artifacts = vec![
            serde_json::json!({
                "app": ["OrbStack.app"],
                "target": "/Applications/OrbStack.app"
            }),
            serde_json::json!({
                "postflight_steps": [{
                    "steps": [{
                        "command": {
                            "base": "appdir",
                            "path": "OrbStack.app/Contents/MacOS/bin/orbctl"
                        },
                        "type": "run",
                        "args": ["_internal", "brew-postflight"]
                    }]
                }]
            }),
        ];

        let artifacts = cask_artifacts(&cask)?;
        assert_eq!(
            artifacts.postflight_steps,
            vec![FlightStep::Run {
                command: FlightPath {
                    base: FlightPathBase::AppDir,
                    path: "OrbStack.app/Contents/MacOS/bin/orbctl".to_string(),
                },
                args: vec!["_internal".to_string(), "brew-postflight".to_string()],
                env: BTreeMap::new(),
                sudo: false,
                guards: Vec::new(),
            }]
        );
        Ok(())
    }

    #[test]
    fn parses_zoom_terminate_process_step() -> Result<()> {
        let mut cask = test_cask("zoom", "7.1.5.84650");
        cask.artifacts = vec![
            serde_json::json!({"uninstall": [{"pkgutil": "us.zoom.pkg.videomeeting"}]}),
            serde_json::json!({"pkg": ["zoomusInstallerFull.pkg"]}),
            serde_json::json!({
                "postflight_steps": [{
                    "steps": [{
                        "type": "terminate_process",
                        "name": "/Applications/zoom.us.app",
                        "match": "full",
                        "attempts": 3,
                        "notices": [
                            "The Zoom package postinstall script launches the Zoom app",
                            "Attempting to close zoom.us.app to avoid unwanted user intervention"
                        ],
                        "failure_message": "Unable to forcibly close zoom.us.app"
                    }]
                }]
            }),
        ];

        assert_eq!(
            cask_artifacts(&cask)?.postflight_steps,
            vec![FlightStep::TerminateProcess {
                name: "/Applications/zoom.us.app".to_string(),
                match_mode: ProcessMatch::Full,
                sudo: false,
                attempts: 3,
                must_succeed: false,
                notices: vec![
                    "The Zoom package postinstall script launches the Zoom app".to_string(),
                    "Attempting to close zoom.us.app to avoid unwanted user intervention"
                        .to_string(),
                ],
                failure_message: Some("Unable to forcibly close zoom.us.app".to_string()),
            }]
        );
        Ok(())
    }

    #[test]
    fn completed_flight_action_names_are_receipt_stable() -> Result<()> {
        let cask = test_cask("example", "1.0.0");
        let stage = tempfile::tempdir()?;
        let source = stage.path().join("obsolete");
        std::fs::write(&source, "remove me")?;
        let steps = vec![FlightStep::Remove {
            paths: vec![FlightPath {
                path: "obsolete".to_string(),
                base: FlightPathBase::StagedPath,
            }],
            recursive: false,
        }];
        let mut completed = Vec::new();

        execute_flight_steps_with_completion(
            &cask,
            &steps,
            stage.path(),
            Path::new("/Applications"),
            "postflight_steps",
            |index, step| {
                completed.push(format!("postflight_steps[{index}]:{}", step.kind()));
                Ok(())
            },
        )?;

        assert_eq!(completed, ["postflight_steps[0]:remove"]);
        assert!(!source.exists());
        Ok(())
    }

    #[test]
    fn terminate_process_has_explicit_completed_action_kind() {
        let step = FlightStep::TerminateProcess {
            name: "zoom.us.app".to_string(),
            match_mode: ProcessMatch::Name,
            sudo: false,
            attempts: 1,
            must_succeed: false,
            notices: Vec::new(),
            failure_message: None,
        };

        assert_eq!(step.kind(), "terminate_process");
    }

    #[test]
    fn terminate_process_defaults_match_homebrew() -> Result<()> {
        let cask = test_cask("example", "1.0.0");
        let step = parse_flight_step(
            &cask,
            "postflight_steps",
            &serde_json::json!({"type": "terminate_process", "name": "Example"}),
        )?;
        assert_eq!(
            step,
            FlightStep::TerminateProcess {
                name: "Example".to_string(),
                match_mode: ProcessMatch::Name,
                sudo: false,
                attempts: 1,
                must_succeed: false,
                notices: Vec::new(),
                failure_message: None,
            }
        );
        Ok(())
    }

    #[test]
    fn terminate_process_rejects_malformed_metadata() {
        let cask = test_cask("example", "1.0.0");
        let invalid = [
            serde_json::json!({"type": "terminate_process"}),
            serde_json::json!({"type": "terminate_process", "name": ""}),
            serde_json::json!({"type": "terminate_process", "name": "x", "match": "prefix"}),
            serde_json::json!({"type": "terminate_process", "name": "x", "attempts": 0}),
            serde_json::json!({"type": "terminate_process", "name": "x", "attempts": 1.5}),
            serde_json::json!({"type": "terminate_process", "name": "x", "sudo": "yes"}),
            serde_json::json!({"type": "terminate_process", "name": "x", "must_succeed": 1}),
            serde_json::json!({"type": "terminate_process", "name": "x", "notices": [1]}),
            serde_json::json!({"type": "terminate_process", "name": "x", "failure_message": 1}),
            serde_json::json!({"type": "terminate_process", "name": "x", "unknown": true}),
        ];
        for value in invalid {
            assert!(parse_flight_step(&cask, "postflight_steps", &value).is_err());
        }
    }

    #[test]
    fn terminate_process_retries_with_direct_argv_and_nonfatal_exhaustion() -> Result<()> {
        let step = FlightStep::TerminateProcess {
            name: "{{appdir}}/Example.app".to_string(),
            match_mode: ProcessMatch::Full,
            sudo: true,
            attempts: 3,
            must_succeed: false,
            notices: vec!["Closing {{version}}".to_string()],
            failure_message: Some("Unable to close {{version}}".to_string()),
        };
        let mut calls = Vec::new();
        let mut sleeps = Vec::new();
        execute_terminate_process(
            &step,
            Path::new("/tmp/stage"),
            Path::new("/Applications"),
            "1.2.3",
            |command, args, sudo| {
                calls.push((command.to_path_buf(), args.to_vec(), sudo));
                Err(eyre!("still running"))
            },
            |duration| sleeps.push(duration),
        )?;
        assert_eq!(calls.len(), 3);
        assert!(calls.iter().all(|(command, args, sudo)| {
            command == Path::new("/usr/bin/pkill")
                && args == &["-f", "/Applications/Example.app"]
                && *sudo
        }));
        assert_eq!(sleeps, vec![std::time::Duration::from_secs(1); 2]);
        Ok(())
    }

    #[test]
    fn terminate_process_name_mode_stops_after_success() -> Result<()> {
        let step = FlightStep::TerminateProcess {
            name: "Example".to_string(),
            match_mode: ProcessMatch::Name,
            sudo: false,
            attempts: 3,
            must_succeed: true,
            notices: Vec::new(),
            failure_message: None,
        };
        let mut attempts = 0;
        execute_terminate_process(
            &step,
            Path::new("/tmp/stage"),
            Path::new("/Applications"),
            "1.0.0",
            |command, args, sudo| {
                attempts += 1;
                assert_eq!(command, Path::new("/usr/bin/killall"));
                assert_eq!(args, &["Example"]);
                assert!(!sudo);
                if attempts == 1 {
                    Err(eyre!("retry"))
                } else {
                    Ok(())
                }
            },
            |_| {},
        )?;
        assert_eq!(attempts, 2);
        Ok(())
    }

    #[test]
    fn terminate_process_must_succeed_returns_final_error() {
        let step = FlightStep::TerminateProcess {
            name: "Example".to_string(),
            match_mode: ProcessMatch::Name,
            sudo: false,
            attempts: 1,
            must_succeed: true,
            notices: Vec::new(),
            failure_message: None,
        };
        let err = execute_terminate_process(
            &step,
            Path::new("/tmp/stage"),
            Path::new("/Applications"),
            "1.0.0",
            |_, _, _| Err(eyre!("still running")),
            |_| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("still running"));
    }

    #[test]
    fn structured_run_expands_paths_args_and_env() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let staged = tmp.path().join("stage");
        let appdir = tmp.path().join("Applications");
        file::create_dir_all(&staged)?;
        file::create_dir_all(&appdir)?;
        let result = staged.join("result");

        execute_flight_steps(
            &test_cask("example", "1.2.3"),
            &[FlightStep::Run {
                command: FlightPath {
                    base: FlightPathBase::Absolute,
                    path: "/bin/sh".to_string(),
                },
                args: vec![
                    "-c".to_string(),
                    "printf '%s' \"$MISE_TEST:$1:$2:$3\" > \"$4\"".to_string(),
                    "_".to_string(),
                    "{{appdir}}".to_string(),
                    "{{staged_path}}".to_string(),
                    "{{HOMEBREW_PREFIX}}".to_string(),
                    "{{staged_path}}/result".to_string(),
                ],
                env: BTreeMap::from([("MISE_TEST".to_string(), "version-{{version}}".to_string())]),
                sudo: false,
                guards: Vec::new(),
            }],
            &staged,
            &appdir,
            "postflight_steps",
        )?;

        assert_eq!(
            file::read_to_string(result)?,
            format!(
                "version-1.2.3:{}:{}:{}",
                appdir.display(),
                staged.display(),
                prefix.display()
            )
        );
        Ok(())
    }

    #[test]
    fn structured_flight_steps_move_and_remove_staged_paths() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let staged = tmp.path();
        let bundle_dir = staged.join("WezTerm-nightly");
        let app = bundle_dir.join("WezTerm.app");
        file::create_dir_all(&app)?;

        execute_flight_steps(
            &test_cask("wezterm@nightly", "latest"),
            &[
                FlightStep::Move {
                    source: FlightPath {
                        base: FlightPathBase::StagedPath,
                        path: "{WezTerm-*,wezterm-*}/WezTerm.app".to_string(),
                    },
                    target: FlightPath {
                        base: FlightPathBase::StagedPath,
                        path: ".".to_string(),
                    },
                    source_glob: true,
                },
                FlightStep::Remove {
                    paths: vec![
                        FlightPath {
                            base: FlightPathBase::StagedPath,
                            path: "WezTerm-*".to_string(),
                        },
                        FlightPath {
                            base: FlightPathBase::StagedPath,
                            path: "wezterm-*".to_string(),
                        },
                    ],
                    recursive: true,
                },
            ],
            staged,
            staged,
            "preflight_steps",
        )?;

        assert!(staged.join("WezTerm.app").is_dir());
        assert!(!bundle_dir.exists());
        Ok(())
    }

    #[test]
    fn rejects_unsupported_structured_flight_steps() {
        let mut cask = test_cask("battle-net", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({
                "preflight_steps": [{
                    "steps": [{
                        "type": "set_permissions",
                        "paths": [{"base": "staged_path", "path": "Battle.net-Setup.app"}],
                        "permissions": "a+x"
                    }]
                }]
            }),
            serde_json::json!({"app": "Battle.net.app"}),
        ];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("unsupported preflight_steps step type set_permissions"));
    }

    #[test]
    fn rejects_structured_flight_step_group_controls() {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({
                "preflight_steps": [{
                    "if": {"arch": "arm64"},
                    "steps": [{
                        "type": "remove",
                        "paths": [{"base": "staged_path", "path": "old"}]
                    }]
                }]
            }),
            serde_json::json!({"app": "Example.app"}),
        ];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("unsupported preflight_steps step group field if"));
    }

    #[test]
    fn rejects_structured_flight_step_controls() {
        let mut cask = test_cask("miniconda", "25.5.1-1");
        cask.artifacts = vec![
            serde_json::json!({
                "postflight_steps": [{
                    "steps": [{
                        "type": "remove",
                        "paths": [{"base": "staged_path", "path": "base/envs"}],
                        "recursive": true,
                        "guards": [{"condition": "if_exists", "path": "{{temp}}/miniconda-envs"}]
                    }]
                }]
            }),
            serde_json::json!({"pkg": ["Miniconda.pkg"]}),
            serde_json::json!({"uninstall": [{"pkgutil": "com.anaconda.pkg"}]}),
        ];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("unsupported postflight_steps remove step field guards"));
    }

    #[test]
    fn rejects_baseless_relative_run_command_paths() {
        let cask = test_cask("example", "1.0.0");
        for path in ["bin/tool", "../tool", "./tool"] {
            let value = serde_json::json!({"path": path});
            let err = parse_run_command(&cask, "preflight_steps", Some(&value))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("invalid preflight_steps run command path"),
                "{err}"
            );
        }
    }

    #[test]
    fn accepts_baseless_bare_and_absolute_run_commands() -> Result<()> {
        let cask = test_cask("example", "1.0.0");
        for path in ["xattr", "/usr/bin/xattr"] {
            let value = serde_json::json!({"path": path});
            assert_eq!(
                parse_run_command(&cask, "preflight_steps", Some(&value))?,
                FlightPath {
                    base: FlightPathBase::Absolute,
                    path: path.to_string(),
                }
            );
        }
        Ok(())
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
    fn cask_shim_supports_completion_stanzas_and_system_command() -> Result<()> {
        let Some(ruby) = file::which("ruby") else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let shim = tmp.path().join("cask_shim.rb");
        let cask = tmp.path().join("example.rb");
        file::write(&shim, CASK_SHIM_RB)?;
        crate::file::write(tmp.path().join("kubectl"), "kubectl")?;
        // Modeled on the docker-desktop cask: completion stanzas plus a
        // postflight that symlinks kubectl via system_command.
        file::write(
            &cask,
            r##"cask "example" do
  version "1.0.0"
  app "Example.app"
  binary "#{appdir}/Example.app/Contents/Resources/bin/example"
  bash_completion "#{appdir}/Example.app/Contents/Resources/etc/example.bash-completion"
  zsh_completion "#{appdir}/Example.app/Contents/Resources/etc/example.zsh-completion"
  fish_completion "#{appdir}/Example.app/Contents/Resources/etc/example.fish-completion"
  manpage "#{appdir}/Example.app/Contents/Resources/man/example.1"
  postflight do
    kubectl_target = staged_path/"kubectl-link"
    next if kubectl_target.exist?
    system_command "/bin/ln", args: ["-sfn", staged_path/"kubectl", kubectl_target],
                              sudo: false
    echoed = system_command "/bin/echo", args: ["-n", "hello"], print_stderr: false
    File.write staged_path/"result", echoed.stdout if echoed.success?
    # A no-args executable whose path contains spaces and shell
    # metacharacters must run via argv, not a shell command line.
    spaced = system_command staged_path/"my tool $HOME"
    File.write staged_path/"spaced-result", spaced.stdout
  end
end
"##,
        )?;
        let spaced_tool = tmp.path().join("my tool $HOME");
        crate::file::write(&spaced_tool, "#!/bin/sh\nprintf spaced-ok\n")?;
        file::make_executable(&spaced_tool)?;

        let output = run_cask_shim_hook(&ruby, &shim, &cask, tmp.path(), "1.0.0", "postflight")?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_link(tmp.path().join("kubectl-link"))?,
            tmp.path().join("kubectl")
        );
        assert_eq!(file::read_to_string(tmp.path().join("result"))?, "hello");
        assert_eq!(
            file::read_to_string(tmp.path().join("spaced-result"))?,
            "spaced-ok"
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cask_shim_system_command_reports_denied_sudo() -> Result<()> {
        let Some(ruby) = file::which("ruby") else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let shim = tmp.path().join("cask_shim.rb");
        let cask = tmp.path().join("example.rb");
        file::write(&shim, CASK_SHIM_RB)?;
        file::write(
            &cask,
            r#"cask "example" do
  version "1.0.0"
  preflight do
    system_command "/usr/bin/true", args: ["--flag"], sudo: true
  end
end
"#,
        )?;

        // MISE_BREW_CASK_SUDO is unset, which must behave as "deny".
        let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
        if nix::unistd::geteuid().is_root() {
            // root never needs to elevate, so the hook succeeds
            assert!(output.status.success());
            return Ok(());
        }
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("needs sudo"), "{stderr}");
        assert!(stderr.contains("sudo /usr/bin/true --flag"), "{stderr}");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cask_shim_system_command_reports_failed_commands() -> Result<()> {
        let Some(ruby) = file::which("ruby") else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let shim = tmp.path().join("cask_shim.rb");
        let cask = tmp.path().join("example.rb");
        file::write(&shim, CASK_SHIM_RB)?;
        file::write(
            &cask,
            r#"cask "example" do
  version "1.0.0"
  preflight do
    system_command "/usr/bin/false"
  end
end
"#,
        )?;

        let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("command failed (exit 1): /usr/bin/false"),
            "{stderr}"
        );
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
    fn detects_suffixless_dmg_archives() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("download");
        let mut contents = vec![0; 1024];
        contents[512..524].copy_from_slice(b"koly\0\0\0\x04\0\0\x02\0");
        std::fs::write(&archive, contents)?;

        assert!(is_dmg_archive(&archive, "raycast-1.104.24-download")?);
        Ok(())
    }

    #[test]
    fn rejects_malformed_suffixless_dmg_trailers() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("download");
        let mut contents = vec![0; 1024];
        contents[512..520].copy_from_slice(b"koly\0\0\0\x04");
        std::fs::write(&archive, contents)?;

        assert!(!is_dmg_archive(&archive, "raycast-1.104.24-download")?);
        Ok(())
    }

    #[test]
    fn does_not_sniff_named_archives_as_dmg() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("archive.zip");
        let mut contents = vec![0; 1024];
        contents[..4].copy_from_slice(b"PK\x03\x04");
        contents[512..524].copy_from_slice(b"koly\0\0\0\x04\0\0\x02\0");
        std::fs::write(&archive, contents)?;

        assert!(!is_dmg_archive(&archive, "archive.zip")?);
        assert_eq!(
            cask_extraction_format(&archive, "archive.zip")?,
            ExtractionFormat::Zip
        );
        Ok(())
    }

    #[test]
    fn leaves_suffixless_raw_binaries_raw() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("claude");
        let mut contents = vec![0; 1024];
        contents[..10].copy_from_slice(b"#!/bin/sh\n");
        std::fs::write(&archive, contents)?;

        assert!(!is_dmg_archive(&archive, "claude-1.0.0-claude")?);
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
    fn prefers_temp_caskroom_wrapper_over_extract_stage() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
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
            find_binary_source(&stage, &tmp_caskroom, &cask, &binary)?,
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
    fn parses_binary_artifacts_and_generated_completions() -> Result<()> {
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
                generated_completions: vec![GeneratedCompletionArtifact {
                    executable: "op".to_string(),
                    args: vec!["completion".to_string()],
                    base_name: None,
                    shell_parameter_format: None,
                    shells: vec![
                        CompletionShell::Bash,
                        CompletionShell::Zsh,
                        CompletionShell::Fish,
                    ],
                }],
                ..Default::default()
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_generated_completions_with_no_shells() {
        let value = serde_json::json!({
            "generate_completions_from_executable": ["op", {"shells": []}]
        });

        let err = parse_generated_completion_artifact(&value)
            .unwrap_err()
            .to_string();

        assert!(err.contains("requires at least one shell"));
    }

    #[test]
    fn parses_declared_completion_artifacts() -> Result<()> {
        let mut cask = test_cask("ghostty", "1.2.0");
        cask.artifacts = vec![
            serde_json::json!({"app": "Ghostty.app"}),
            serde_json::json!({
                "bash_completion": [
                    "$APPDIR/Ghostty.app/Contents/Resources/bash-completion/completions/ghostty.bash"
                ],
                "target": "$HOMEBREW_PREFIX/etc/bash_completion.d/ghostty"
            }),
            serde_json::json!({
                "fish_completion": [
                    "$APPDIR/Ghostty.app/Contents/Resources/fish/vendor_completions.d/ghostty.fish"
                ],
                "target": "$HOMEBREW_PREFIX/share/fish/vendor_completions.d/ghostty.fish"
            }),
            serde_json::json!({
                "zsh_completion": [
                    "$APPDIR/Ghostty.app/Contents/Resources/zsh/site-functions/_ghostty"
                ],
                "target": "$HOMEBREW_PREFIX/share/zsh/site-functions/_ghostty"
            }),
        ];

        assert_eq!(
            cask_artifacts(&cask)?.completions,
            vec![
                CompletionArtifact {
                    shell: CompletionShell::Bash,
                    source: "$APPDIR/Ghostty.app/Contents/Resources/bash-completion/completions/ghostty.bash"
                        .to_string(),
                    target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/ghostty".to_string()),
                },
                CompletionArtifact {
                    shell: CompletionShell::Fish,
                    source: "$APPDIR/Ghostty.app/Contents/Resources/fish/vendor_completions.d/ghostty.fish"
                        .to_string(),
                    target: Some(
                        "$HOMEBREW_PREFIX/share/fish/vendor_completions.d/ghostty.fish"
                            .to_string()
                    ),
                },
                CompletionArtifact {
                    shell: CompletionShell::Zsh,
                    source: "$APPDIR/Ghostty.app/Contents/Resources/zsh/site-functions/_ghostty"
                        .to_string(),
                    target: Some("$HOMEBREW_PREFIX/share/zsh/site-functions/_ghostty".to_string()),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn completion_target_paths_match_homebrew_names() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        assert_eq!(
            completion_target_path(CompletionShell::Bash, "ghostty.bash")?,
            tmp.path().join("etc/bash_completion.d/ghostty")
        );
        assert_eq!(
            completion_target_path(CompletionShell::Fish, "ghostty")?,
            tmp.path()
                .join("share/fish/vendor_completions.d/ghostty.fish")
        );
        assert_eq!(
            completion_target_path(CompletionShell::Zsh, "ghostty")?,
            tmp.path().join("share/zsh/site-functions/_ghostty")
        );
        assert_eq!(
            generated_completion_target_path(CompletionShell::Pwsh, "ghostty")?,
            tmp.path().join("share/pwsh/completions/_ghostty.ps1")
        );
        Ok(())
    }

    #[test]
    fn stages_and_links_declared_completion() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(stage.join("completions"))?;
        file::create_dir_all(&caskroom)?;
        crate::file::write(stage.join("completions/ghostty.bash"), "complete")?;
        let cask = test_cask("ghostty", "1.0.0");
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "completions/ghostty.bash".to_string(),
            target: None,
        };
        let artifacts = CaskArtifacts {
            completions: vec![completion.clone()],
            ..Default::default()
        };
        let target = completion.target_path()?;

        stage_completion(&stage, &caskroom, &cask, &[], &completion)?;
        link_completion(&cask, &artifacts, &caskroom, &target)?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("etc/bash_completion.d/ghostty"))?,
            "complete"
        );
        assert_eq!(
            std::fs::read_link(&target)?,
            caskroom.join("etc/bash_completion.d/ghostty")
        );
        assert_eq!(crate::file::read_to_string(target)?, "complete");
        Ok(())
    }

    #[test]
    fn declared_completion_source_maps_caskroom_path_to_temp_caskroom() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("tmp-caskroom");
        let cask = test_cask("foo", "1.0.0");
        file::create_dir_all(&stage)?;
        file::create_dir_all(caskroom.join("etc/bash_completion.d"))?;
        crate::file::write(caskroom.join("etc/bash_completion.d/foo"), "complete")?;
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "$HOMEBREW_PREFIX/Caskroom/foo/1.0.0/etc/bash_completion.d/foo".to_string(),
            target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
        };

        stage_completion(&stage, &caskroom, &cask, &[], &completion)?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("etc/bash_completion.d/foo"))?,
            "complete"
        );
        Ok(())
    }

    #[test]
    fn declared_completion_source_maps_caskroom_path_to_extract_stage() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("tmp-caskroom");
        let cask = test_cask("foo", "1.0.0");
        file::create_dir_all(stage.join("share/completions"))?;
        file::create_dir_all(&caskroom)?;
        crate::file::write(stage.join("share/completions/foo.bash"), "complete")?;
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "$HOMEBREW_PREFIX/Caskroom/foo/1.0.0/share/completions/foo.bash".to_string(),
            target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
        };

        stage_completion(&stage, &caskroom, &cask, &[], &completion)?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("etc/bash_completion.d/foo"))?,
            "complete"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn link_completion_adopts_homebrew_app_symlink() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("docker-desktop", "2.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let app = AppArtifact {
            source: "Docker.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Docker.app".to_string()),
        };
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "$APPDIR/Docker.app/Contents/Resources/etc/docker.bash-completion".to_string(),
            target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/docker".to_string()),
        };
        let artifacts = CaskArtifacts {
            apps: vec![app.clone()],
            completions: vec![completion.clone()],
            ..Default::default()
        };
        let relative = Path::new("etc/bash_completion.d/docker");
        let target = tmp.path().join(relative);
        let caskroom_completion = caskroom.join(relative);
        let app_completion = app_target_path(app.target_name())?
            .join("Contents/Resources/etc/docker.bash-completion");
        file::create_dir_all(caskroom_completion.parent().unwrap())?;
        file::create_dir_all(app_completion.parent().unwrap())?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&caskroom_completion, "new")?;
        crate::file::write(&app_completion, "homebrew")?;
        file::make_symlink(&app_completion, &target)?;

        link_completion(&cask, &artifacts, &caskroom, &target)?;

        assert_eq!(std::fs::read_link(&target)?, caskroom_completion);
        assert_eq!(crate::file::read_to_string(target)?, "new");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn link_completion_rejects_other_file_in_declared_app() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("foo", "2.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "$APPDIR/Example.app/Contents/Resources/etc/expected.bash".to_string(),
            target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
        };
        let artifacts = CaskArtifacts {
            apps: vec![app.clone()],
            completions: vec![completion.clone()],
            ..Default::default()
        };
        let target = completion.target_path()?;
        let app_resources = app_target_path(app.target_name())?.join("Contents/Resources/etc");
        let expected = app_resources.join("expected.bash");
        let other = app_resources.join("other.bash");
        file::create_dir_all(&app_resources)?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(expected, "expected")?;
        crate::file::write(&other, "other")?;
        file::make_symlink(&other, &target)?;

        let err = ensure_completion_target_replaceable(&cask, &artifacts, &target)
            .unwrap_err()
            .to_string();

        assert!(err.contains("is not owned by cask 'foo'"));
        assert_eq!(std::fs::read_link(&target)?, other);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn link_completion_rejects_target_owned_by_another_cask() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("foo", "2.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let other_caskroom = caskroom_version_dir("other", "1.0.0");
        let relative = Path::new("etc/bash_completion.d/foo");
        let target = tmp.path().join(relative);
        file::create_dir_all(caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(other_caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(caskroom.join(relative), "new")?;
        crate::file::write(other_caskroom.join(relative), "other")?;
        file::make_symlink(&other_caskroom.join(relative), &target)?;

        let err = link_completion(&cask, &CaskArtifacts::default(), &caskroom, &target)
            .unwrap_err()
            .to_string();

        assert!(err.contains("is not owned by cask 'foo'"));
        assert_eq!(std::fs::read_link(&target)?, other_caskroom.join(relative));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_generated_completion_output() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&caskroom)?;
        let executable = stage.join("op");
        crate::file::write(
            &executable,
            "#!/bin/sh\nprintf '%s|%s|%s' \"$1\" \"$2\" \"$SHELL\"\n",
        )?;
        let cask = test_cask("1password-cli", "2.34.1");
        let completion = GeneratedCompletionArtifact {
            executable: "op".to_string(),
            args: vec!["completion".to_string()],
            base_name: None,
            shell_parameter_format: None,
            shells: vec![CompletionShell::Bash],
        };

        stage_generated_completions(&stage, &caskroom, &cask, &[], &completion)?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("etc/bash_completion.d/op"))?,
            "completion|bash|bash"
        );
        Ok(())
    }

    #[test]
    fn generated_completion_executable_expands_appdir() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&caskroom)?;
        let app_executable = tmp.path().join("Applications/Foo.app/Contents/MacOS/foo");
        file::create_dir_all(app_executable.parent().unwrap())?;
        crate::file::write(&app_executable, "app cli")?;
        let cask = test_cask("foo", "1.0.0");
        let completion = GeneratedCompletionArtifact {
            executable: "$APPDIR/Foo.app/Contents/MacOS/foo".to_string(),
            args: vec![],
            base_name: None,
            shell_parameter_format: None,
            shells: vec![CompletionShell::Bash],
        };
        let apps = [AppArtifact {
            source: "Foo.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Foo.app".to_string()),
        }];

        assert_eq!(
            find_generated_completion_executable(&stage, &caskroom, &cask, &apps, &completion,)?,
            app_executable
        );
        Ok(())
    }

    #[test]
    fn appdir_artifact_source_matches_app_case_insensitively() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let prefix_appdir = tmp.path().join("Applications");
        let relative = "foo.app/Contents/MacOS/foo";
        file::create_dir_all(prefix_appdir.join(relative).parent().unwrap())?;
        crate::file::write(prefix_appdir.join(relative), "prefix")?;
        let apps = [
            AppArtifact {
                source: "Other.app".to_string(),
                target: None,
            },
            AppArtifact {
                source: "foo.app".to_string(),
                target: Some("$HOMEBREW_PREFIX/Applications/foo.app".to_string()),
            },
        ];

        assert_eq!(
            appdir_artifact_source("$APPDIR/Foo.app/Contents/MacOS/foo", &apps)?,
            Some(prefix_appdir.join(relative)),
        );
        Ok(())
    }

    #[test]
    fn generated_completion_executable_prefers_staged_prefix_binary() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(tmp.path().join("bin"))?;
        file::create_dir_all(caskroom.join("bin"))?;
        crate::file::write(tmp.path().join("bin/op"), "old")?;
        crate::file::write(caskroom.join("bin/op"), "new")?;
        let cask = test_cask("1password-cli", "2.34.1");
        let completion = GeneratedCompletionArtifact {
            executable: "$HOMEBREW_PREFIX/bin/op".to_string(),
            args: vec![],
            base_name: None,
            shell_parameter_format: None,
            shells: vec![CompletionShell::Bash],
        };

        assert_eq!(
            find_generated_completion_executable(&stage, &caskroom, &cask, &[], &completion,)?,
            caskroom.join("bin/op")
        );
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_generated_completion_bare_executable() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(stage.join("a"))?;
        file::create_dir_all(stage.join("b"))?;
        file::create_dir_all(&caskroom)?;
        crate::file::write(stage.join("a/tool"), "a")?;
        crate::file::write(stage.join("b/tool"), "b")?;
        let cask = test_cask("tool", "1.0.0");
        let completion = GeneratedCompletionArtifact {
            executable: "tool".to_string(),
            args: vec![],
            base_name: None,
            shell_parameter_format: None,
            shells: vec![CompletionShell::Bash],
        };

        let err = find_generated_completion_executable(&stage, &caskroom, &cask, &[], &completion)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("completion executable 'tool' is ambiguous")
        );
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_generated_completion_nested_executable() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(stage.join("a/bin"))?;
        file::create_dir_all(stage.join("b/bin"))?;
        file::create_dir_all(&caskroom)?;
        crate::file::write(stage.join("a/bin/tool"), "a")?;
        crate::file::write(stage.join("b/bin/tool"), "b")?;
        let cask = test_cask("tool", "1.0.0");
        let completion = GeneratedCompletionArtifact {
            executable: "bin/tool".to_string(),
            args: vec![],
            base_name: None,
            shell_parameter_format: None,
            shells: vec![CompletionShell::Bash],
        };

        let err = find_generated_completion_executable(&stage, &caskroom, &cask, &[], &completion)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("completion executable 'bin/tool' is ambiguous")
        );
        Ok(())
    }

    #[test]
    fn remove_obsolete_completions_removes_only_caskroom_symlinks() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("foo", "2.0.0");
        let old_caskroom = caskroom_version_dir(&cask.token, "1.0.0");
        let other_caskroom = caskroom_version_dir("other", "1.0.0");
        let relative = Path::new("etc/bash_completion.d/foo");
        let target = tmp.path().join(relative);
        let dangling_target = tmp.path().join("etc/bash_completion.d/dangling-foo");
        let other_target = tmp.path().join("etc/bash_completion.d/other-foo");
        let regular_target = tmp.path().join("etc/bash_completion.d/regular-foo");
        file::create_dir_all(old_caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(other_caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(old_caskroom.join(relative), "old")?;
        crate::file::write(other_caskroom.join(relative), "old")?;
        crate::file::write(&regular_target, "old")?;
        file::make_symlink(&old_caskroom.join(relative), &target)?;
        file::make_symlink(
            &old_caskroom.join("etc/bash_completion.d/dangling"),
            &dangling_target,
        )?;
        file::make_symlink(&other_caskroom.join(relative), &other_target)?;

        remove_obsolete_completions(
            &cask,
            &[
                target.clone(),
                dangling_target.clone(),
                other_target.clone(),
                regular_target.clone(),
            ],
            &[],
        )?;

        assert!(target.symlink_metadata().is_err());
        assert!(dangling_target.symlink_metadata().is_err());
        assert!(other_target.symlink_metadata().is_ok());
        assert!(regular_target.symlink_metadata().is_ok());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn remove_obsolete_completions_removes_dangling_symlinks_with_symlinked_prefix() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let real_prefix = tmp.path().join("homebrew-real");
        let prefix = tmp.path().join("homebrew");
        file::create_dir_all(&real_prefix)?;
        file::make_symlink(&real_prefix, &prefix)?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("foo", "2.0.0");
        let old_caskroom = caskroom_version_dir(&cask.token, "1.0.0");
        let relative = Path::new("etc/bash_completion.d/dangling");
        let target = prefix.join("etc/bash_completion.d/foo");
        file::create_dir_all(old_caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&old_caskroom.join(relative), &target)?;

        remove_obsolete_completions(&cask, std::slice::from_ref(&target), &[])?;

        assert!(target.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn completion_shell_parameter_formats_match_homebrew() {
        let (args, env) =
            completion_shell_parameter(Some("cobra"), CompletionShell::Zsh, Path::new("tool"));
        assert_eq!(args, vec!["completion".to_string(), "zsh".to_string()]);
        assert_eq!(env, Vec::<(String, String)>::new());

        let (args, env) =
            completion_shell_parameter(Some("click"), CompletionShell::Fish, Path::new("my-tool"));
        assert!(args.is_empty());
        assert_eq!(
            env,
            vec![("_MY_TOOL_COMPLETE".to_string(), "fish_source".to_string())]
        );

        let (args, env) =
            completion_shell_parameter(Some("clap"), CompletionShell::Bash, Path::new("tool"));
        assert!(args.is_empty());
        assert_eq!(env, vec![("COMPLETE".to_string(), "bash".to_string())]);

        let (args, env) = completion_shell_parameter(
            Some("--autocomplete=init:"),
            CompletionShell::Pwsh,
            Path::new("tool"),
        );
        assert_eq!(args, vec!["--autocomplete=init:powershell".to_string()]);
        assert_eq!(env, Vec::<(String, String)>::new());
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
    fn ignores_zap_pkgutil_ids_for_pkg_receipts() -> Result<()> {
        let mut cask = test_cask("google-japanese-ime", "3.33.6130");
        cask.artifacts = vec![
            serde_json::json!({"uninstall": [{"pkgutil": "com.google.pkg.GoogleJapaneseInput"}]}),
            serde_json::json!({"pkg": ["GoogleJapaneseInput.pkg"]}),
            serde_json::json!({"zap": [{"pkgutil": "com.google.pkg.Keystone"}]}),
        ];

        assert_eq!(
            cask_artifacts(&cask)?,
            CaskArtifacts {
                pkgs: vec![PkgArtifact {
                    source: "GoogleJapaneseInput.pkg".to_string()
                }],
                pkg_ids: vec!["com.google.pkg.GoogleJapaneseInput".to_string()],
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
    fn rejects_pkg_artifacts_with_only_zap_pkgutil_ids() {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({"pkg": ["Example.pkg"]}),
            serde_json::json!({"zap": [{"pkgutil": "com.example.cleanup"}]}),
        ];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("pkg artifacts require pkgutil ids in uninstall metadata"));
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
    fn parses_completion_artifacts_and_skips_manpage_artifacts() -> Result<()> {
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
        assert_eq!(artifacts.completions.len(), 3);
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
        let expected = font_dir().join("MyFont.ttf");
        assert_eq!(font_target_path(&font)?, expected);
        Ok(())
    }

    #[test]
    fn font_target_path_from_source_only() -> Result<()> {
        let font = FontArtifact {
            source: "FontAwesome.otf".to_string(),
            target: None,
        };
        let expected = font_dir().join("FontAwesome.otf");
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
        let expected = font_dir().join("JetBrainsMono.ttf");
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
        let expected = font_dir().join("TildeFont.ttf");
        assert_eq!(font_target_path(&font)?, expected);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_font_dir_uses_xdg_data_home() {
        assert_eq!(font_dir(), crate::env::XDG_DATA_HOME.join("fonts"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_font_target_preserves_xdg_subdirectories() -> Result<()> {
        let target = font_dir().join("nerd-fonts").join("NestedFont.ttf");
        let font = FontArtifact {
            source: "NestedFont.ttf".to_string(),
            target: Some(target.to_string_lossy().to_string()),
        };

        assert_eq!(font_filename(&font)?, "nerd-fonts/NestedFont.ttf");
        assert_eq!(font_target_path(&font)?, target);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_supports_font_only_casks() -> Result<()> {
        let mut cask = test_cask("font-test", "1.0.0");
        cask.artifacts = vec![serde_json::json!({"font": "TestFont.ttf"})];
        let artifacts = cask_artifacts(&cask)?;

        validate_platform_support(&cask, &artifacts)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_rejects_non_font_casks() -> Result<()> {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![serde_json::json!({"app": "Example.app"})];
        let artifacts = cask_artifacts(&cask)?;

        let err = validate_platform_support(&cask, &artifacts)
            .unwrap_err()
            .to_string();
        assert!(err.contains("only font-only casks"));
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

        assert_eq!(
            binary_target_path("op", Path::new("/Applications"))?,
            tmp.path().join("bin/op")
        );
        assert_eq!(
            binary_target_path("sbin/op", Path::new("/Applications"))?,
            tmp.path().join("sbin/op")
        );
        assert_eq!(
            binary_target_path("$HOMEBREW_PREFIX/bin/op", Path::new("/Applications"))?,
            tmp.path().join("bin/op")
        );
        Ok(())
    }

    #[test]
    fn binary_targets_must_stay_under_an_allowed_root() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        // Targets outside both the prefix and /usr/local are rejected.
        let err = binary_target_path("/opt/elsewhere/bin/op", Path::new("/Applications"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be under"));
        let err = binary_target_path("../op", Path::new("/Applications"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("must not contain '..'"));
        Ok(())
    }

    #[test]
    fn binary_targets_allow_absolute_usr_local() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        // Casks like docker-desktop hardcode absolute /usr/local targets; these
        // are honored even when the prefix is elsewhere (arm64 /opt/homebrew).
        assert_eq!(
            binary_target_path("/usr/local/bin/docker", Path::new("/Applications"))?,
            PathBuf::from("/usr/local/bin/docker")
        );
        assert_eq!(
            binary_target_path(
                "/usr/local/cli-plugins/docker-compose",
                Path::new("/Applications")
            )?,
            PathBuf::from("/usr/local/cli-plugins/docker-compose")
        );
        Ok(())
    }

    #[test]
    fn appdir_binary_targets_are_contained() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        assert_eq!(
            binary_target_path("$APPDIR/Surge Dashboard.app", Path::new("/Applications"))?,
            PathBuf::from("/Applications/Surge Dashboard.app")
        );
        let prefix_appdir = tmp.path().join("Applications");
        assert_eq!(
            binary_target_path("$APPDIR/Surge Dashboard.app", &prefix_appdir)?,
            prefix_appdir.join("Surge Dashboard.app")
        );
        for target in [
            "$APPDIR/../secret",
            "$APPDIR//absolute",
            "$APPDIR/Surge.app/../../secret",
            "prefix/$APPDIR/secret",
        ] {
            assert!(
                binary_target_path(target, Path::new("/Applications")).is_err(),
                "accepted {target}"
            );
        }
        Ok(())
    }

    #[test]
    fn caskroom_binary_paths_support_contained_appdir_targets() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/surge/1.0.0");
        let appdir = tmp.path().join("Applications");
        let binary = BinaryArtifact {
            source: "$APPDIR/Surge.app/Contents/Applications/Surge Dashboard.app".to_string(),
            target: Some("$APPDIR/Surge Dashboard.app".to_string()),
        };
        assert_eq!(
            caskroom_binary_path(&caskroom, &appdir, &binary)?,
            caskroom.join("Surge Dashboard.app")
        );
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
            caskroom_binary_path(&caskroom, Path::new("/Applications"), &binary)?,
            caskroom.join("sbin/op")
        );
        Ok(())
    }

    #[test]
    fn caskroom_binary_paths_strip_usr_local_root() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/docker-desktop/1.0.0");
        let binary = BinaryArtifact {
            source: "$APPDIR/Docker.app/Contents/Resources/bin/docker".to_string(),
            target: Some("/usr/local/bin/docker".to_string()),
        };

        assert_eq!(
            caskroom_binary_path(&caskroom, Path::new("/Applications"), &binary)?,
            caskroom.join("bin/docker")
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_uses_only_recorded_legacy_targets() -> Result<()> {
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
            schema_version: 0,
            version: cask.version.clone(),
            apps: vec![app_target_path(app.target_name())?],
            binaries: vec![],
            fonts: vec![],
            completions: vec![],
            pkg_ids: vec![],
            targets: Vec::new(),
            prune_safe: false,
            prune_blocker: None,
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
    fn installed_cask_version_rejects_unknown_receipt_schema() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("future", "1.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            schema_version: 4,
            version: cask.version.clone(),
            apps: Vec::new(),
            binaries: Vec::new(),
            fonts: Vec::new(),
            completions: Vec::new(),
            pkg_ids: Vec::new(),
            targets: Vec::new(),
            prune_safe: false,
            prune_blocker: None,
        };
        file::write(
            caskroom.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;

        assert_eq!(
            installed_cask_version(&cask, &CaskArtifacts::default())?,
            None
        );
        Ok(())
    }

    #[test]
    fn cask_prune_removes_only_receipt_owned_direct_artifacts() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let cask = test_cask("example", "1.0.0");
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let target = app_target_path(app.target_name())?;
        file::create_dir_all(&target)?;
        file::write(target.join("version"), "1.0.0")?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(version_dir.join("Example.app"))?;
        file::write(version_dir.join("Example.app/version"), "1.0.0")?;
        write_receipt(
            &version_dir,
            &cask,
            &CaskArtifacts {
                apps: vec![app],
                ..Default::default()
            },
        )?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);
        assert!(plan.skipped.is_empty());
        assert_eq!(apply_cask_prune_plan_in(&plan, true, &state_dir)?, 0);
        assert!(target.exists());

        assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 1);
        assert!(!target.exists());
        assert!(!caskroom_token_dir(&cask.token).exists());
        assert!(!cask_journal_pending_in(&state_dir, &cask.token));
        Ok(())
    }

    #[test]
    fn cask_prune_skips_configured_drifted_and_legacy_casks() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");

        let configured = test_cask("configured", "1.0.0");
        let configured_dir = caskroom_version_dir(&configured.token, &configured.version);
        let configured_target = tmp.path().join("Applications/Configured.app");
        file::create_dir_all(&configured_target)?;
        file::create_dir_all(configured_dir.join("Configured.app"))?;
        write_receipt(
            &configured_dir,
            &configured,
            &CaskArtifacts {
                apps: vec![AppArtifact {
                    source: "Configured.app".to_string(),
                    target: Some("$HOMEBREW_PREFIX/Applications/Configured.app".to_string()),
                }],
                ..Default::default()
            },
        )?;

        let drifted = test_cask("drifted", "1.0.0");
        let drifted_dir = caskroom_version_dir(&drifted.token, &drifted.version);
        let drifted_target = tmp.path().join("Applications/Drifted.app");
        file::create_dir_all(&drifted_target)?;
        file::create_dir_all(drifted_dir.join("Drifted.app"))?;
        write_receipt(
            &drifted_dir,
            &drifted,
            &CaskArtifacts {
                apps: vec![AppArtifact {
                    source: "Drifted.app".to_string(),
                    target: Some("$HOMEBREW_PREFIX/Applications/Drifted.app".to_string()),
                }],
                ..Default::default()
            },
        )?;
        file::write(drifted_target.join("changed"), "changed")?;

        let legacy = test_cask("legacy", "1.0.0");
        let legacy_dir = caskroom_version_dir(&legacy.token, &legacy.version);
        file::create_dir_all(&legacy_dir)?;
        file::write(
            legacy_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&CaskReceipt {
                schema_version: 2,
                version: legacy.version.clone(),
                apps: Vec::new(),
                binaries: Vec::new(),
                fonts: Vec::new(),
                completions: Vec::new(),
                pkg_ids: Vec::new(),
                targets: Vec::new(),
                prune_safe: false,
                prune_blocker: None,
            })?,
        )?;

        let plan =
            cask_prune_plan_from_tokens(&BTreeSet::from([configured.token.clone()]), &state_dir)?;
        assert!(plan.remove.is_empty());
        assert_eq!(
            plan.skipped
                .iter()
                .map(|skip| skip.token.as_str())
                .collect::<Vec<_>>(),
            vec!["drifted", "legacy"]
        );
        Ok(())
    }

    #[test]
    fn cask_prune_skips_shared_targets_and_pending_transactions() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");

        write_test_app_receipt(&test_cask("shared-a", "1.0.0"), "Shared.app")?;
        write_test_app_receipt(&test_cask("shared-b", "1.0.0"), "Shared.app")?;
        write_test_app_receipt(&test_cask("single", "1.0.0"), "Multi.app")?;
        write_test_app_receipt(&test_cask("multi", "1.0.0"), "Multi.app")?;
        write_test_app_receipt(&test_cask("multi", "2.0.0"), "Multi.app")?;
        write_test_app_receipt(&test_cask("pending", "1.0.0"), "Pending.app")?;
        let journal_dir = state_dir.join("brew-cask/pending");
        file::create_dir_all(&journal_dir)?;
        file::write(journal_dir.join("1.0.0.json"), "{}")?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert!(plan.remove.is_empty());
        assert_eq!(plan.skipped.len(), 5);
        for token in ["shared-a", "shared-b"] {
            assert!(plan.skipped.iter().any(|skip| {
                skip.token == token && skip.reason.contains("also claimed by another cask")
            }));
        }
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "pending"
                && skip
                    .reason
                    .contains("incomplete cask transaction is pending")
        }));
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "single" && skip.reason.contains("also claimed by another cask")
        }));
        assert!(
            plan.skipped.iter().any(|skip| {
                skip.token == "multi" && skip.reason.contains("expected exactly one")
            })
        );
        Ok(())
    }

    #[test]
    fn cask_prune_rechecks_shared_targets_before_removal() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let target = write_test_app_receipt(&test_cask("planned", "1.0.0"), "Shared.app")?;
        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);

        write_test_app_receipt(&test_cask("late-claim", "1.0.0"), "Shared.app")?;

        assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 0);
        assert!(target.exists());
        assert!(caskroom_token_dir("planned").exists());
        Ok(())
    }

    #[test]
    fn cask_prune_fails_closed_when_a_receipt_is_corrupt() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        write_test_app_receipt(&test_cask("clean", "1.0.0"), "Clean.app")?;
        let corrupt_dir = caskroom_version_dir("corrupt", "1.0.0");
        file::create_dir_all(&corrupt_dir)?;
        file::write(corrupt_dir.join(".mise-cask.toml"), "not = [valid")?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;

        assert!(plan.remove.is_empty());
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "corrupt" && skip.reason.contains("receipt could not be read")
        }));
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "clean" && skip.reason.contains("could not be indexed completely")
        }));
        Ok(())
    }

    #[test]
    fn cask_prune_receipt_rejects_pkg_and_lifecycle_casks() -> Result<()> {
        let mut cask = test_cask("example", "1.0.0");
        let direct = CaskArtifacts {
            apps: vec![AppArtifact {
                source: "Example.app".to_string(),
                target: None,
            }],
            ..Default::default()
        };
        assert_eq!(cask_prune_blocker(&cask, &direct), None);

        cask.artifacts = vec![serde_json::json!({"uninstall": [{"quit": "com.example"}]})];
        assert!(cask_prune_blocker(&cask, &direct).is_some());

        cask.artifacts.clear();
        let pkg = CaskArtifacts {
            pkgs: vec![PkgArtifact {
                source: "Example.pkg".to_string(),
            }],
            pkg_ids: vec!["com.example.pkg".to_string()],
            ..Default::default()
        };
        assert!(cask_prune_blocker(&cask, &pkg).is_some());

        let wrapper = CaskArtifacts {
            command_wrappers: vec![CommandWrapperArtifact {
                name: "example".to_string(),
                target: None,
                content: None,
                executable: Some("$APPDIR/Example.app/Contents/MacOS/example".to_string()),
                args: Vec::new(),
                env: BTreeMap::new(),
            }],
            ..Default::default()
        };
        assert_eq!(
            cask_prune_blocker(&cask, &wrapper).as_deref(),
            Some("command wrapper artifacts are not supported for pruning")
        );
        Ok(())
    }

    #[test]
    fn any_version_journal_marks_token_pending() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let journal_dir = tmp.path().join("brew-cask/example");
        file::create_dir_all(&journal_dir)?;
        file::write(journal_dir.join("0.9.0.json"), "{}")?;
        file::write(journal_dir.join("1.0.0.json"), "{}")?;

        assert!(cask_journal_pending_in(tmp.path(), "example"));
        assert!(!cask_journal_pending_in(tmp.path(), "other"));
        remove_cask_journals_in(tmp.path(), "example")?;
        assert!(!cask_journal_pending_in(tmp.path(), "example"));
        Ok(())
    }

    #[test]
    fn installed_cask_version_rejects_binary_state_without_receipt() -> Result<()> {
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

        let target = binary.target_path(Path::new("/Applications"))?;
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
            None
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_does_not_invent_wrapper_from_current_api() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("firefox", "153.0.1");
        let app = AppArtifact {
            source: "Firefox.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Firefox.app".to_string()),
        };
        let wrapper = CommandWrapperArtifact {
            name: "firefox".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/firefox".to_string()),
            content: None,
            executable: Some("$APPDIR/Firefox.app/Contents/MacOS/firefox".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
        };
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let app_target = app_target_path(app.target_name())?;
        file::create_dir_all(&app_target)?;
        let receipt = CaskReceipt {
            schema_version: 0,
            version: cask.version.clone(),
            apps: vec![app_target],
            binaries: Vec::new(),
            fonts: Vec::new(),
            completions: Vec::new(),
            pkg_ids: Vec::new(),
            targets: Vec::new(),
            prune_safe: false,
            prune_blocker: None,
        };
        file::write(
            caskroom.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;
        let artifacts = CaskArtifacts {
            apps: vec![app],
            command_wrappers: vec![wrapper.clone()],
            ..Default::default()
        };

        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some(cask.version.clone())
        );

        let target = wrapper.target_path()?;
        file::create_dir_all(target.parent().unwrap())?;
        file::write(target, "wrapper")?;
        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some(cask.version)
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

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
        link_binary(&caskroom, Path::new("/Applications"), &binary)?;

        let target = binary.target_path(Path::new("/Applications"))?;
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

        stage_binary(&stage, &caskroom, &cask, &[], &bin)?;
        stage_binary(&stage, &caskroom, &cask, &[], &sbin)?;
        link_binary(&caskroom, Path::new("/Applications"), &bin)?;
        link_binary(&caskroom, Path::new("/Applications"), &sbin)?;

        assert_eq!(
            crate::file::read_to_string(bin.target_path(Path::new("/Applications"))?)?,
            "bin"
        );
        assert_eq!(
            crate::file::read_to_string(sbin.target_path(Path::new("/Applications"))?)?,
            "sbin"
        );
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

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

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

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
        link_binary(&caskroom, Path::new("/Applications"), &binary)?;

        let staged = caskroom.join("bin/karabiner_cli");
        assert_eq!(std::fs::read_link(&staged)?, pkg_binary);
        let target = binary.target_path(Path::new("/Applications"))?;
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

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
        file::remove_file(&pkg_binary)?;
        let err = link_binary(&caskroom, Path::new("/Applications"), &binary)
            .unwrap_err()
            .to_string();

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

    #[cfg(target_os = "macos")]
    #[test]
    fn upgrades_app_with_protected_existing_contents() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("Docker.app");
        let protected_dir = target.join("Contents/Resources");
        file::create_dir_all(&protected_dir)?;
        crate::file::write(protected_dir.join("docker"), "old")?;
        let status = std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone deny delete_child"])
            .arg(&protected_dir)
            .status()?;
        assert!(status.success());

        let tmp_target = tmp.path().join("Docker.mise-tmp-test");
        file::create_dir_all(&tmp_target)?;
        crate::file::write(tmp_target.join("version"), "new")?;

        let result = swap_app(&target, &tmp_target);

        // Remove the ACL so tempfile can clean up even when the repro fails.
        let old_target = target.with_extension(format!(
            "mise-old-{}",
            crate::hash::hash_to_str(&target.display().to_string())
        ));
        if old_target.exists() {
            let status = std::process::Command::new("/bin/chmod")
                .arg("-RN")
                .arg(&old_target)
                .status()?;
            assert!(status.success());
        }

        result?;
        assert_eq!(crate::file::read_to_string(target.join("version"))?, "new");
        assert!(!old_target.exists());
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
    fn installed_cask_version_does_not_invent_pkg_ids_from_current_api() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("pkg-only", "1.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            schema_version: 0,
            version: cask.version.clone(),
            apps: vec![],
            binaries: vec![],
            fonts: vec![],
            completions: vec![],
            pkg_ids: vec![],
            targets: Vec::new(),
            prune_safe: false,
            prune_blocker: None,
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
            Some("1.0.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_rejects_app_state_without_receipt() -> Result<()> {
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
            None
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_rejects_completion_state_without_receipt() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("completion-only", "1.0.0");
        let completion = CompletionArtifact {
            shell: CompletionShell::Zsh,
            source: "ghostty".to_string(),
            target: None,
        };
        file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;
        let artifacts = CaskArtifacts {
            completions: vec![completion.clone()],
            ..Default::default()
        };

        assert_eq!(installed_cask_version(&cask, &artifacts)?, None);

        let target = completion.target_path()?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(target, "complete")?;
        assert_eq!(installed_cask_version(&cask, &artifacts)?, None);
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

        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            schema_version: 0,
            version: cask.version.clone(),
            apps: vec![app_target_path(
                "$HOMEBREW_PREFIX/Applications/Example.app",
            )?],
            binaries: Vec::new(),
            fonts: Vec::new(),
            completions: Vec::new(),
            pkg_ids: Vec::new(),
            targets: Vec::new(),
            prune_safe: false,
            prune_blocker: None,
        };
        file::write(
            caskroom.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;
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

    #[cfg(unix)]
    #[test]
    fn failed_activation_restores_caskroom_and_external_links() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("completion-only", "1.0.0");
        let destination = caskroom_version_dir(&cask.token, &cask.version);
        let staged = caskroom_tmp_dir(&cask);
        let relative = Path::new("etc/bash_completion.d/tool");
        file::create_dir_all(destination.join(relative).parent().unwrap())?;
        file::create_dir_all(staged.join(relative).parent().unwrap())?;
        crate::file::write(destination.join(relative), "previous")?;
        crate::file::write(staged.join(relative), "replacement")?;
        let target = tmp.path().join(relative);
        let new_target = tmp.path().join("bin/new-tool");
        file::create_dir_all(target.parent().unwrap())?;
        file::create_dir_all(new_target.parent().unwrap())?;
        file::make_symlink(&destination.join(relative), &target)?;
        let mut link_transaction =
            ArtifactLinkTransaction::begin(vec![target.clone(), new_target.clone()])?;

        let err = replace_caskroom(&cask, &staged, &destination, || {
            file::make_symlink(&destination.join(relative), &target)?;
            file::make_symlink(&destination.join("bin/new-tool"), &new_target)?;
            Err(eyre!("link failed"))
        })
        .unwrap_err();
        link_transaction.rollback()?;

        assert_eq!(err.to_string(), "link failed");
        assert_eq!(crate::file::read_to_string(&target)?, "previous");
        assert!(new_target.symlink_metadata().is_err());
        assert!(!caskroom_backup_dir(&cask).exists());
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

    #[test]
    fn fetch_git_clone_and_stage_clones_and_restructures_only_path() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;

        // Create a local git repo to clone from
        let repo = tmp.path().join("repo.git");
        std::fs::create_dir_all(repo.join("fonts").join("sample"))?;
        std::fs::write(
            repo.join("fonts").join("sample").join("font.ttf"),
            "initial content",
        )?;
        std::fs::write(
            repo.join("fonts").join("sample").join("font-bold.ttf"),
            "bold",
        )?;

        // Use --initial-branch to avoid depending on the configured default branch name.
        let repo_str = repo.to_string_lossy().to_string();
        let run = |args: &[&str]| -> Result<()> {
            let mut cmd = std::process::Command::new("git");
            if !cmd.args(args).status()?.success() {
                bail!("git {} failed", args.join(" "));
            }
            Ok(())
        };
        run(&["-C", &repo_str, "init", "-q", "--initial-branch=main"])?;
        run(&[
            "-C",
            &repo_str,
            "-c",
            "user.email=test@test",
            "-c",
            "user.name=test",
            "add",
            "-A",
        ])?;
        run(&[
            "-C",
            &repo_str,
            "-c",
            "user.email=test@test",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            "baseline",
        ])?;

        // Create a dedicated branch with different content to verify branch selection.
        run(&["-C", &repo_str, "checkout", "-q", "-b", "fonts-v2"])?;
        std::fs::write(
            repo.join("fonts").join("sample").join("font.ttf"),
            "branch content",
        )?;
        run(&[
            "-C",
            &repo_str,
            "-c",
            "user.email=test@test",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-a",
            "-m",
            "updated fonts",
        ])?;

        let url = format!("file://{}", repo.display());

        let cask = Cask {
            token: "font-test".to_string(),
            aliases: vec![],
            old_tokens: vec![],
            version: "latest".to_string(),
            url,
            url_specs: CaskUrlSpecs {
                branch: Some("fonts-v2".to_string()),
                only_path: Some("fonts/sample".to_string()),
            },
            sha256: Some("no_check".to_string()),
            artifacts: vec![],
            ruby_source_path: None,
            ruby_source_checksum: None,
            tap_git_head: None,
            raw_base: None,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let stage = rt.block_on(fetch_git_clone_and_stage(&cask, None))?;

        assert!(stage.join("font.ttf").is_file());
        assert!(stage.join("font-bold.ttf").is_file());
        // Verify the content from the dedicated branch, not the default branch.
        assert_eq!(
            std::fs::read_to_string(stage.join("font.ttf"))?,
            "branch content"
        );
        Ok(())
    }
}
