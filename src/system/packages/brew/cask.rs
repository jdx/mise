use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use eyre::{WrapErr, bail, eyre};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::api::RubySourceChecksum;
use super::prefix;
use super::receipt;
use super::source;
use crate::cmd::CmdLineRunner;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::git::{CloneOptions, Git};
use crate::hash;
use crate::http::{HTTP, HTTP_FETCH};
use crate::result::Result;
use crate::sandbox::SandboxConfig;
use crate::system::ManagerPackageOptions;
use crate::system::packages::{
    InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager,
};
use crate::system::sudo;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressIcon, SingleReport};

const API_BASE: &str = "https://formulae.brew.sh/api";
const HOMEBREW_CASK_RAW: &str = "https://raw.githubusercontent.com/Homebrew/homebrew-cask";
const CASK_SHIM_RB: &str = include_str!("cask_shim.rb");
/// where `app` artifacts are linked when [`APP_DIR_ENV`] is unset
const DEFAULT_APP_DIR: &str = "/Applications";
/// user-facing override for the `app` artifact destination, mirroring
/// `brew install --appdir`; see [`target_app_dir`] and
/// docs/bootstrap/packages/brew.md
const APP_DIR_ENV: &str = "MISE_BREW_CASK_OPT_APPDIR";

pub(crate) struct BrewCaskManager {}

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
    #[serde(default, deserialize_with = "deserialize_null_default")]
    depends_on: CaskDependencies,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    conflicts_with: CaskConflicts,
    #[serde(default)]
    ruby_source_path: Option<String>,
    #[serde(default)]
    ruby_source_checksum: Option<RubySourceChecksum>,
    #[serde(default)]
    tap_git_head: Option<String>,
    #[serde(default)]
    tap: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    auto_updates: bool,
    #[serde(skip)]
    raw_base: Option<String>,
    #[serde(skip)]
    definition_source: String,
    #[serde(skip)]
    loaded_from_internal_api: bool,
    #[serde(skip)]
    platform_policy: CaskPlatformPolicy,
    #[serde(skip)]
    resolved_formula_dependencies: Vec<super::resolve::ResolvedFormula>,
    #[serde(skip)]
    resolved_cask_dependencies: Vec<ResolvedCaskDependency>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum CaskPlatformPolicy {
    #[default]
    Unspecified,
    PublicSupported(BTreeSet<String>),
    Internal(CaskPlatformRequirements),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CaskPlatformRequirements {
    required_os: Option<super::tag::OperatingSystem>,
    arch: Option<super::tag::Architecture>,
    macos_min: Option<u32>,
    macos_max: Option<u32>,
    macos_exact: Option<BTreeSet<u32>>,
}

#[derive(Deserialize)]
struct InternalApiPayload {
    casks: BTreeMap<String, Value>,
    cask_tap_git_head: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CaskDependencies {
    #[serde(default)]
    formula: Vec<String>,
    #[serde(default)]
    cask: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CaskConflicts {
    #[serde(default)]
    cask: Vec<String>,
    #[serde(default)]
    formula: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedCaskDependency {
    token: String,
    version: String,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
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
    env: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PkgArtifact {
    source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallerArtifact {
    executable: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GenericArtifact {
    source: String,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FontArtifact {
    source: String,
    target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManpageArtifact {
    source: String,
    section: String,
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
    Copy {
        source: FlightPath,
        target: FlightPath,
        recursive: bool,
        overwrite: bool,
        source_glob: bool,
        guards: Vec<FlightGuard>,
    },
    Symlink {
        source: FlightPath,
        target: FlightPath,
        force: bool,
        uninstall: bool,
        source_glob: bool,
        sudo: FlightSudo,
        guards: Vec<FlightGuard>,
    },
    Run {
        command: FlightPath,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        sudo: bool,
        network_access: bool,
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
    SetOwnership {
        paths: Vec<FlightPath>,
        user: Option<String>,
        group: Option<String>,
        recursive: bool,
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
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlightSudo {
    Never,
    Always,
    IfNeeded,
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
    installers: Vec<InstallerArtifact>,
    generic: Vec<GenericArtifact>,
    fonts: Vec<FontArtifact>,
    manpages: Vec<ManpageArtifact>,
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
    /// Self-updating apps may legitimately drift from their downloaded payload.
    #[serde(default)]
    auto_updates: bool,
    /// App targets whose content fingerprint has special self-update/adoption semantics.
    /// Current native installs retain Homebrew's backlink; older receipts may not.
    #[serde(default)]
    metadata_only_apps: Vec<PathBuf>,
    #[serde(default)]
    apps: Vec<PathBuf>,
    #[serde(default)]
    binaries: Vec<PathBuf>,
    #[serde(default)]
    fonts: Vec<PathBuf>,
    #[serde(default)]
    manpages: Vec<PathBuf>,
    #[serde(default)]
    completions: Vec<PathBuf>,
    #[serde(default)]
    flight_directories: Vec<PathBuf>,
    #[serde(default)]
    generic: Vec<PathBuf>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uninstall: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct CaskTargetFingerprint {
    kind: CaskTargetKind,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaskTargetPlan {
    /// Complete native receipt inventory. This is descriptive state, not work.
    receipt_inventory_targets: Vec<PathBuf>,
    /// Public targets that this transaction will recreate before commit.
    artifact_activation_targets: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectiveCaskDirs {
    appdir: PathBuf,
    appimagedir: PathBuf,
    fontdir: PathBuf,
    vst_plugindir: PathBuf,
    vst3_plugindir: PathBuf,
    manpagedir: PathBuf,
}

impl EffectiveCaskDirs {
    fn current() -> Self {
        let home = crate::dirs::HOME.clone();
        #[cfg(target_os = "linux")]
        let xdg_data = std::env::var_os("HOMEBREW_XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        #[cfg(target_os = "linux")]
        return Self {
            appdir: home.join(".config/apps"),
            appimagedir: home.join("Applications"),
            fontdir: xdg_data.join("fonts"),
            vst_plugindir: home.join(".vst"),
            vst3_plugindir: home.join(".vst3"),
            manpagedir: prefix::prefix().join("share/man"),
        };
        #[cfg(not(target_os = "linux"))]
        Self {
            appdir: PathBuf::from("/Applications"),
            appimagedir: home.join("Applications"),
            fontdir: home.join("Library/Fonts"),
            vst_plugindir: home.join("Library/Audio/Plug-Ins/VST"),
            vst3_plugindir: home.join("Library/Audio/Plug-Ins/VST3"),
            manpagedir: prefix::prefix().join("share/man"),
        }
    }
}

fn configured_cask_dirs() -> Result<EffectiveCaskDirs> {
    let mut dirs = EffectiveCaskDirs::current();
    dirs.appdir = target_app_dir()?;
    Ok(dirs)
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CaskTargetKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CaskTransactionPhase {
    Prepared,
    Staging,
    RunningExternalAction { action: String },
    Activating,
    Activated,
    Committed,
    Pruning,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CaskRecoveryMode {
    DiscardStaging,
    RestoreFilesystem,
    Manual,
    FinishCommit,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct CaskTransactionJournal {
    schema_version: u8,
    token: String,
    version: String,
    phase: CaskTransactionPhase,
    recovery: CaskRecoveryMode,
    #[serde(default)]
    receipt_inventory_targets: Vec<PathBuf>,
    #[serde(default)]
    activation_targets: Vec<PathBuf>,
    #[serde(default)]
    predecessor_targets: Vec<CaskTargetRecord>,
    #[serde(default)]
    had_predecessor_metadata: bool,
    #[serde(default)]
    reopen_bundle_ids: Vec<String>,
    #[serde(default)]
    completed: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CaskTransactionJournalHeader {
    schema_version: u8,
}

#[derive(Debug, Deserialize)]
struct LegacyCaskTransactionJournal {
    token: String,
    version: String,
    #[serde(default)]
    completed: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CaskPruneCandidate {
    pub token: String,
    pub version: String,
    version_dir: PathBuf,
    receipt: CaskReceipt,
    homebrew_receipt: Option<receipt::CaskReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaskPruneSkip {
    pub token: String,
    pub reason: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CaskPrunePlan {
    pub remove: Vec<CaskPruneCandidate>,
    pub skipped: Vec<CaskPruneSkip>,
}

impl CaskPrunePlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.remove.is_empty()
    }
}

impl BrewCaskManager {
    pub(crate) fn new() -> Self {
        Self {}
    }

    async fn install_with_manager_options(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        manager_options: &ManagerPackageOptions,
    ) -> Result<()> {
        if let Some(p) = pkgs.iter().find(|p| p.version.is_some()) {
            bail!("brew casks are installed at their current version ('{p}')");
        }
        if opts.dry_run {
            prefix::bootstrap(true)?;
            for pkg in pkgs {
                self.install_one(pkg, opts, None, false, manager_options)
                    .await?;
            }
            return Ok(());
        }
        let mpr = MultiProgressReport::get();
        mpr.init_footer(false, "install", pkgs.len());
        for pkg in pkgs {
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew-cask:{}", pkg.name));
            match self
                .install_one(pkg, opts, Some(&*pr), false, manager_options)
                .await
            {
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

    async fn install_one(
        &self,
        req: &PackageRequest,
        opts: &InstallOpts,
        pr: Option<&dyn SingleReport>,
        upgrading: bool,
        manager_options: &ManagerPackageOptions,
    ) -> Result<String> {
        self.install_one_with_ancestors(req, opts, pr, upgrading, &BTreeSet::new(), manager_options)
            .await
            .map(|installed| installed.version)
    }

    async fn install_one_with_ancestors(
        &self,
        req: &PackageRequest,
        opts: &InstallOpts,
        pr: Option<&dyn SingleReport>,
        upgrading: bool,
        ancestors: &BTreeSet<String>,
        manager_options: &ManagerPackageOptions,
    ) -> Result<ResolvedCaskDependency> {
        let mut cask = fetch_cask(req).await?;
        if ancestors.contains(&cask.token) {
            bail!("brew-cask:{}: dependency cycle detected", cask.token);
        }
        let mut ancestors = ancestors.clone();
        ancestors.insert(cask.token.clone());
        let artifacts = cask_artifacts(&cask)?;
        recover_before_payload_validation(&mut cask, |cask| {
            if !opts.dry_run && cask_journal_pending_in(&crate::dirs::STATE, &cask.token) {
                prefix::bootstrap(false)?;
                let _lock = lock_cask(&cask.token)?;
                recover_flight_backups_for_cask(&cask.token)?;
                recover_cask_transaction(cask)?;
            }
            Ok(())
        })?;
        validate_platform_support(&cask, &artifacts)?;
        let classified = installed_cask_state(&cask, &artifacts)?;
        let installed = validate_legacy_cask(&cask, classified)?;
        let adopt_requested = manager_options.brew_cask_adopt(&req.name)
            || manager_options.brew_cask_adopt(&cask.token);
        let adopt = adopt_requested && matches!(&installed, InstalledCaskState::Absent);
        if let Some(version) = existing_install_noop(&installed, &cask, upgrading) {
            info!("brew-cask:{}: already installed", cask.token);
            return Ok(ResolvedCaskDependency {
                token: cask.token,
                version,
            });
        }
        if let InstalledCaskState::NeedsRepair {
            reason,
            replacement_safe: false,
            ..
        } = &installed
        {
            bail!("brew-cask:{}: needs repair: {reason}", cask.token);
        }
        if matches!(
            installed,
            InstalledCaskState::Installed(ref version) if version == &cask.version
        ) {
            info!("brew-cask:{}: already installed", cask.token);
            return Ok(ResolvedCaskDependency {
                token: cask.token,
                version: cask.version,
            });
        }
        for conflict in &cask.conflicts_with.cask {
            if !installed_versions(conflict).is_empty() {
                bail!(
                    "brew-cask:{}: conflicts with installed cask {}",
                    cask.token,
                    conflict
                );
            }
        }
        for conflict in &cask.conflicts_with.formula {
            if !super::pour::installed_versions(conflict).is_empty() {
                bail!(
                    "brew-cask:{}: conflicts with installed formula {}",
                    cask.token,
                    conflict
                );
            }
        }
        if !cask.depends_on.formula.is_empty() {
            let dependencies = cask
                .depends_on
                .formula
                .iter()
                .map(|name| PackageRequest {
                    name: name.clone(),
                    version: None,
                    tap_url: None,
                })
                .collect::<Vec<_>>();
            cask.resolved_formula_dependencies =
                super::resolve::resolve_closure_with_taps(&dependencies).await?;
            super::BrewManager::new()
                .install(&dependencies, opts)
                .await?;
        }
        for dependency in &cask.depends_on.cask {
            let request = PackageRequest {
                name: dependency.clone(),
                version: None,
                tap_url: None,
            };
            let installed = Box::pin(self.install_one_with_ancestors(
                &request,
                opts,
                None,
                upgrading,
                &ancestors,
                manager_options,
            ))
            .await?;
            cask.resolved_cask_dependencies.push(installed);
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
            for installer in &artifacts.installers {
                miseprintln!("run installer {}", installer.executable);
            }
            for artifact in &artifacts.generic {
                miseprintln!("install artifact {}", artifact.target);
            }
            for font in &artifacts.fonts {
                miseprintln!("install font {}", font.source);
            }
            for manpage in &artifacts.manpages {
                miseprintln!("install manpage {}", manpage.source);
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
            return Ok(ResolvedCaskDependency {
                token: cask.token,
                version: cask.version,
            });
        }
        let runtime_dependencies = cask_runtime_dependencies(&cask)?;
        prefix::bootstrap(false)?;
        let _caskroom_lock = lock_cask(&cask.token)?;
        recover_flight_backups_for_cask(&cask.token)?;
        match reconcile_legacy_cask_locked(&cask, installed_cask_state(&cask, &artifacts)?)? {
            InstalledCaskState::NeedsRepair {
                reason,
                replacement_safe: false,
                ..
            } => {
                bail!("brew-cask:{}: needs repair: {reason}", cask.token);
            }
            InstalledCaskState::NeedsRepair {
                replacement_safe: true,
                ..
            } => {}
            InstalledCaskState::Installed(version)
                if existing_install_noop(
                    &InstalledCaskState::Installed(version.clone()),
                    &cask,
                    upgrading,
                )
                .is_some() =>
            {
                return Ok(ResolvedCaskDependency {
                    token: cask.token,
                    version,
                });
            }
            InstalledCaskState::Installed(_) | InstalledCaskState::Absent => {}
            InstalledCaskState::LegacyMise(_) => unreachable!("legacy state was reconciled"),
        }
        let previous_binaries = previous_binary_targets(&cask)?;
        let previous_fonts = previous_font_targets(&cask)?;
        let previous_completions = previous_completion_targets(&cask)?;
        let previous_homebrew_receipt = homebrew_metadata_present(&cask.token)
            .then(|| receipt::read_cask_receipt(&caskroom_token_dir(&cask.token)))
            .transpose()?;
        if let Some(receipt) = &previous_homebrew_receipt {
            validate_homebrew_uninstall_artifacts(&cask.token, receipt)?;
        }
        let previous_homebrew_targets = previous_homebrew_receipt
            .as_ref()
            .map(|receipt| homebrew_receipt_targets(&cask.token, receipt))
            .transpose()?
            .unwrap_or_default();
        let predecessor = previous_homebrew_receipt
            .as_ref()
            .map(|receipt| -> Result<CaskPruneCandidate> {
                Ok(CaskPruneCandidate {
                    token: cask.token.clone(),
                    version: receipt.source.version.clone(),
                    version_dir: caskroom_version_dir(&cask.token, &receipt.source.version),
                    receipt: synthetic_homebrew_prune_receipt(&cask.token, receipt)?,
                    homebrew_receipt: Some(receipt.clone()),
                })
            })
            .transpose()?;
        let previous_flight_symlinks = previous_homebrew_targets
            .iter()
            .filter(|record| record.fingerprint.kind == CaskTargetKind::Symlink)
            .map(|record| record.path.clone())
            .collect::<BTreeSet<_>>();
        let previous_removable_flight_symlinks = previous_homebrew_targets
            .iter()
            .filter(|record| {
                record.fingerprint.kind == CaskTargetKind::Symlink
                    && record.uninstall.unwrap_or(true)
            })
            .map(|record| record.path.clone())
            .collect::<BTreeSet<_>>();
        let previous_flight_directories = previous_homebrew_targets
            .iter()
            .filter(|record| record.fingerprint.kind == CaskTargetKind::Directory)
            .map(|record| record.path.clone())
            .collect::<BTreeSet<_>>();
        let caskroom_token = caskroom_token_dir(&cask.token);
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let tmp_caskroom = caskroom_tmp_dir(&cask);
        let appdir = cask_appdir(&artifacts.apps)?;
        let mut target_plan = cask_target_plan(&cask, &artifacts)?;
        let adopted_app_targets = if adopt {
            artifacts
                .apps
                .iter()
                .map(|app| app_target_path(app.target_name()))
                .collect::<Result<BTreeSet<_>>>()?
                .into_iter()
                .filter(|target| target.symlink_metadata().is_ok())
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        target_plan
            .artifact_activation_targets
            .retain(|target| !adopted_app_targets.contains(target));
        let obsolete_previous_flight_symlinks = previous_removable_flight_symlinks
            .iter()
            .filter(|target| !target_plan.artifact_activation_targets.contains(target))
            .cloned()
            .collect::<Vec<_>>();
        validate_activation_target_claims(&target_plan, &previous_homebrew_targets)?;
        let mut journal = CaskTransactionJournal {
            schema_version: 2,
            token: cask.token.clone(),
            version: cask.version.clone(),
            phase: CaskTransactionPhase::Prepared,
            recovery: CaskRecoveryMode::DiscardStaging,
            receipt_inventory_targets: target_plan.receipt_inventory_targets.clone(),
            activation_targets: target_plan.artifact_activation_targets.clone(),
            predecessor_targets: previous_homebrew_targets.clone(),
            had_predecessor_metadata: previous_homebrew_receipt.is_some(),
            reopen_bundle_ids: Vec::new(),
            completed: Vec::new(),
        };
        let mut flight_targets = FlightTargetTransaction::default();
        flight_targets.receipt_caskroom = Some(caskroom.clone());
        flight_targets.previous_symlinks = previous_flight_symlinks.clone();
        flight_targets.previous_directories = previous_flight_directories;
        write_cask_journal(&journal)?;
        let stage = fetch_and_stage(&cask, pr).await?;
        validate_adoptable_apps(&stage, &artifacts.apps, &adopted_app_targets)?;
        let mut flight_activation_targets =
            planned_flight_activation_targets(&cask, &artifacts, &stage, &appdir)?;
        let flight_cleanup_targets = obsolete_previous_flight_symlinks
            .iter()
            .filter(|target| !flight_activation_targets.contains(target))
            .cloned()
            .collect::<Vec<_>>();
        flight_activation_targets.extend(flight_cleanup_targets.iter().cloned());
        reject_duplicate_cask_targets(&cask, &flight_activation_targets)?;
        let mut all_activation_targets = target_plan.artifact_activation_targets.clone();
        all_activation_targets.extend(flight_activation_targets.iter().cloned());
        reject_duplicate_cask_targets(&cask, &all_activation_targets)?;
        dedup_paths_preserving_order(&mut flight_activation_targets);
        validate_activation_target_claims(
            &CaskTargetPlan {
                receipt_inventory_targets: flight_activation_targets.clone(),
                artifact_activation_targets: flight_activation_targets.clone(),
            },
            &previous_homebrew_targets,
        )?;
        journal
            .receipt_inventory_targets
            .extend(flight_activation_targets.iter().cloned());
        journal
            .activation_targets
            .extend(flight_activation_targets.iter().cloned());
        dedup_paths_preserving_order(&mut journal.receipt_inventory_targets);
        dedup_paths_preserving_order(&mut journal.activation_targets);
        write_cask_journal(&journal)?;
        flight_targets.allowed_targets = Some(flight_activation_targets.into_iter().collect());
        file::remove_all(&tmp_caskroom)?;
        file::create_dir_all(&tmp_caskroom)?;
        let current_completions = completion_target_paths(&cask, &artifacts)?;
        if let Some(predecessor) = &predecessor
            && let Some(receipt) = &predecessor.homebrew_receipt
        {
            execute_predecessor_uninstall_recording(predecessor, receipt, &mut journal)?;
        }
        // Match Homebrew's artifact phases: preflight runs before app installation.
        // An appdir-based preflight command therefore sees only a previously installed app.
        if !artifacts.preflight_steps.is_empty() {
            set_cask_external_action(&mut journal, "preflight_steps")?;
        }
        execute_flight_steps_recording(
            &cask,
            &artifacts.preflight_steps,
            &stage,
            &appdir,
            "preflight_steps",
            &mut journal,
            &mut flight_targets,
        )
        .await?;
        if !artifacts.preflight_steps.is_empty() {
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
        }
        if has_lifecycle_hook(&cask, "preflight") {
            set_cask_external_action(&mut journal, "preflight_hook")?;
        }
        execute_lifecycle_hook(&cask, &stage, &appdir, "preflight", pr).await?;
        if has_lifecycle_hook(&cask, "preflight") {
            record_cask_action(&mut journal, "preflight_hook")?;
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
        }
        stage_primary_container(&stage, &tmp_caskroom)?;
        // Homebrew leaves artifacts from the installed version available to
        // preflight. Back them up only after preflight so guards and commands
        // can observe those links during an upgrade. A structured preflight
        // step that replaces one has already protected it transactionally.
        for target in &flight_cleanup_targets {
            flight_targets.protect(target)?;
        }
        if !artifacts.installers.is_empty() {
            set_cask_external_action(&mut journal, "installer_artifacts")?;
        }
        run_installers_before_durabilizing(
            &stage,
            &tmp_caskroom,
            &artifacts.installers,
            &mut flight_targets,
            |index| record_cask_action(&mut journal, &format!("installer[{index}]")),
        )?;
        if !artifacts.installers.is_empty() {
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
        }
        let metadata_only_apps = artifacts
            .apps
            .iter()
            .map(|app| app_target_path(app.target_name()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|target| cask.auto_updates || adopted_app_targets.contains(target))
            .collect::<BTreeSet<_>>();
        for (index, app) in artifacts.apps.iter().enumerate() {
            let target = app_target_path(app.target_name())?;
            if !adopted_app_targets.contains(&target) {
                install_app(&stage, &tmp_caskroom, app)?;
            }
            record_cask_action(&mut journal, &format!("app[{index}]"))?;
        }
        for (index, pkg) in artifacts.pkgs.iter().enumerate() {
            set_cask_external_action(&mut journal, &format!("pkg[{index}]"))?;
            install_pkg(&stage, pkg)?;
            record_cask_action(&mut journal, &format!("pkg[{index}]"))?;
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
        }
        for (index, font) in artifacts.fonts.iter().enumerate() {
            stage_font(&stage, &tmp_caskroom, font)?;
            record_cask_action(&mut journal, &format!("font[{index}]"))?;
        }
        for (index, manpage) in artifacts.manpages.iter().enumerate() {
            stage_manpage(&stage, &tmp_caskroom, &artifacts.apps, manpage)?;
            record_cask_action(&mut journal, &format!("manpage[{index}]"))?;
        }
        for (index, wrapper) in artifacts.command_wrappers.iter().enumerate() {
            stage_command_wrapper(&tmp_caskroom, &appdir, &cask, wrapper)?;
            record_cask_action(&mut journal, &format!("command_wrapper[{index}]"))?;
        }
        for (index, artifact) in artifacts.generic.iter().enumerate() {
            install_generic_artifact(&stage, &tmp_caskroom, artifact, &mut flight_targets)?;
            record_cask_action(&mut journal, &format!("artifact[{index}]"))?;
        }
        let app_activation_targets = artifacts
            .apps
            .iter()
            .map(|app| app_target_path(app.target_name()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|target| !adopted_app_targets.contains(target))
            .collect::<Vec<_>>();
        let mut app_link_transaction = ArtifactLinkTransaction::begin(
            app_activation_targets.clone(),
            &previous_homebrew_targets,
        )?;
        if !app_activation_targets.is_empty() {
            if journal.recovery == CaskRecoveryMode::DiscardStaging {
                journal.recovery = CaskRecoveryMode::RestoreFilesystem;
            }
            journal.phase = CaskTransactionPhase::Activating;
            write_cask_journal(&journal)?;
            for (index, app) in artifacts.apps.iter().enumerate() {
                let target = app_target_path(app.target_name())?;
                if !adopted_app_targets.contains(&target) {
                    activate_app(&tmp_caskroom, app, true)?;
                }
                record_cask_action(&mut journal, &format!("app_activation[{index}]"))?;
            }
        }
        if !artifacts.postflight_steps.is_empty() {
            set_cask_external_action(&mut journal, "postflight_steps")?;
        }
        execute_flight_steps_recording(
            &cask,
            &artifacts.postflight_steps,
            &tmp_caskroom,
            &appdir,
            "postflight_steps",
            &mut journal,
            &mut flight_targets,
        )
        .await?;
        file::remove_all(cask_step_home(&cask))?;
        if !artifacts.postflight_steps.is_empty() {
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
        }
        if has_lifecycle_hook(&cask, "postflight") {
            set_cask_external_action(&mut journal, "postflight_hook")?;
        }
        execute_lifecycle_hook(&cask, &tmp_caskroom, &appdir, "postflight", pr).await?;
        if has_lifecycle_hook(&cask, "postflight") {
            record_cask_action(&mut journal, "postflight_hook")?;
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
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
            set_cask_external_action(&mut journal, &format!("generated_completion[{index}]"))?;
            stage_generated_completions(&stage, &tmp_caskroom, &cask, &artifacts.apps, generated)?;
            record_cask_action(&mut journal, &format!("generated_completion[{index}]"))?;
            set_cask_phase(&mut journal, CaskTransactionPhase::Staging)?;
        }
        let current_binaries = binary_targets(&artifacts)?;
        let current_fonts = font_target_paths(&artifacts)?;
        journal
            .receipt_inventory_targets
            .extend(flight_targets.installed_targets().iter().cloned());
        dedup_paths_preserving_order(&mut journal.receipt_inventory_targets);
        journal.phase = CaskTransactionPhase::Activating;
        if journal.recovery == CaskRecoveryMode::DiscardStaging {
            journal.recovery = CaskRecoveryMode::RestoreFilesystem;
        }
        write_cask_journal(&journal)?;
        let remaining_activation_targets = target_plan
            .artifact_activation_targets
            .iter()
            .filter(|target| !app_activation_targets.contains(target))
            .cloned()
            .collect();
        let mut link_transaction = ArtifactLinkTransaction::begin(
            remaining_activation_targets,
            &previous_homebrew_targets,
        )?;
        let activation = replace_caskroom(&cask, &tmp_caskroom, &caskroom, || {
            retarget_transient_symlinks(&tmp_caskroom, &caskroom, &caskroom, &flight_targets)?;
            for font in &artifacts.fonts {
                link_font(&caskroom, font)?;
            }
            for binary in &artifacts.binaries {
                link_binary(&caskroom, &cask, &artifacts.apps, &appdir, binary)?;
            }
            for wrapper in &artifacts.command_wrappers {
                link_command_wrapper(&caskroom, wrapper)?;
            }
            for manpage in &artifacts.manpages {
                link_manpage(&caskroom, &artifacts.apps, manpage)?;
            }
            for target in &current_completions {
                link_completion(&cask, &artifacts, &caskroom, &stage, target)?;
            }
            write_homebrew_metadata(&caskroom, &cask, &runtime_dependencies, true)?;
            if requires_auxiliary_cask_receipt(
                cask.auto_updates,
                &metadata_only_apps,
                flight_targets.installed_targets(),
                &flight_targets.installed_directories,
            ) {
                write_auxiliary_cask_receipt_with_flight_targets(
                    &cask,
                    &artifacts,
                    &journal.receipt_inventory_targets,
                    &flight_targets.uninstall,
                    &flight_targets.installed_directories,
                    &metadata_only_apps,
                )?;
            }
            Ok(())
        });
        let mut caskroom_transaction = match activation {
            Ok(transaction) => transaction,
            Err(err) => {
                if let Err(rollback_err) = link_transaction.rollback() {
                    return Err(err.wrap_err(format!(
                        "failed to restore external cask artifacts: {rollback_err:#}"
                    )));
                }
                if let Err(rollback_err) = app_link_transaction.rollback() {
                    return Err(err.wrap_err(format!(
                        "failed to restore predecessor app after activation failed: {rollback_err:#}"
                    )));
                }
                return Err(err);
            }
        };
        if let Err(err) = record_cask_action(&mut journal, "activated")
            .and_then(|()| set_cask_phase(&mut journal, CaskTransactionPhase::Activated))
            .and_then(|()| {
                let auxiliary_receipt = read_auxiliary_cask_receipt(&cask.token, &cask.version)?;
                validate_installed_cask_topology_with_metadata(
                    &cask,
                    &artifacts,
                    &caskroom,
                    cask.auto_updates,
                    &metadata_only_apps,
                    auxiliary_receipt
                        .as_ref()
                        .map(|receipt| receipt.targets.as_slice()),
                )
            })
        {
            let caskroom_rollback = caskroom_transaction.rollback();
            let links_rollback = link_transaction.rollback();
            let apps_rollback = app_link_transaction.rollback();
            if let Err(rollback_err) = caskroom_rollback.and(links_rollback).and(apps_rollback) {
                return Err(err.wrap_err(format!(
                    "failed to restore predecessor after activation validation failed: {rollback_err:#}"
                )));
            }
            return Err(err);
        }
        reopen_predecessor_apps_recording(&cask, &mut journal)?;
        journal.phase = CaskTransactionPhase::Committed;
        journal.recovery = CaskRecoveryMode::FinishCommit;
        write_cask_journal(&journal)?;
        caskroom_transaction.commit()?;
        link_transaction.commit()?;
        app_link_transaction.commit()?;
        flight_targets.commit()?;
        remove_obsolete_binary_links(&cask, &previous_binaries, &current_binaries)?;
        remove_obsolete_completions(&cask, &previous_completions, &current_completions)?;
        remove_obsolete_fonts(&cask, &previous_fonts, &current_fonts)?;
        for target in previous_homebrew_targets {
            if !journal.receipt_inventory_targets.contains(&target.path)
                && target.uninstall.unwrap_or(true)
                && cask_target_record_matches(&target)?
            {
                remove_artifact_target_elevating(&target.path)?;
            }
        }
        remove_stale_versions(&caskroom_token, &cask.version)?;
        remove_cask_journals(&cask.token)?;
        file::remove_all(stage)?;
        Ok(ResolvedCaskDependency {
            token: cask.token,
            version: cask.version,
        })
    }
}

impl AppArtifact {
    fn target_name(&self) -> &str {
        self.target.as_deref().unwrap_or_else(|| {
            Path::new(&self.source)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&self.source)
        })
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
        binary_target_path(&self.target_name()?, &target_app_dir()?)
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
            statuses.push(installed_cask_status(req)?);
        }
        Ok(statuses)
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install_with_manager_options(pkgs, opts, &ManagerPackageOptions::None)
            .await
    }

    async fn install_with_options(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        manager_options: &ManagerPackageOptions,
    ) -> Result<()> {
        self.install_with_manager_options(pkgs, opts, manager_options)
            .await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.dry_run {
            for pkg in pkgs {
                self.install_one(pkg, opts, None, true, &ManagerPackageOptions::None)
                    .await?;
            }
            return Ok(());
        }
        let mpr = MultiProgressReport::get();
        mpr.init_footer(false, "upgrade", pkgs.len());
        for pkg in pkgs {
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew-cask:{}", pkg.name));
            match self
                .install_one(pkg, opts, Some(&*pr), true, &ManagerPackageOptions::None)
                .await
            {
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
}

fn installed_cask_status(req: &PackageRequest) -> Result<PackageStatus> {
    let token = match split_tap_name(&req.name) {
        Some((_, _, token)) => token,
        None => req.name.as_str(),
    };
    validate_cask_path_component("requested token", token)?;
    let installed = installed_cask_state_for_token(token)?;
    if let Some(state) = installed_native_unsupported_state(token) {
        return Ok(PackageStatus {
            request: req.clone(),
            state,
        });
    }
    let state = match installed {
        InstalledCaskState::Installed(version) => match &req.version {
            Some(requested) if version != *requested => {
                PackageState::VersionMismatch { installed: version }
            }
            _ if read_auxiliary_cask_receipt(token, &version)?
                .is_some_and(|receipt| receipt.auto_updates) =>
            {
                PackageState::InstalledAutoUpdates { version }
            }
            _ => PackageState::Installed { version },
        },
        InstalledCaskState::Absent => PackageState::Missing,
        InstalledCaskState::LegacyMise(legacy) => PackageState::NeedsRepair {
            installed: legacy.version.clone(),
            reason: offline_legacy_cask_reason(token, &legacy),
        },
        InstalledCaskState::NeedsRepair {
            installed, reason, ..
        } => PackageState::NeedsRepair {
            installed: installed.unwrap_or_default(),
            reason,
        },
    };
    Ok(PackageStatus {
        request: req.clone(),
        state,
    })
}

impl BrewCaskManager {
    pub(crate) fn unsupported_state_is_installed(req: &PackageRequest) -> bool {
        let token = split_tap_name(&req.name)
            .map(|(_, _, token)| token)
            .unwrap_or(&req.name);
        validate_cask_path_component("requested token", token).is_ok()
            && caskroom_token_dir(token)
                .join(".metadata")
                .symlink_metadata()
                .is_ok()
    }

    pub(crate) async fn platform_unavailable_reason(
        req: &PackageRequest,
    ) -> Result<Option<String>> {
        let cask = fetch_cask(req).await?;
        Ok(validate_catalog_platform_support(&cask)
            .err()
            .map(|error| error.to_string()))
    }
}

fn installed_native_unsupported_state(token: &str) -> Option<PackageState> {
    let token_dir = caskroom_token_dir(token);
    if token_dir.join(".metadata").symlink_metadata().is_err() {
        return None;
    }
    let receipt = receipt::read_cask_receipt(&token_dir).ok()?;
    let installed = cask_from_homebrew_receipt(token, &receipt);
    let artifacts = parse_cask_artifacts(&installed, false).ok()?;
    unsupported_package_state(&installed, &artifacts)
}

fn offline_legacy_cask_reason(token: &str, legacy: &CaskReceipt) -> String {
    for target in &legacy.targets {
        match cask_target_record_matches(target) {
            Ok(true) => {}
            Ok(false) => {
                return format!(
                    "brew-cask:{token}: legacy mise target fingerprint changed at {}; reinstall with either 'brew install --cask {token}' or mise apply after uninstalling",
                    target.path.display()
                );
            }
            Err(err) => {
                return format!(
                    "brew-cask:{token}: legacy mise target could not be verified at {} ({err}); reinstall with either 'brew install --cask {token}' or mise apply after uninstalling",
                    target.path.display()
                );
            }
        }
    }
    match pkg_ids_installed(&legacy.pkg_ids) {
        Ok(true) => {}
        Ok(false) => {
            return format!(
                "brew-cask:{token}: legacy mise package receipt is missing; reinstall with either 'brew install --cask {token}' or mise apply after uninstalling"
            );
        }
        Err(err) => {
            return format!(
                "brew-cask:{token}: legacy mise package receipt could not be verified ({err}); reinstall with either 'brew install --cask {token}' or mise apply after uninstalling"
            );
        }
    }
    format!(
        "brew-cask:{token}: legacy mise metadata requires catalog-backed validation and conversion during apply; status remained offline and made no changes"
    )
}

async fn fetch_cask(req: &PackageRequest) -> Result<Cask> {
    let name = &req.name;
    let (requested_token, official_api) = match split_tap_name(name) {
        Some(("homebrew", "cask", token)) => (token, true),
        Some((_, _, token)) => (token, false),
        None => (name.as_str(), true),
    };
    validate_cask_path_component("requested token", requested_token)?;
    if official_api {
        let mut cask = fetch_internal_cask(requested_token)
            .await
            .wrap_err_with(|| {
                format!("failed to fetch Homebrew internal cask '{requested_token}'")
            })?;
        cask.raw_base = Some(HOMEBREW_CASK_RAW.to_string());
        validate_cask_identity(&cask, requested_token, true)?;
        return Ok(cask);
    }
    if !official_api {
        bail!(
            "brew-cask: third-party cask '{name}' is unsupported because its tap metadata does not provide an independently authenticated artifact identity"
        );
    }
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
    let definition_json = HTTP_FETCH.get_text_cached(&url).await.wrap_err_with(|| {
        format!(
            "failed to fetch Homebrew cask '{name}' directly. \
                 Tapped casks must publish API metadata at api/cask/<token>.json"
        )
    })?;
    let mut cask = parse_public_cask_metadata(&definition_json, &super::tag::host_tag())
        .wrap_err_with(|| format!("failed to parse Homebrew cask '{name}' metadata"))?;
    cask.raw_base = raw_base;
    cask.definition_source = url;
    cask.loaded_from_internal_api = false;
    if cask.tap.is_none() {
        cask.tap = Some(match split_tap_name(name) {
            Some((owner, tap, _)) => format!("{owner}/{tap}"),
            None => "homebrew/cask".to_string(),
        });
    }
    validate_cask_identity(&cask, requested_token, official_api)?;
    Ok(cask)
}

async fn fetch_internal_cask(token: &str) -> Result<Cask> {
    let url = format!(
        "{API_BASE}/internal/packages.{}.jws.json",
        super::tag::host_tag()
    );
    let raw_envelope = HTTP_FETCH
        .get_text_cached(&url)
        .await
        .wrap_err("failed to fetch Homebrew internal packages API")?;
    let verified_payload = super::api::verify_internal_api_envelope(&raw_envelope)?;
    let payload: InternalApiPayload = serde_json::from_str(&verified_payload)?;
    let raw = payload
        .casks
        .get(token)
        .ok_or_else(|| eyre!("Homebrew internal API has no cask '{token}'"))?;
    let object = raw
        .as_object()
        .ok_or_else(|| eyre!("Homebrew internal API cask '{token}' is not an object"))?;
    let url_args = object
        .get("url_args")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("Homebrew internal API cask '{token}' has no URL"))?;
    let url_value = url_args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("Homebrew internal API cask '{token}' has invalid URL args"))?;
    let url_kwargs = object.get("url_kwargs").map(strip_internal_symbols);
    let url_specs = CaskUrlSpecs {
        branch: url_kwargs
            .as_ref()
            .and_then(|value| value.get("branch"))
            .and_then(Value::as_str)
            .map(str::to_string),
        only_path: url_kwargs
            .as_ref()
            .and_then(|value| value.get("only_path"))
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    let raw_artifacts = object
        .get("raw_artifacts")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("Homebrew internal API cask '{token}' has no artifacts"))?;
    let artifacts = raw_artifacts
        .iter()
        .map(internal_artifact_to_api)
        .collect::<Result<Vec<_>>>()?;
    let ruby_source_checksum = object
        .get("ruby_source_checksum")
        .map(strip_internal_symbols)
        .map(serde_json::from_value)
        .transpose()?;
    let (depends_on, platform_policy) = object
        .get("depends_on_args")
        .map(parse_internal_cask_dependencies)
        .transpose()?
        .unwrap_or_default();
    Ok(Cask {
        token: token.to_string(),
        aliases: Vec::new(),
        old_tokens: Vec::new(),
        version: object
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("Homebrew internal API cask '{token}' has no version"))?
            .to_string(),
        url: url_value.to_string(),
        url_specs,
        sha256: object
            .get("sha256")
            .and_then(Value::as_str)
            .map(strip_internal_symbol)
            .map(str::to_string),
        artifacts,
        ruby_source_path: object
            .get("ruby_source_path")
            .and_then(Value::as_str)
            .map(str::to_string),
        ruby_source_checksum,
        tap_git_head: Some(payload.cask_tap_git_head),
        tap: object
            .get("tap_string")
            .and_then(Value::as_str)
            .map(str::to_string),
        auto_updates: object
            .get("auto_updates")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        depends_on,
        conflicts_with: object
            .get("conflicts_with_args")
            .map(strip_internal_symbols)
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default(),
        raw_base: None,
        definition_source: url,
        loaded_from_internal_api: true,
        platform_policy,
        resolved_formula_dependencies: Vec::new(),
        resolved_cask_dependencies: Vec::new(),
    })
}

fn parse_public_cask_metadata(raw: &str, host_tag: &str) -> Result<Cask> {
    let mut value: Value = serde_json::from_str(raw)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| eyre!("Homebrew public cask metadata is not an object"))?;
    let variations = object.remove("variations").unwrap_or(Value::Null);
    if !variations.is_null() {
        let variations = variations
            .as_object()
            .ok_or_else(|| eyre!("Homebrew public cask variations are not an object"))?;
        for tag in variations.keys() {
            if !super::tag::is_known_platform_tag(tag) {
                bail!("Homebrew public cask has unknown platform variation '{tag}'");
            }
        }
        if let Some(variation) = variations.get(host_tag) {
            let variation = variation.as_object().ok_or_else(|| {
                eyre!("Homebrew public cask variation '{host_tag}' is not an object")
            })?;
            object.extend(variation.clone());
        }
    }
    let supported = object
        .remove("supported_platforms")
        .ok_or_else(|| eyre!("Homebrew public cask has no supported_platforms"))?;
    let supported = supported
        .as_array()
        .ok_or_else(|| eyre!("Homebrew public cask supported_platforms is not an array"))?
        .iter()
        .map(|tag| {
            let tag = tag
                .as_str()
                .ok_or_else(|| eyre!("Homebrew public cask platform is not a string"))?;
            if !super::tag::is_known_platform_tag(tag) {
                bail!("Homebrew public cask has unknown supported platform '{tag}'");
            }
            Ok(tag.to_string())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut cask: Cask = serde_json::from_value(value)?;
    cask.platform_policy = CaskPlatformPolicy::PublicSupported(supported);
    Ok(cask)
}

fn parse_internal_cask_dependencies(raw: &Value) -> Result<(CaskDependencies, CaskPlatformPolicy)> {
    let normalized = strip_internal_symbols(raw);
    let object = normalized
        .as_object()
        .ok_or_else(|| eyre!("Homebrew internal cask depends_on_args is not an object"))?;
    let mut requirements = CaskPlatformRequirements::default();
    for (key, value) in object {
        match key.as_str() {
            "formula" | "cask" => {}
            "arch" => {
                requirements.arch = Some(match value.as_str() {
                    Some("arm64") => super::tag::Architecture::Arm64,
                    Some("intel" | "x86_64") => super::tag::Architecture::Intel,
                    _ => bail!("Homebrew internal cask has unknown arch requirement {value}"),
                });
            }
            "linux" => {
                if value.as_str() != Some("any") {
                    bail!("Homebrew internal cask has unknown linux requirement {value}");
                }
                set_required_os(&mut requirements, super::tag::OperatingSystem::Linux)?;
            }
            "macos" => {
                set_required_os(&mut requirements, super::tag::OperatingSystem::Macos)?;
                match value {
                    Value::String(value) if value == "any" => {}
                    Value::String(value) => {
                        requirements.macos_min = Some(parse_macos_name(value)?);
                    }
                    Value::Array(values) => {
                        let exact = values
                            .iter()
                            .map(|value| {
                                value.as_str().ok_or_else(|| {
                                    eyre!("Homebrew internal cask macos value is not a string")
                                })
                            })
                            .map(|value| value.and_then(parse_macos_name))
                            .collect::<Result<BTreeSet<_>>>()?;
                        if exact.is_empty() {
                            bail!("Homebrew internal cask has an empty macos requirement");
                        }
                        requirements.macos_exact = Some(exact);
                    }
                    _ => bail!("Homebrew internal cask has unknown macos requirement {value}"),
                }
            }
            "maximum_macos" => {
                set_required_os(&mut requirements, super::tag::OperatingSystem::Macos)?;
                requirements.macos_max = Some(
                    value
                        .as_str()
                        .ok_or_else(|| {
                            eyre!("Homebrew internal cask maximum_macos is not a string")
                        })
                        .and_then(parse_macos_name)?,
                );
            }
            _ => bail!("Homebrew internal cask has unknown depends_on key '{key}'"),
        }
    }
    let dependencies = serde_json::from_value(normalized)?;
    Ok((dependencies, CaskPlatformPolicy::Internal(requirements)))
}

fn set_required_os(
    requirements: &mut CaskPlatformRequirements,
    os: super::tag::OperatingSystem,
) -> Result<()> {
    if requirements
        .required_os
        .is_some_and(|required| required != os)
    {
        bail!("Homebrew internal cask has conflicting operating-system requirements");
    }
    requirements.required_os = Some(os);
    Ok(())
}

fn parse_macos_name(value: &str) -> Result<u32> {
    super::tag::macos_major(value)
        .ok_or_else(|| eyre!("Homebrew internal cask has unknown macOS version '{value}'"))
}

fn strip_internal_symbols(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.strip_prefix(':').unwrap_or(key).to_string(),
                        strip_internal_symbols(value),
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(strip_internal_symbols).collect()),
        Value::String(value) => Value::String(strip_internal_symbol(value).to_string()),
        value => value.clone(),
    }
}

fn strip_internal_symbol(value: &str) -> &str {
    value.strip_prefix(':').unwrap_or(value)
}

fn internal_artifact_to_api(raw: &Value) -> Result<Value> {
    let parts = raw
        .as_array()
        .ok_or_else(|| eyre!("Homebrew internal API artifact is not an array"))?;
    let key = parts
        .first()
        .and_then(Value::as_str)
        .and_then(|key| key.strip_prefix(':'))
        .ok_or_else(|| eyre!("Homebrew internal API artifact has no DSL key"))?;
    let mut args = match parts.get(1).map(strip_internal_symbols) {
        None => Value::Null,
        Some(Value::Array(args)) => Value::Array(args),
        Some(Value::Object(kwargs)) => Value::Array(vec![Value::Object(kwargs)]),
        Some(value) => Value::Array(vec![value]),
    };
    if let Some(Value::Object(kwargs)) = parts.get(2).map(strip_internal_symbols)
        && !kwargs.is_empty()
    {
        match &mut args {
            Value::Array(args) => args.push(Value::Object(kwargs)),
            _ => unreachable!(),
        }
    }
    let mut artifact = serde_json::Map::new();
    artifact.insert(key.to_string(), args);
    Ok(Value::Object(artifact))
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
    if cask_payload_source_kind(&cask.url) == CaskPayloadSourceKind::Vcs {
        return fetch_git_clone_and_stage(cask, pr).await;
    }
    let (archive, effective_filename) = fetch_archive(cask, pr).await?;
    extract_archive_named(cask, &archive, &effective_filename, pr)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaskPayloadSourceKind {
    Archive,
    Vcs,
}

fn cask_payload_source_kind(url: &str) -> CaskPayloadSourceKind {
    let without_fragment = url.split_once('#').map_or(url, |(prefix, _)| prefix);
    let without_query = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(prefix, _)| prefix);
    let scheme = without_query
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    let vcs_scheme = scheme
        .as_deref()
        .is_some_and(|scheme| scheme == "git" || scheme == "ssh" || scheme.starts_with("git+"));
    let vcs_path = without_query
        .trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with(".git");
    if vcs_scheme || vcs_path {
        CaskPayloadSourceKind::Vcs
    } else {
        CaskPayloadSourceKind::Archive
    }
}

fn recover_before_payload_validation(
    cask: &mut Cask,
    recover: impl FnOnce(&Cask) -> Result<()>,
) -> Result<()> {
    recover(cask)?;
    validate_cask_payload_identity(cask)
}

fn validate_cask_payload_identity(cask: &mut Cask) -> Result<()> {
    if cask_payload_source_kind(&cask.url) == CaskPayloadSourceKind::Vcs {
        bail!(
            "brew-cask:{}: VCS payloads are unsupported because signed metadata does not bind an immutable repository revision",
            cask.token
        );
    }
    let Some(sha256) = cask.sha256.as_deref() else {
        bail!(
            "brew-cask:{}: signed metadata has no payload sha256",
            cask.token
        );
    };
    if sha256 == "no_check" {
        bail!(
            "brew-cask:{}: sha256 no_check is unsupported because it does not authenticate the downloaded payload",
            cask.token
        );
    }
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(
            "brew-cask:{}: signed payload sha256 is malformed",
            cask.token
        );
    }
    cask.sha256 = Some(sha256.to_ascii_lowercase());
    Ok(())
}

async fn fetch_git_clone_and_stage(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<PathBuf> {
    let extract_dir = cask_extract_dir(cask);
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

async fn fetch_archive(cask: &Cask, pr: Option<&dyn SingleReport>) -> Result<(PathBuf, String)> {
    let filename = archive_filename(&cask.url)
        .ok_or_else(|| eyre!("brew-cask:{}: URL has no file name", cask.token))?;
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("casks");
    file::create_dir_all(&cache_dir)?;
    let url_hash = &hash::hash_sha256_to_str(&cask.url)[..12];
    let archive = cache_dir.join(format!(
        "{}-{}-{url_hash}-{filename}",
        cask.token, cask.version
    ));
    let filename_record = archive.with_file_name(format!(
        ".{}.effective-filename",
        archive
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("brew-cask:{}: invalid cache filename", cask.token))?
    ));
    let cached_filename = read_effective_filename_record(&filename_record)?;
    if archive.exists() && cached_filename.is_none() {
        std::fs::remove_file(&archive)?;
    }
    let downloaded = !archive.exists();
    let effective_filename = if downloaded {
        let metadata = HTTP
            .download_file_with_metadata(&cask.url, &archive, pr)
            .await?;
        // Strip macOS quarantine so it doesn't propagate into extracted/copied artifacts.
        let _ = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&archive)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        metadata.effective_filename.unwrap_or(filename)
    } else {
        cached_filename.expect("cached archive filename was checked")
    };
    match cask.sha256.as_deref() {
        Some("no_check") => {}
        Some(sha256) => hash::ensure_checksum(&archive, sha256, pr, "sha256")?,
        None => bail!("brew-cask:{}: cask metadata has no sha256", cask.token),
    }
    if downloaded {
        file::write(&filename_record, &effective_filename)?;
    }
    Ok((archive, effective_filename))
}

fn read_effective_filename_record(path: &Path) -> Result<Option<String>> {
    let value = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let candidate = Path::new(&value);
    if value.is_empty()
        || value.chars().any(char::is_control)
        || candidate.components().count() != 1
        || candidate.file_name().and_then(|name| name.to_str()) != Some(value.as_str())
    {
        bail!("invalid cached cask response filename: {}", path.display());
    }
    Ok(Some(value))
}

fn extract_archive_named(
    cask: &Cask,
    archive: &Path,
    effective_filename: &str,
    pr: Option<&dyn SingleReport>,
) -> Result<PathBuf> {
    let extract_dir = cask_extract_dir(cask);
    file::remove_all(&extract_dir)?;
    file::create_dir_all(&extract_dir)?;
    let filename = effective_filename;
    if is_dmg_archive(archive, filename)? {
        file::un_dmg(archive, &extract_dir)?;
        discard_dmg_presentation_entries(&extract_dir)?;
    } else {
        let format = cask_extraction_format(archive, filename)?;
        if format == ExtractionFormat::Raw {
            // Preserve the original URL filename so artifact lookup can match
            // both raw binaries (for example `claude`) and installer packages.
            // Homebrew gives Content-Disposition precedence over the URL; an
            // older mise cache has no response metadata, so an uncompressed
            // XAR may also recover its sole declared pkg basename.
            let payload_filename = raw_payload_filename(cask, archive, filename)?;
            let dest = extract_dir.join(&payload_filename);
            file::copy(archive, &dest)?;
            if raw_payload_is_executable(cask, &payload_filename) {
                file::make_executable(&dest)?;
            }
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

fn raw_payload_filename(cask: &Cask, archive: &Path, effective_filename: &str) -> Result<String> {
    if Path::new(effective_filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
        || !file_starts_with(archive, b"xar!")?
    {
        return Ok(effective_filename.to_string());
    }
    let pkgs = cask
        .artifacts
        .iter()
        .map(parse_pkg_artifact)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let [pkg] = pkgs.as_slice() else {
        return Ok(effective_filename.to_string());
    };
    let path = Path::new(&pkg.source);
    if path.components().count() != 1
        || path.extension().and_then(|extension| extension.to_str()) != Some("pkg")
    {
        return Ok(effective_filename.to_string());
    }
    Ok(pkg.source.clone())
}

fn file_starts_with(path: &Path, magic: &[u8]) -> Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut actual = vec![0; magic.len()];
    Ok(file.read_exact(&mut actual).is_ok() && actual == magic)
}

fn raw_payload_is_executable(cask: &Cask, filename: &str) -> bool {
    cask.artifacts.iter().any(|artifact| {
        parse_binary_artifact(artifact).is_some_and(|binary| {
            !binary.source.starts_with("$APPDIR/")
                && Path::new(&binary.source)
                    .file_name()
                    .is_some_and(|source| source == filename)
        })
    })
}

fn cask_extract_dir(cask: &Cask) -> PathBuf {
    crate::dirs::CACHE
        .join("system-brew")
        .join("cask-extract")
        .join(format!("{}-{}", cask.token, cask.version))
}

/// Match Homebrew's DMG BOM filtering. Disk-image presentation metadata and
/// links back to canonical system directories are not part of the staged cask
/// payload and must never be copied into Caskroom.
fn discard_dmg_presentation_entries(stage: &Path) -> Result<()> {
    const METADATA: &[&str] = &[
        ".background",
        ".com.apple.timemachine.donotpresent",
        ".com.apple.timemachine.supported",
        ".DocumentRevisions-V100",
        ".DS_Store",
        ".fseventsd",
        ".MobileBackups",
        ".Spotlight-V100",
        ".TemporaryItems",
        ".Trashes",
        ".VolumeIcon.icns",
        ".HFS+ Private Directory Data\r",
        ".HFS+ Private Data\r",
    ];
    // DMGs conventionally expose only these top-level system-directory links.
    // This is the top-level subset of Homebrew 6.0.17's MacOS::SYSTEM_DIRS.
    const SYSTEM_DIRS: &[&str] = &[
        "/",
        "/Applications",
        "/Applications/Utilities",
        "/Incompatible Software",
        "/Library",
        "/Network",
        "/System",
        "/User Information",
        "/Users",
        "/Volumes",
        "/bin",
        "/boot",
        "/cores",
        "/dev",
        "/etc",
        "/home",
        "/libexec",
        "/lost+found",
        "/media",
        "/mnt",
        "/net",
        "/opt",
        "/private",
        "/proc",
        "/root",
        "/sbin",
        "/srv",
        "/tmp",
        "/usr",
        "/var",
    ];

    for name in METADATA {
        file::remove_all(stage.join(name))?;
    }
    let mut system_links = Vec::new();
    for entry in walkdir::WalkDir::new(stage).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_symlink() {
            continue;
        }
        let target = std::fs::read_link(entry.path())?;
        if target.is_absolute()
            && SYSTEM_DIRS
                .iter()
                .any(|system_dir| target == Path::new(system_dir))
        {
            system_links.push(entry.path().to_path_buf());
        }
    }
    for link in system_links {
        file::remove_file(link)?;
    }
    Ok(())
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
    let caskroom_app = caskroom_artifact_path(caskroom, &app.source, "app")?;
    file::remove_all(&caskroom_app)?;
    if let Some(parent) = caskroom_app.parent() {
        file::create_dir_all(parent)?;
    }
    copy_cask_artifact(&source, &caskroom_app)?;
    Ok(())
}

fn validate_adoptable_apps(
    stage: &Path,
    apps: &[AppArtifact],
    adopted_targets: &BTreeSet<PathBuf>,
) -> Result<()> {
    for app in apps {
        let target = app_target_path(app.target_name())?;
        if !adopted_targets.contains(&target) {
            continue;
        }
        let source = find_app(stage, &app.source)
            .ok_or_else(|| eyre!("brew-cask: app artifact '{}' was not found", app.source))?;
        if cask_target_fingerprint(&source)? != cask_target_fingerprint(&target)? {
            bail!(
                "brew-cask: cannot adopt '{}': existing artifact is not identical to the cask artifact",
                target.display()
            );
        }
    }
    Ok(())
}

/// Activate a staged app using Homebrew's moved-artifact topology: the public
/// app is the authoritative payload and the Caskroom entry is a backlink.
fn activate_app(caskroom: &Path, app: &AppArtifact, keep_backlink: bool) -> Result<()> {
    let caskroom_app = caskroom_artifact_path(caskroom, &app.source, "app")?;
    if !caskroom_app.is_dir() {
        bail!("brew-cask: app artifact '{}' was not staged", app.source);
    }
    let logical_target = app_target_path(app.target_name())?;
    let stable_target = path_with_resolved_existing_ancestor(&logical_target);
    let parent = ensure_trusted_appdir(
        stable_target
            .parent()
            .ok_or_else(|| eyre!("brew-cask: app target has no parent directory"))?,
    )?;
    let name = logical_target
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: app target has no filename"))?
        .to_owned();
    let name_hash = crate::hash::hash_to_str(&logical_target.display().to_string());
    let tmp_name = replace_bundle_extension(&name, &format!("mise-tmp-{name_hash}"));
    remove_all_at(&parent.fd, &tmp_name)?;
    copy_app_bundle_into(&caskroom_app, &parent.fd, &tmp_name)?;
    if exists_at(&parent.fd, &name)? {
        remove_all_at(&parent.fd, &tmp_name)?;
        bail!(
            "brew-cask: app target appeared during activation: {}",
            logical_target.display()
        );
    }
    nix::fcntl::renameat(
        &parent.fd,
        tmp_name.as_os_str(),
        &parent.fd,
        name.as_os_str(),
    )
    .wrap_err_with(|| format!("brew-cask: failed to activate {}", logical_target.display()))?;

    // Keep the staged copy until the public app is live. If backlink creation
    // fails, restore staging before removing the public copy so the outer
    // transaction can deterministically restore its predecessor.
    file::remove_all(&caskroom_app)?;
    if keep_backlink && let Err(err) = make_symlink_elevating(&logical_target, &caskroom_app) {
        let restore = copy_cask_artifact(&logical_target, &caskroom_app)
            .and_then(|()| remove_app_at(&parent, &name));
        if let Err(restore_err) = restore {
            return Err(err.wrap_err(format!(
                "failed to restore staged app after backlink creation failed: {restore_err:#}"
            )));
        }
        return Err(err);
    }
    // Remove macOS quarantine attribute so Gatekeeper doesn't block the app.
    let relative = Path::new(".").join(&name);
    let _ = run_in_trusted_dir(
        "xattr",
        &[
            std::ffi::OsStr::new("-r"),
            std::ffi::OsStr::new("-d"),
            std::ffi::OsStr::new("com.apple.quarantine"),
            relative.as_os_str(),
        ],
        &parent.fd,
    );
    Ok(())
}

/// Replace an app bundle's extension for a bare file name
/// (`Firefox.app` + `mise-tmp-ab12` -> `Firefox.mise-tmp-ab12`).
fn replace_bundle_extension(name: &std::ffi::OsStr, extension: &str) -> std::ffi::OsString {
    Path::new(name).with_extension(extension).into_os_string()
}

/// Whether `name` exists in `dir`, without following a final symlink.
#[cfg(unix)]
fn exists_at<Fd: std::os::fd::AsFd>(dir: Fd, name: &std::ffi::OsStr) -> Result<bool> {
    match nix::sys::stat::fstatat(dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(_) => Ok(true),
        Err(nix::errno::Errno::ENOENT) => Ok(false),
        Err(err) => Err(err.into()),
    }
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

/// Remove an app bundle inside `parent`, repairing protected contents before
/// escalating ownership. Removal and repair are bound to the descriptor.
#[cfg(unix)]
fn remove_app_at(parent: &TrustedOperationParent, name: &std::ffi::OsStr) -> Result<()> {
    match remove_all_at(&parent.fd, name) {
        Ok(()) => return Ok(()),
        Err(err) if !is_permission_denied(&err) => return Err(err),
        Err(_) => {}
    }

    repair_app_permissions_at(parent, name);
    match remove_all_at(&parent.fd, name) {
        Ok(()) => return Ok(()),
        Err(err) if !is_permission_denied(&err) => return Err(err),
        Err(_) => {}
    }

    let user = nix::unistd::User::from_uid(nix::unistd::geteuid())?
        .map(|user| user.name)
        .ok_or_else(|| eyre!("brew-cask: could not determine current user"))?;
    // Match Homebrew's final ownership-recovery step. Both the unprivileged and
    // the elevated attempt run with their working directory bound to the
    // verified appdir descriptor and address the bundle by a relative name, so
    // the recursive chown cannot be redirected outside the validated directory
    // by a replacement of any appdir component. sudo::run_in_dir applies the
    // same system_packages.sudo policy as sudo::run and refuses to prompt
    // without a TTY.
    //
    // `-h -P` keep the recursion inside the bundle: cask bundles legitimately
    // contain symlinks, and without these the walk would dereference one that
    // points outward and change ownership of the referent outside the verified
    // application directory.
    let relative = Path::new(".").join(name);
    let status = run_in_trusted_dir(
        "chown",
        &[
            std::ffi::OsStr::new("-R"),
            std::ffi::OsStr::new("-h"),
            std::ffi::OsStr::new("-P"),
            std::ffi::OsStr::new("--"),
            std::ffi::OsStr::new(&user),
            relative.as_os_str(),
        ],
        &parent.fd,
    );
    if !matches!(status, Ok(status) if status.success()) {
        sudo::run_in_dir(
            "chown",
            &[
                "-R".to_string(),
                "-h".to_string(),
                "-P".to_string(),
                "--".to_string(),
                user,
                relative.display().to_string(),
            ],
            &parent.fd,
        )?;
    }
    repair_app_permissions_at(parent, name);
    remove_all_at(&parent.fd, name)
}

/// Clear flags, restore owner permissions, and remove ACLs from an app bundle,
/// resolving it relative to the verified descriptor.
///
/// Every command is recursive, so `-P` is mandatory: cask bundles legitimately
/// contain symlinks, and without it a bundle symlink pointing outside the
/// application directory would have the flags or permissions of its referent
/// changed instead. macOS `chmod`/`chflags` reject `-R` together with `-h`, but
/// `-P` alone keeps the recursion from dereferencing symlink entries (verified:
/// an outward symlink's referent keeps its original mode and flags).
#[cfg(unix)]
fn repair_app_permissions_at(parent: &TrustedOperationParent, name: &std::ffi::OsStr) {
    let relative = Path::new(".").join(name);
    let run = |program: &str, args: &[&str]| {
        let mut argv: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
        argv.push(relative.as_os_str());
        let _ = run_in_trusted_dir(program, &argv, &parent.fd);
    };
    run("/usr/bin/chflags", &["-R", "-P", "--", "000"]);
    run("/bin/chmod", &["-R", "-P", "--", "u+rwx"]);
    run("/bin/chmod", &["-R", "-P", "-N"]);
}

/// Return whether an eyre chain originated from an I/O permission error.
fn is_permission_denied(err: &eyre::Report) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied)
            || cause
                .downcast_ref::<nix::errno::Errno>()
                .is_some_and(|err| {
                    matches!(err, nix::errno::Errno::EACCES | nix::errno::Errno::EPERM)
                })
    })
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
        &symlink_command_args(source, link),
    )
}

fn symlink_command_args(source: &Path, link: &Path) -> Vec<String> {
    let mut args = vec!["-s".into(), "-f".into()];
    if cfg!(target_os = "macos") {
        args.push("-h".into());
    } else {
        args.push("-n".into());
    }
    args.extend([
        "--".into(),
        source.display().to_string(),
        link.display().to_string(),
    ]);
    args
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
    } else if metadata.is_dir() {
        (
            remove_app(path),
            vec![
                "-r".into(),
                "-f".into(),
                "--".into(),
                path.display().to_string(),
            ],
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

#[cfg(test)]
fn remove_empty_directory_elevating(path: &Path) -> Result<()> {
    let Ok(metadata) = path.symlink_metadata() else {
        return Ok(());
    };
    if !metadata.is_dir() {
        return Ok(());
    }
    if path
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_some())
    {
        return Ok(());
    }
    with_sudo_fallback(
        file::remove_dir(path),
        "rmdir",
        &["--".into(), path.display().to_string()],
    )
}

/// Copy a cask artifact while preserving macOS metadata where it matters.
/// Other platforms use mise's symlink-preserving native copy implementation.
fn copy_cask_artifact(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        ditto(from, to)
    }
    #[cfg(not(target_os = "macos"))]
    {
        if from.is_dir() {
            file::create_dir_all(to)?;
            file::copy_dir_all_preserve_symlinks(from, to)
        } else {
            file::copy(from, to)
        }
    }
}

fn copy_staged_artifact_closure(stage: &Path, owned_stage: &Path, source: &Path) -> Result<()> {
    let stage = lexically_normalized_path(stage);
    let mut pending = vec![lexically_normalized_path(source)];
    let mut visited = BTreeSet::new();
    while let Some(source) = pending.pop() {
        let relative = staged_relative_path(&stage, &source).ok_or_else(|| {
            eyre!(
                "brew-cask: staged symlink target escaped extraction root: {}",
                source.display()
            )
        })?;
        if relative.components().next().is_some()
            && !source
                .parent()
                .is_some_and(|parent| path_starts_with_resolved_root(parent, &stage))
        {
            bail!(
                "brew-cask: staged symlink path escaped extraction root: {}",
                source.display()
            );
        }
        if !visited.insert(relative.to_path_buf()) {
            continue;
        }
        let destination = owned_stage.join(&relative);
        let metadata = source.symlink_metadata()?;
        if destination.symlink_metadata().is_err() {
            if let Some(parent) = destination.parent() {
                file::create_dir_all(parent)?;
            }
            if metadata.file_type().is_symlink() {
                file::make_symlink(&std::fs::read_link(&source)?, &destination)?;
            } else {
                copy_cask_artifact(&source, &destination)?;
            }
        } else if metadata.is_dir() && destination.is_dir() {
            copy_cask_artifact(&source, &destination)?;
        }

        let symlinks = WalkDir::new(&destination)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) if entry.file_type().is_symlink() => Some(Ok(entry.into_path())),
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for link in symlinks {
            let link_relative = link.strip_prefix(owned_stage)?;
            let original_link = stage.join(link_relative);
            let link_source = std::fs::read_link(&link)?;
            let original_target = lexically_normalized_path(&resolve_symlink_target(
                &original_link,
                link_source.clone(),
            ));
            let Some(target_relative) = staged_relative_path(&stage, &original_target) else {
                continue;
            };
            let owned_target = owned_stage.join(&target_relative);
            pending.push(original_target);
            if link_source.is_absolute() {
                file::remove_file(&link)?;
                file::make_symlink(&owned_target, &link)?;
            }
        }
    }
    Ok(())
}

fn durabilize_staged_symlink_targets(
    stage: &Path,
    temporary_caskroom: &Path,
    targets: &mut FlightTargetTransaction,
) -> Result<()> {
    let owned_stage = temporary_caskroom.join(".homebrew-staged");
    for target in targets.installed.clone() {
        if staged_relative_path(stage, &target).is_some() {
            continue;
        }
        let Ok(metadata) = target.symlink_metadata() else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let source = std::fs::read_link(&target)?;
        let staged_source = lexically_normalized_path(&resolve_symlink_target(&target, source));
        let Some(relative) = staged_relative_path(stage, &staged_source) else {
            continue;
        };
        let temporary_source = owned_stage.join(relative);
        copy_staged_artifact_closure(stage, &owned_stage, &staged_source)?;
        remove_artifact_target_elevating(&target)?;
        create_flight_symlink(&temporary_source, &target, FlightSudo::IfNeeded)?;
    }
    Ok(())
}

fn retarget_transient_symlinks(
    temporary_caskroom: &Path,
    installed_caskroom: &Path,
    final_caskroom: &Path,
    targets: &FlightTargetTransaction,
) -> Result<()> {
    for target in &targets.installed {
        let Ok(metadata) = target.symlink_metadata() else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let source = std::fs::read_link(target)?;
        let resolved = resolve_symlink_target(target, source);
        let Ok(relative) = resolved.strip_prefix(temporary_caskroom) else {
            continue;
        };
        remove_artifact_target_elevating(target)?;
        create_flight_symlink(&final_caskroom.join(relative), target, FlightSudo::IfNeeded)?;
    }
    let internal_symlinks = WalkDir::new(installed_caskroom)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_symlink() => Some(Ok(entry.into_path())),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for target in internal_symlinks {
        let source = std::fs::read_link(&target)?;
        // Relative links retain the same relationship when the whole caskroom is
        // renamed. Only absolute links embed the temporary caskroom path.
        if !source.is_absolute() {
            continue;
        }
        let Ok(relative) = source.strip_prefix(temporary_caskroom) else {
            continue;
        };
        file::remove_file(&target)?;
        create_flight_symlink(&final_caskroom.join(relative), &target, FlightSudo::Never)?;
    }
    Ok(())
}

fn install_generic_artifact(
    stage: &Path,
    temporary_caskroom: &Path,
    artifact: &GenericArtifact,
    targets: &mut FlightTargetTransaction,
) -> Result<()> {
    let source = find_artifact_matching(stage, &artifact.source, |_| true)
        .ok_or_else(|| eyre!("brew-cask: artifact '{}' was not found", artifact.source))?;
    if !path_starts_with_resolved_root(&source, stage) {
        bail!(
            "brew-cask: refusing generic artifact source outside the extraction root: {}",
            source.display()
        );
    }
    let target = generic_artifact_target_path(&artifact.target)?;
    // Not a lexical `strip_prefix`: the lookup resolves symlinks it had to
    // traverse, so a source reached that way can be contained by the stage
    // without sharing its literal prefix — as it is whenever `stage` itself
    // has a symlinked ancestor. `staged_relative_path` retries against the
    // resolved stage, matching the containment check above.
    let relative_source = staged_relative_path(stage, &source).ok_or_else(|| {
        eyre!(
            "brew-cask: generic artifact source is not contained by the extraction root: {}",
            source.display()
        )
    })?;
    let caskroom_source = temporary_caskroom.join(relative_source);
    if !path_starts_with_resolved_root(&caskroom_source, temporary_caskroom) {
        bail!(
            "brew-cask: refusing to stage generic artifact through a path outside the caskroom: {}",
            caskroom_source.display()
        );
    }
    #[cfg(not(unix))]
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    let elevated_target = targets.protect_generic(&target)?;
    copy_generic_artifact(&source, &target, elevated_target.as_deref())?;
    if let Some(parent) = caskroom_source.parent() {
        file::create_dir_all(parent)?;
    }
    file::make_symlink(&target, &caskroom_source)?;
    targets.record_installed(target);
    Ok(())
}

fn copy_generic_artifact(from: &Path, to: &Path, elevated_target: Option<&Path>) -> Result<()> {
    validate_generic_copy_target(to)?;
    #[cfg(unix)]
    {
        if let Some(target) = elevated_target {
            copy_generic_artifact_elevated(from, target)
        } else {
            copy_generic_artifact_unprivileged(from, to)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = elevated_target;
        copy_cask_artifact(from, to)
    }
}

#[cfg(unix)]
fn copy_generic_artifact_unprivileged(from: &Path, to: &Path) -> Result<()> {
    let parent = open_trusted_operation_parent(to, true, true)?;
    let name = to
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: generic artifact target has no filename"))?;
    let staging_name = format!(".mise-copy-{}", crate::rand::random_string(16));
    nix::sys::stat::mkdirat(
        &parent.fd,
        staging_name.as_str(),
        nix::sys::stat::Mode::S_IRWXU,
    )?;
    let flags = nix::fcntl::OFlag::O_RDONLY
        | nix::fcntl::OFlag::O_DIRECTORY
        | nix::fcntl::OFlag::O_NOFOLLOW;
    let staging_fd = nix::fcntl::openat(
        &parent.fd,
        staging_name.as_str(),
        flags,
        nix::sys::stat::Mode::empty(),
    )?;
    let staging_stat = nix::sys::stat::fstat(&staging_fd)?;
    if staging_stat.st_uid != nix::unistd::geteuid().as_raw() || staging_stat.st_mode & 0o077 != 0 {
        bail!("brew-cask: temporary artifact directory is not private");
    }
    let staging = TrustedOperationParent { fd: staging_fd };
    let temporary_name = std::ffi::OsStr::new("payload");
    match copy_cask_artifact_at(from, &staging.fd, temporary_name) {
        Ok(()) => {
            match nix::fcntl::renameat(&staging.fd, temporary_name, &parent.fd, name)
                .wrap_err_with(|| format!("failed to install {}", to.display()))
            {
                Ok(()) => {
                    remove_private_staging_dir(&parent, &staging, staging_name.as_ref())?;
                    Ok(())
                }
                Err(err) => {
                    remove_all_at(&staging.fd, temporary_name).wrap_err_with(|| {
                            format!(
                                "failed to clean up temporary generic artifact after rename failed: {err:#}"
                            )
                        })?;
                    remove_private_staging_dir(&parent, &staging, staging_name.as_ref())?;
                    Err(err)
                }
            }
        }
        Err(err) => {
            let _ = remove_all_at(&staging.fd, temporary_name);
            let _ = remove_private_staging_dir(&parent, &staging, staging_name.as_ref());
            Err(err)
        }
    }
}

#[cfg(unix)]
fn copy_generic_artifact_elevated(from: &Path, to: &Path) -> Result<()> {
    ensure_target_absent(to)?;
    let staging = tempfile::Builder::new()
        .prefix("mise-cask-copy-")
        .tempdir()?;
    let payload = staging.path().join("payload");
    copy_cask_artifact(from, &payload)?;
    sudo::run(
        "mv",
        &[
            "--".into(),
            payload.display().to_string(),
            to.display().to_string(),
        ],
        &[],
    )
}

#[cfg(unix)]
fn ensure_strict_elevated_target(target: &Path) -> Result<PathBuf> {
    let parent = target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: generic artifact target has no parent"))?;
    let mut existing = parent;
    loop {
        match existing.symlink_metadata() {
            Ok(_) => break,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                existing = existing.parent().ok_or_else(|| {
                    eyre!("brew-cask: generic artifact parent has no existing ancestor")
                })?;
            }
            Err(err) => return Err(err.into()),
        }
    }
    let trusted = open_trusted_operation_parent(&existing.join(".mise-validation"), false, false)?;
    let stable_existing = trusted.stable_path()?;
    validate_strict_elevated_ancestors(&stable_existing)?;
    let missing = parent.strip_prefix(existing)?;
    let stable_parent = stable_existing.join(missing);
    if existing != parent {
        sudo::run(
            "mkdir",
            &[
                "-p".into(),
                "--".into(),
                stable_parent.display().to_string(),
            ],
            &[],
        )?;
    }
    let stable_parent = std::fs::canonicalize(&stable_parent)?;
    validate_strict_elevated_ancestors(&stable_parent)?;
    Ok(stable_parent.join(
        target
            .file_name()
            .ok_or_else(|| eyre!("brew-cask: generic artifact target has no filename"))?,
    ))
}

#[cfg(unix)]
fn validate_strict_elevated_ancestors(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let stable_prefix = file::desymlink_path(&prefix::prefix());
    for directory in path.ancestors() {
        let metadata = directory.symlink_metadata()?;
        if !strict_elevated_directory_is_trusted(
            directory,
            &stable_prefix,
            metadata.uid(),
            metadata.mode(),
        ) {
            bail!(
                "brew-cask: refusing elevated operation through mutable directory {}",
                directory.display()
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
fn strict_elevated_directory_is_trusted(
    directory: &Path,
    stable_prefix: &Path,
    uid: u32,
    mode: u32,
) -> bool {
    uid == 0
        && mode & 0o002 == 0
        // Intel Homebrew conventionally uses root:admin 0775 for /usr/local.
        // Permit that exact prefix, but require every descendant and every
        // other ancestor used by the elevated operation to be non-writable.
        && (mode & 0o020 == 0 || directory == stable_prefix)
}

#[cfg(unix)]
fn ensure_target_absent(target: &Path) -> Result<()> {
    match target.symlink_metadata() {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => bail!(
            "brew-cask: refusing elevated operation because target appeared: {}",
            target.display()
        ),
        Err(err) => Err(err.into()),
    }
}

#[cfg(unix)]
fn copy_cask_artifact_at<Fd: std::os::fd::AsFd>(
    from: &Path,
    parent: Fd,
    name: &std::ffi::OsStr,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = from.symlink_metadata()?;
    let mode = nix::sys::stat::Mode::from_bits_truncate(
        metadata.permissions().mode() as nix::libc::mode_t
    );
    if metadata.file_type().is_symlink() {
        nix::unistd::symlinkat(&std::fs::read_link(from)?, parent, name)?;
    } else if metadata.is_dir() {
        nix::sys::stat::mkdirat(&parent, name, nix::sys::stat::Mode::S_IRWXU)?;
        let fd = nix::fcntl::openat(
            &parent,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW,
            nix::sys::stat::Mode::empty(),
        )?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_cask_artifact_at(&entry.path(), &fd, &entry.file_name())?;
        }
        nix::sys::stat::fchmod(&fd, mode)?;
    } else if metadata.is_file() {
        let destination = nix::fcntl::openat(
            parent,
            name,
            nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_NOFOLLOW,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )?;
        let mut source = std::fs::File::open(from)?;
        let mut destination = std::fs::File::from(destination);
        copy_file_contents(&mut source, &mut destination)?;
        nix::sys::stat::fchmod(&destination, mode)?;
    } else {
        bail!(
            "brew-cask: unsupported generic artifact type: {}",
            from.display()
        );
    }
    Ok(())
}

#[cfg(all(unix, target_os = "macos"))]
fn copy_file_contents(from: &mut std::fs::File, to: &mut std::fs::File) -> Result<()> {
    use std::os::fd::AsRawFd;

    // SAFETY: both descriptors remain open for the call and fcopyfile does not
    // retain them. A null state requests the default copyfile state.
    let result = unsafe {
        nix::libc::fcopyfile(
            from.as_raw_fd(),
            to.as_raw_fd(),
            std::ptr::null_mut(),
            nix::libc::COPYFILE_DATA | nix::libc::COPYFILE_METADATA,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn copy_file_contents(from: &mut std::fs::File, to: &mut std::fs::File) -> Result<()> {
    std::io::copy(from, to)?;
    Ok(())
}

#[cfg(unix)]
fn remove_all_at<Fd: std::os::fd::AsFd>(parent: Fd, name: &std::ffi::OsStr) -> Result<()> {
    let stat =
        match nix::sys::stat::fstatat(&parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(nix::errno::Errno::ENOENT) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
    let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
    if kind.contains(nix::sys::stat::SFlag::S_IFDIR) {
        let fd = nix::fcntl::openat(
            &parent,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW,
            nix::sys::stat::Mode::empty(),
        )?;
        let mut directory = nix::dir::Dir::from_fd(fd)?;
        let entries = directory
            .iter()
            .map(|entry| entry.map(|entry| entry.file_name().to_owned()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for entry in entries {
            if entry.as_bytes() != b"." && entry.as_bytes() != b".." {
                remove_all_at(&directory, std::ffi::OsStr::from_bytes(entry.to_bytes()))?;
            }
        }
        nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::RemoveDir)?;
    } else {
        nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::NoRemoveDir)?;
    }
    Ok(())
}

#[cfg(unix)]
fn remove_private_staging_dir(
    parent: &TrustedOperationParent,
    staging: &TrustedOperationParent,
    staging_name: &std::ffi::OsStr,
) -> Result<()> {
    let bound = nix::sys::stat::fstat(&staging.fd)?;
    let linked = nix::sys::stat::fstatat(
        &parent.fd,
        staging_name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )?;
    if bound.st_dev != linked.st_dev || bound.st_ino != linked.st_ino {
        bail!("brew-cask: temporary artifact directory was replaced");
    }
    nix::unistd::unlinkat(
        &parent.fd,
        staging_name,
        nix::unistd::UnlinkatFlags::RemoveDir,
    )?;
    Ok(())
}

fn validate_generic_copy_target(target: &Path) -> Result<()> {
    let prefix = prefix::prefix();
    if !target.starts_with(&prefix)
        || target.strip_prefix(&prefix)?.components().next().is_none()
        || !path_starts_with_resolved_root(target, &prefix)
    {
        bail!(
            "brew-cask: refusing generic artifact copy outside Homebrew prefix: {}",
            target.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
struct TrustedOperationParent {
    fd: std::os::fd::OwnedFd,
}

#[cfg(unix)]
impl TrustedOperationParent {
    fn path(&self) -> Result<PathBuf> {
        #[cfg(target_os = "linux")]
        return Ok(
            Path::new("/proc/self/fd").join(std::os::fd::AsRawFd::as_raw_fd(&self.fd).to_string())
        );
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let mut path = PathBuf::new();
            nix::fcntl::fcntl(&self.fd, nix::fcntl::FcntlArg::F_GETPATH(&mut path))?;
            Ok(path)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
        Ok(Path::new("/dev/fd").join(std::os::fd::AsRawFd::as_raw_fd(&self.fd).to_string()))
    }

    fn stable_path(&self) -> Result<PathBuf> {
        Ok(std::fs::canonicalize(self.path()?)?)
    }
}

#[cfg(unix)]
fn trusted_parent_is_writable(parent: &TrustedOperationParent) -> Result<bool> {
    let stat = nix::sys::stat::fstat(&parent.fd)?;
    let uid = nix::unistd::geteuid().as_raw();
    if uid == 0 {
        return Ok(true);
    }
    if stat.st_uid == uid {
        return Ok(stat.st_mode & 0o200 != 0);
    }
    let gid = nix::unistd::getegid().as_raw();
    if stat.st_gid == gid || current_process_groups()?.contains(&stat.st_gid) {
        return Ok(stat.st_mode & 0o020 != 0);
    }
    Ok(stat.st_mode & 0o002 != 0)
}

#[cfg(unix)]
fn current_process_groups() -> Result<Vec<nix::libc::gid_t>> {
    // SAFETY: A null buffer with size zero is the documented way to query the
    // number of supplementary groups and does not dereference the pointer.
    let count = unsafe { nix::libc::getgroups(0, std::ptr::null_mut()) };
    if count < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut groups = vec![0; count as usize];
    // SAFETY: `groups` has capacity for exactly `count` gid_t values;
    // getgroups returns an error instead of writing when that is insufficient.
    let count = unsafe { nix::libc::getgroups(count, groups.as_mut_ptr()) };
    if count < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    groups.truncate(count as usize);
    Ok(groups)
}

#[cfg(unix)]
fn sudo_invoking_id(effective_uid: u32, variable: &str) -> Option<u32> {
    sudo_invoking_id_from(effective_uid, crate::env::var(variable).ok().as_deref())
}

#[cfg(unix)]
fn sudo_invoking_id_from(effective_uid: u32, value: Option<&str>) -> Option<u32> {
    if effective_uid != 0 {
        return None;
    }
    value
        .and_then(|value| value.parse().ok())
        .filter(|id| *id != 0)
}

#[cfg(unix)]
fn open_trusted_operation_parent(
    target: &Path,
    allow_current_user: bool,
    create_missing: bool,
) -> Result<TrustedOperationParent> {
    let prefix = prefix::prefix();
    let parent = target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: generic artifact target has no parent"))?;
    let relative_parent = parent.strip_prefix(&prefix)?;
    let resolved_prefix = file::desymlink_path(&prefix);
    open_trusted_directory(
        &resolved_prefix,
        relative_parent,
        allow_current_user,
        create_missing,
    )
}

/// Open (and optionally create) `resolved_root`/`relative` one component at a
/// time with `O_NOFOLLOW`, verifying at every step that the component is a real
/// directory owned by root or the current user and not writable by an untrusted
/// party. `resolved_root` must already be a real (symlink-free) absolute path.
///
/// Walking component-by-component with `O_NOFOLLOW` binds the operation to the
/// exact directory chain that existed when the walk ran: a symlink cannot be
/// interposed for any component, and because every ancestor is verified as
/// non-untrusted-writable an unprivileged attacker cannot plant one to begin
/// with. Callers then mutate relative to the returned directory fd.
#[cfg(unix)]
fn open_trusted_directory(
    resolved_root: &Path,
    relative: &Path,
    allow_current_user: bool,
    create_missing: bool,
) -> Result<TrustedOperationParent> {
    use nix::fcntl::{OFlag, open, openat};
    use nix::sys::stat::{Mode, SFlag, fstat};

    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW;
    let mut fd = open(resolved_root, flags, Mode::empty()).wrap_err_with(|| {
        format!(
            "brew-cask: cannot open operation directory {}",
            resolved_root.display()
        )
    })?;
    let current_uid = nix::unistd::geteuid().as_raw();
    let current_gid = nix::unistd::getegid().as_raw();
    let current_groups = current_process_groups()?;
    let sudo_uid = sudo_invoking_id(current_uid, "SUDO_UID");
    let sudo_gid = sudo_invoking_id(current_uid, "SUDO_GID");
    let verify = |fd: &std::os::fd::OwnedFd, directory: &Path| -> Result<()> {
        let stat = fstat(fd)?;
        let owner_is_user = stat.st_uid == current_uid || Some(stat.st_uid) == sudo_uid;
        let trusted_owner = stat.st_uid == 0 || (allow_current_user && owner_is_user);
        let trusted_group = stat.st_gid == current_gid
            || Some(stat.st_gid) == sudo_gid
            || current_groups.contains(&stat.st_gid);
        let writable_by_untrusted = stat.st_mode & 0o002 != 0
            || (stat.st_mode & 0o020 != 0 && (!allow_current_user || !trusted_group));
        if !SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFDIR)
            || !trusted_owner
            || writable_by_untrusted
        {
            bail!(
                "brew-cask: refusing operation through untrusted directory {}",
                directory.display()
            );
        }
        Ok(())
    };
    let mut directory = resolved_root.to_path_buf();
    verify(&fd, &directory)?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("brew-cask: invalid generic artifact parent");
        };
        directory.push(name);
        fd = match openat(&fd, name, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(nix::errno::Errno::ENOENT) if create_missing => {
                match nix::sys::stat::mkdirat(
                    &fd,
                    name,
                    Mode::S_IRWXU | Mode::S_IRGRP | Mode::S_IXGRP | Mode::S_IROTH | Mode::S_IXOTH,
                ) {
                    Ok(()) | Err(nix::errno::Errno::EEXIST) => {}
                    Err(err) => {
                        return Err(err).wrap_err_with(|| {
                            format!(
                                "brew-cask: cannot create operation directory {}",
                                directory.display()
                            )
                        });
                    }
                }
                openat(&fd, name, flags, Mode::empty()).wrap_err_with(|| {
                    format!(
                        "brew-cask: cannot open operation directory {}",
                        directory.display()
                    )
                })?
            }
            Err(err) => {
                return Err(err).wrap_err_with(|| {
                    format!(
                        "brew-cask: cannot open operation directory {}",
                        directory.display()
                    )
                });
            }
        };
        verify(&fd, &directory)?;
    }
    Ok(TrustedOperationParent { fd })
}

/// Create the appdir (if missing) using symlink-safe, trust-verified directory
/// operations and return the bound descriptor.
///
/// Every component is opened with `openat`/`O_NOFOLLOW` and trust-checked, and
/// missing components are created with `mkdirat`, so a symlink interposed on any
/// component — including a not-yet-existing tail — is rejected rather than
/// followed.
///
/// The descriptor is returned rather than dropped so the caller can address the
/// app through it. Releasing it here would reintroduce the race: a replacement
/// of an accepted (user-owned) component between validation and mutation would
/// otherwise redirect the subsequent copy, rename, removal, and ownership
/// recovery, all of which resolve pathnames again.
#[cfg(unix)]
fn ensure_trusted_appdir(appdir: &Path) -> Result<TrustedOperationParent> {
    // Anchor the walk at `/` and verify every component from there. `/` is the
    // only directory that cannot be renamed or replaced, so it is the one safe
    // pathname to open; each component below it is opened with `openat` and
    // `O_NOFOLLOW` and trust-checked before descending.
    //
    // Scanning for the deepest existing ancestor and re-opening *that* by
    // pathname would reintroduce a race: between the scan and the open, a
    // same-uid process could replace the scanned directory with another
    // same-uid-owned real directory, which would pass the ownership checks and
    // become the retained descriptor. Starting from an unreplaceable root and
    // descending only through verified descriptors removes that window.
    // The path is walked exactly as given — it is NOT canonicalized here.
    // Canonicalizing would follow a symlink planted on the appdir or its tail,
    // which is precisely the substitution this walk must reject. Configured
    // overrides are already resolved by `target_app_dir`, so production paths
    // are symlink-free and the walk succeeds; a planted symlink hits
    // `O_NOFOLLOW` and fails.
    let relative = appdir.strip_prefix(Path::new("/")).map_err(|_| {
        eyre!(
            "brew-cask: app directory '{}' must be an absolute path",
            appdir.display()
        )
    })?;
    // `allow_current_user` is true because a per-user appdir such as
    // `~/Applications` is legitimately owned by the invoking user.
    open_trusted_directory(Path::new("/"), relative, true, true)
}

fn run_installer_artifact(
    stage: &Path,
    installer: &InstallerArtifact,
    copied_files: &BTreeSet<PathBuf>,
) -> Result<()> {
    let executable = stage.join(&installer.executable);
    if staged_relative_path(stage, &executable).is_none() {
        bail!(
            "brew-cask: refusing installer executable outside trusted installer roots: {}",
            executable.display()
        );
    }
    if !executable.is_file() {
        bail!(
            "brew-cask: installer executable '{}' was not found",
            installer.executable
        );
    }
    let executable = file::desymlink_path(&executable);
    if !executable.starts_with(file::desymlink_path(stage)) && !copied_files.contains(&executable) {
        bail!(
            "brew-cask: refusing installer executable outside trusted installer roots: {}",
            executable.display()
        );
    }
    file::make_executable(&executable)?;
    let prefix = prefix::prefix();
    let mut paths = vec![prefix.join("bin"), prefix.join("sbin")];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    let path = std::env::join_paths(paths)?;
    CmdLineRunner::new(executable)
        .env("PATH", path)
        .args(&installer.args)
        .raw(true)
        .execute()
}

fn run_installers_before_durabilizing(
    stage: &Path,
    temporary_caskroom: &Path,
    installers: &[InstallerArtifact],
    targets: &mut FlightTargetTransaction,
    mut completed: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    for (index, installer) in installers.iter().enumerate() {
        run_installer_artifact(stage, installer, targets.copied_files())?;
        completed(index)?;
    }
    durabilize_staged_symlink_targets(stage, temporary_caskroom, targets)
}

fn generic_artifact_target_path(target: &str) -> Result<PathBuf> {
    let prefix = prefix::prefix();
    let expanded = target.replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy());
    let target = PathBuf::from(expanded);
    if !target.is_absolute()
        || !target.starts_with(&prefix)
        || target.strip_prefix(&prefix)?.components().next().is_none()
        || target
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || !path_starts_with_resolved_root(&target, &prefix)
    {
        bail!(
            "brew-cask: generic artifact target '{}' must stay below {}",
            target.display(),
            prefix.display()
        );
    }
    Ok(target)
}

fn generic_artifact_targets(artifacts: &CaskArtifacts) -> Result<Vec<PathBuf>> {
    artifacts
        .generic
        .iter()
        .map(|artifact| generic_artifact_target_path(&artifact.target))
        .collect()
}

#[cfg(test)]
fn remove_obsolete_generic_artifacts(
    previous_targets: &[CaskTargetRecord],
    current_targets: &[PathBuf],
) -> Result<()> {
    let prefix = prefix::prefix();
    for record in previous_targets {
        if current_targets.contains(&record.path) || !cask_target_record_matches(record)? {
            continue;
        }
        if !path_starts_with_resolved_root(&record.path, &prefix) {
            bail!(
                "brew-cask: refusing to remove generic artifact outside {}: {}",
                prefix.display(),
                record.path.display()
            );
        }
        if let Err(err) = remove_trusted_generic_target(&record.path) {
            warn!(
                "brew-cask: leaving obsolete generic artifact {} because its parent directories are mutable: {err:#}",
                record.path.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
fn remove_trusted_generic_target(target: &Path) -> Result<()> {
    let expected_parent = resolved_parent(target)?;
    match remove_trusted_generic_target_from(target, &expected_parent) {
        Ok(()) => Ok(()),
        Err(err) if is_permission_denied(&err) => {
            #[cfg(unix)]
            let operation_target = ensure_strict_elevated_target(target)?;
            #[cfg(not(unix))]
            let operation_target = target.to_path_buf();
            remove_artifact_target_elevating(&operation_target).wrap_err_with(|| {
                format!(
                    "failed to remove generic artifact after unprivileged removal failed: {err:#}"
                )
            })
        }
        Err(err) => Err(err),
    }
}

fn remove_trusted_generic_target_from(target: &Path, expected_parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let parent = open_trusted_operation_parent(target, true, false)?;
        validate_trusted_operation_parent(&parent, expected_parent)?;
        let name = target
            .file_name()
            .ok_or_else(|| eyre!("brew-cask: generic artifact target has no filename"))?;
        remove_all_at(&parent.fd, name)
    }
    #[cfg(not(unix))]
    {
        let _ = expected_parent;
        file::remove_all(target)
    }
}

#[cfg(unix)]
fn validate_trusted_operation_parent(
    parent: &TrustedOperationParent,
    expected_parent: &Path,
) -> Result<()> {
    let actual_parent = std::fs::canonicalize(parent.path()?)?;
    if actual_parent != expected_parent {
        bail!(
            "brew-cask: refusing operation through a changed generic artifact parent: {}",
            expected_parent.display()
        );
    }
    Ok(())
}

fn rename_trusted_generic_target(from: &Path, to: &Path, expected_parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if from.parent() != to.parent() {
            bail!("brew-cask: generic artifact backup changed directories");
        }
        let parent = open_trusted_operation_parent(from, true, false)?;
        validate_trusted_operation_parent(&parent, expected_parent)?;
        let from_name = from
            .file_name()
            .ok_or_else(|| eyre!("brew-cask: generic artifact source has no filename"))?;
        let to_name = to
            .file_name()
            .ok_or_else(|| eyre!("brew-cask: generic artifact target has no filename"))?;
        nix::fcntl::renameat(&parent.fd, from_name, &parent.fd, to_name)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = expected_parent;
        file::rename(from, to)
    }
}

fn stage_primary_container(stage: &Path, caskroom: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        ditto(stage, caskroom)?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        file::copy_dir_all_preserve_symlinks(stage, caskroom)?;
    }
    // Homebrew's ZIP strategy removes AppleDouble resource-fork metadata
    // after extraction. Preserve the primary container, not extractor junk.
    file::remove_all(caskroom.join("__MACOSX"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
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

/// Run a helper with its working directory bound to `dir` via `fchdir`, so
/// relative arguments resolve from that exact directory inode.
///
/// Passing a pathname to a subprocess would let it re-resolve every component,
/// which a same-uid replacement can redirect. `fchdir` in the child pins
/// resolution to the descriptor mise already verified, so relative names cannot
/// escape the validated application directory.
#[cfg(unix)]
fn run_in_trusted_dir<Fd: std::os::fd::AsFd>(
    program: &str,
    args: &[&std::ffi::OsStr],
    dir: Fd,
) -> Result<std::process::ExitStatus> {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    let raw = dir.as_fd().as_raw_fd();
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    // SAFETY: `fchdir` is async-signal-safe and only alters the child's working
    // directory. `raw` stays open in the parent for the duration of the spawn,
    // and CLOEXEC (if set) only takes effect at exec, after pre_exec runs.
    unsafe {
        cmd.pre_exec(move || {
            if nix::libc::fchdir(raw) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.status()
        .wrap_err_with(|| format!("failed to run {program}"))
}

/// Copy the bundle `from` into a freshly created directory `name` inside `dir`,
/// without ever letting the copy resolve `name` through an untrusted pathname.
///
/// On macOS, `name` is created with `mkdirat`, opened with `O_NOFOLLOW`, and
/// `ditto` writes through the bound directory descriptor. Other Unix platforms
/// recursively create the bundle through `*at` operations rooted at `dir`.
/// Both paths prevent a same-uid process from redirecting the predictable
/// temporary name outside the verified application directory.
///
/// A racing creation of `name` surfaces as `EEXIST` and fails closed rather than
/// being followed.
#[cfg(unix)]
fn copy_app_bundle_into<Fd: std::os::fd::AsFd>(
    from: &Path,
    dir: Fd,
    name: &std::ffi::OsStr,
) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        copy_cask_artifact_at(from, dir, name).wrap_err_with(|| {
            format!(
                "brew-cask: cannot stage app bundle {}",
                Path::new(name).display()
            )
        })
    }
    #[cfg(target_os = "macos")]
    {
        nix::sys::stat::mkdirat(&dir, name, nix::sys::stat::Mode::S_IRWXU).wrap_err_with(|| {
            format!(
                "brew-cask: cannot stage app bundle {}",
                Path::new(name).display()
            )
        })?;
        let destination = nix::fcntl::openat(
            &dir,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW,
            nix::sys::stat::Mode::empty(),
        )
        .wrap_err_with(|| {
            format!(
                "brew-cask: cannot open staging directory {}",
                Path::new(name).display()
            )
        })?;
        let stat = nix::sys::stat::fstat(&destination)?;
        if stat.st_uid != nix::unistd::geteuid().as_raw() {
            bail!("brew-cask: staging directory is not owned by the current user");
        }
        // `ditto src dst` copies the *contents* of src into dst, so pointing it at
        // the bound directory reproduces the bundle in place.
        let status = run_in_trusted_dir(
            "ditto",
            &[from.as_os_str(), std::ffi::OsStr::new(".")],
            &destination,
        )?;
        if !status.success() {
            bail!(
                "ditto failed copying {} to {}",
                from.display(),
                Path::new(name).display()
            );
        }
        // Restore the bundle's own permissions, which the private staging mode hid.
        if let Ok(metadata) = from.symlink_metadata() {
            use std::os::unix::fs::PermissionsExt;
            let mode = nix::sys::stat::Mode::from_bits_truncate(
                metadata.permissions().mode() as nix::libc::mode_t
            );
            nix::sys::stat::fchmod(&destination, mode)?;
        }
        Ok(())
    }
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
    let user = std::env::var("USER").unwrap_or_default();
    let env = [
        ("LOGNAME".to_string(), user.clone()),
        ("USER".to_string(), user.clone()),
        ("USERNAME".to_string(), user),
    ];
    sudo::run("/usr/sbin/installer", &args, &env)
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
        create_dir_all_elevating(parent)?;
    }
    rename_elevating(&caskroom_font, &target)?;
    if let Err(err) = make_symlink_elevating(&target, &caskroom_font) {
        if let Err(restore_err) = rename_elevating(&target, &caskroom_font) {
            return Err(err.wrap_err(format!(
                "failed to restore staged font after backlink creation failed: {restore_err:#}"
            )));
        }
        return Err(err);
    }
    Ok(())
}

fn caskroom_font_path(caskroom: &Path, font: &FontArtifact) -> Result<PathBuf> {
    caskroom_artifact_path(caskroom, &font.source, "font")
}

fn caskroom_artifact_path(caskroom: &Path, source: &str, kind: &str) -> Result<PathBuf> {
    let source = Path::new(source);
    if source.is_absolute()
        || source.components().next().is_none()
        || source
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("brew-cask: invalid {kind} source '{}'", source.display());
    }
    Ok(caskroom.join(source))
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
    EffectiveCaskDirs::current().fontdir
}

fn manpage_target_path(manpage: &ManpageArtifact) -> Result<PathBuf> {
    let filename = Path::new(&manpage.source)
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: invalid manpage source '{}'", manpage.source))?;
    Ok(EffectiveCaskDirs::current()
        .manpagedir
        .join(format!("man{}", manpage.section))
        .join(filename))
}

fn staged_manpage_source(
    caskroom: &Path,
    apps: &[AppArtifact],
    manpage: &ManpageArtifact,
) -> Result<PathBuf> {
    if let Some(source) = staged_appdir_artifact_source(&manpage.source, apps, caskroom)? {
        return Ok(source);
    }
    let source = caskroom.join(&manpage.source);
    if !source.is_file() {
        bail!(
            "brew-cask: manpage artifact '{}' was not staged",
            manpage.source
        );
    }
    Ok(source)
}

fn stage_manpage(
    stage: &Path,
    caskroom: &Path,
    apps: &[AppArtifact],
    manpage: &ManpageArtifact,
) -> Result<()> {
    if staged_manpage_source(caskroom, apps, manpage).is_ok() {
        return Ok(());
    }
    let source = find_file_artifact(stage, &manpage.source).ok_or_else(|| {
        eyre!(
            "brew-cask: manpage artifact '{}' was not found",
            manpage.source
        )
    })?;
    let destination = caskroom.join(&manpage.source);
    if let Some(parent) = destination.parent() {
        file::create_dir_all(parent)?;
    }
    copy_cask_artifact(&source, &destination)
}

fn link_manpage(caskroom: &Path, apps: &[AppArtifact], manpage: &ManpageArtifact) -> Result<()> {
    let source = if manpage.source.starts_with("$APPDIR/") {
        appdir_artifact_source(&manpage.source, apps)?.ok_or_else(|| {
            eyre!(
                "brew-cask: manpage APPDIR artifact '{}' is missing after app activation",
                manpage.source
            )
        })?
    } else {
        staged_manpage_source(caskroom, apps, manpage)?
    };
    let target = manpage_target_path(manpage)?;
    if let Some(parent) = target.parent() {
        create_dir_all_elevating(parent)?;
    }
    make_symlink_elevating(&source, &target)
}

fn manpage_target_paths(artifacts: &CaskArtifacts) -> Result<Vec<PathBuf>> {
    artifacts.manpages.iter().map(manpage_target_path).collect()
}

#[cfg(test)]
fn execute_flight_steps(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut targets = FlightTargetTransaction::default();
            execute_flight_steps_with_completion_async(
                cask,
                steps,
                staged_path,
                appdir,
                kind,
                &mut targets,
                |_, _| Ok(()),
            )
            .await?;
            targets.commit()
        })
}

#[cfg(test)]
fn execute_flight_steps_with_completion(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    targets: &mut FlightTargetTransaction,
    completed: impl FnMut(usize, &FlightStep) -> Result<()>,
) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(execute_flight_steps_with_completion_async(
            cask,
            steps,
            staged_path,
            appdir,
            kind,
            targets,
            completed,
        ))
}

#[cfg(test)]
fn execute_flight_step(
    cask: &Cask,
    step: &FlightStep,
    staged_path: &Path,
    appdir: &Path,
    targets: &mut FlightTargetTransaction,
) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(execute_flight_step_async(
            cask,
            step,
            staged_path,
            appdir,
            targets,
        ))
}

async fn execute_flight_steps_recording(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    journal: &mut CaskTransactionJournal,
    targets: &mut FlightTargetTransaction,
) -> Result<()> {
    execute_flight_steps_with_completion_async(
        cask,
        steps,
        staged_path,
        appdir,
        kind,
        targets,
        |index, step| record_cask_action(journal, &format!("{kind}[{index}]:{}", step.kind())),
    )
    .await
}

async fn execute_flight_steps_with_completion_async(
    cask: &Cask,
    steps: &[FlightStep],
    staged_path: &Path,
    appdir: &Path,
    kind: &str,
    targets: &mut FlightTargetTransaction,
    mut completed: impl FnMut(usize, &FlightStep) -> Result<()>,
) -> Result<()> {
    for (index, step) in steps.iter().enumerate() {
        execute_flight_step_async(cask, step, staged_path, appdir, targets)
            .await
            .wrap_err_with(|| {
                format!("brew-cask:{}: failed to run structured {kind}", cask.token)
            })?;
        completed(index, step)?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct FlightTargetTransaction {
    backups: Vec<ArtifactLinkBackup>,
    allowed_targets: Option<BTreeSet<PathBuf>>,
    receipt_caskroom: Option<PathBuf>,
    installed: Vec<PathBuf>,
    uninstall: BTreeMap<PathBuf, bool>,
    previous_symlinks: BTreeSet<PathBuf>,
    copied_files: BTreeSet<PathBuf>,
    previous_directories: BTreeSet<PathBuf>,
    installed_directories: Vec<PathBuf>,
    committed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlightRecoveryRecord {
    target: PathBuf,
    #[serde(default)]
    backup: Option<PathBuf>,
    target_parent: PathBuf,
    #[serde(default)]
    backup_parent: Option<PathBuf>,
    #[serde(default)]
    receipt_caskroom: Option<PathBuf>,
    #[serde(default = "default_elevate_recovery")]
    elevate: bool,
}

fn default_elevate_recovery() -> bool {
    true
}

impl FlightTargetTransaction {
    fn protect(&mut self, target: &Path) -> Result<()> {
        self.protect_with_elevation(target, true)
    }

    fn protect_unprivileged(&mut self, target: &Path) -> Result<()> {
        self.protect_with_elevation(target, false)
    }

    fn protect_generic(&mut self, target: &Path) -> Result<Option<PathBuf>> {
        #[cfg(unix)]
        {
            match open_trusted_operation_parent(target, true, true) {
                Ok(parent) if trusted_parent_is_writable(&parent)? => {
                    self.protect_unprivileged(target)?;
                    Ok(None)
                }
                Ok(_) => {
                    let target = ensure_strict_elevated_target(target)?;
                    self.protect(&target)?;
                    Ok(Some(target))
                }
                Err(err) if is_permission_denied(&err) => {
                    let target = ensure_strict_elevated_target(target)?;
                    self.protect(&target)?;
                    Ok(Some(target))
                }
                Err(err) => Err(err),
            }
        }
        #[cfg(not(unix))]
        {
            self.protect(target)?;
            Ok(None)
        }
    }

    fn protect_with_elevation(&mut self, target: &Path, elevate: bool) -> Result<()> {
        if self
            .allowed_targets
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(target))
        {
            bail!(
                "brew-cask: refusing unpreflighted lifecycle target {}",
                target.display()
            );
        }
        if self.backups.iter().any(|entry| entry.target == target) {
            return Ok(());
        }
        ensure_no_unresolved_flight_recovery(target)?;
        let target_parent = resolved_parent(target)?;
        let backup = if target.symlink_metadata().is_ok() {
            let parent = flight_backup_parent(target)?;
            let backup = unused_flight_backup_path(parent, target)?;
            let recovery = flight_backup_recovery_path(&backup);
            let record = FlightRecoveryRecord {
                target: target.to_path_buf(),
                backup: Some(backup.clone()),
                target_parent: target_parent.clone(),
                backup_parent: Some(resolved_parent(&backup)?),
                receipt_caskroom: self.receipt_caskroom.clone(),
                elevate,
            };
            write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;
            let rename = if elevate {
                rename_elevating(target, &backup)
            } else {
                rename_trusted_generic_target(target, &backup, &target_parent)
            };
            if let Err(err) = rename {
                let _ = file::remove_all(&recovery);
                return Err(err);
            }
            Some(backup)
        } else {
            let recovery = flight_absent_recovery_path(target);
            let record = FlightRecoveryRecord {
                target: target.to_path_buf(),
                backup: None,
                target_parent: target_parent.clone(),
                backup_parent: None,
                receipt_caskroom: self.receipt_caskroom.clone(),
                elevate,
            };
            write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;
            None
        };
        let backup_parent = backup.as_deref().map(resolved_parent).transpose()?;
        self.backups.push(ArtifactLinkBackup {
            target: target.to_path_buf(),
            backup,
            target_parent,
            backup_parent,
            elevate,
        });
        Ok(())
    }

    fn record_installed(&mut self, target: PathBuf) {
        if !self.installed.contains(&target) {
            self.installed.push(target);
        }
    }

    fn record_installed_flight(&mut self, target: PathBuf, uninstall: bool) {
        self.record_installed(target.clone());
        self.uninstall.insert(target, uninstall);
    }

    fn installed_targets(&self) -> &[PathBuf] {
        &self.installed
    }

    #[cfg(test)]
    fn uninstall_targets(&self) -> &BTreeMap<PathBuf, bool> {
        &self.uninstall
    }

    fn record_copied_files(&mut self, source: &Path, target: &Path) -> Result<()> {
        let metadata = source.symlink_metadata()?;
        if metadata.is_file() {
            self.copied_files.insert(file::desymlink_path(target));
            return Ok(());
        }
        for entry in WalkDir::new(source).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(source)?;
                self.copied_files
                    .insert(file::desymlink_path(&target.join(relative)));
            }
        }
        Ok(())
    }

    fn copied_files(&self) -> &BTreeSet<PathBuf> {
        &self.copied_files
    }

    fn record_installed_directory(&mut self, target: PathBuf) {
        if !self.installed_directories.contains(&target) {
            self.installed_directories.push(target);
        }
    }

    #[cfg(test)]
    fn installed_directories(&self) -> &[PathBuf] {
        &self.installed_directories
    }

    fn rollback(&mut self) -> Result<()> {
        let mut first_error = None;
        let mut failed = Vec::new();
        for entry in std::mem::take(&mut self.backups).into_iter().rev() {
            if let Err(err) = validate_backup_parents(&entry) {
                first_error.get_or_insert(err);
                failed.push(entry);
                continue;
            }
            let remove = if entry.elevate {
                remove_artifact_target_elevating(&entry.target)
            } else {
                remove_trusted_generic_target_from(&entry.target, &entry.target_parent)
            };
            if let Err(err) = remove {
                first_error.get_or_insert(err);
                failed.push(entry);
                continue;
            }
            if let Some(backup) = &entry.backup {
                let rename = if entry.elevate {
                    rename_elevating(backup, &entry.target)
                } else {
                    rename_trusted_generic_target(backup, &entry.target, &entry.target_parent)
                };
                if let Err(err) = rename {
                    first_error.get_or_insert(err);
                    failed.push(entry);
                    continue;
                }
            }
            if let Err(err) = file::remove_all(flight_target_recovery_path(&entry)) {
                warn!("brew-cask: failed to remove flight recovery record: {err:#}");
            }
        }
        failed.reverse();
        self.backups = failed;
        self.installed.clear();
        self.uninstall.clear();
        self.copied_files.clear();
        self.installed_directories.clear();
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn commit(&mut self) -> Result<()> {
        self.committed = true;
        let backups = std::mem::take(&mut self.backups);
        let mut first_error = None;
        for entry in backups {
            if let Err(err) = file::remove_all(flight_target_recovery_path(&entry)) {
                first_error.get_or_insert(err);
            }
            if let Some(backup) = &entry.backup {
                // Attempt both removals independently. Either a missing record
                // or a missing backup is enough to prevent stale recovery from
                // restoring pre-install data over a later target.
                let remove = if entry.elevate {
                    remove_artifact_target_elevating(backup)
                } else {
                    remove_trusted_generic_target_from(
                        backup,
                        entry.backup_parent.as_ref().unwrap(),
                    )
                };
                if let Err(err) = remove {
                    first_error.get_or_insert(err);
                }
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }
}

fn unused_flight_backup_path(parent: &Path, target: &Path) -> Result<PathBuf> {
    let stem = format!(
        ".mise-flight-backup-{}-{}",
        hash::hash_to_str(&target.display().to_string()),
        std::process::id()
    );
    for attempt in 0_u64.. {
        let backup = parent.join(format!("{stem}-{attempt}"));
        let recovery = flight_backup_recovery_path(&backup);
        let backup_missing = match backup.symlink_metadata() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Ok(_) => false,
            Err(err) => return Err(err.into()),
        };
        let recovery_missing = match recovery.symlink_metadata() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Ok(_) => false,
            Err(err) => return Err(err.into()),
        };
        if backup_missing && recovery_missing {
            return Ok(backup);
        }
    }
    unreachable!("the flight backup suffix space is exhausted")
}

fn flight_backup_recovery_path(backup: &Path) -> PathBuf {
    flight_recovery_root().join(format!(
        "{}.recovery",
        hash::hash_to_str(&backup.display().to_string())
    ))
}

fn flight_absent_recovery_path(target: &Path) -> PathBuf {
    flight_recovery_root().join(format!(
        "absent-{}.recovery",
        hash::hash_to_str(&target.display().to_string())
    ))
}

fn flight_target_recovery_path(entry: &ArtifactLinkBackup) -> PathBuf {
    entry
        .backup
        .as_deref()
        .map(flight_backup_recovery_path)
        .unwrap_or_else(|| flight_absent_recovery_path(&entry.target))
}

fn flight_recovery_root() -> PathBuf {
    crate::dirs::STATE.join("brew-cask").join("flight-recovery")
}

fn ensure_no_unresolved_flight_recovery(target: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(flight_recovery_root()) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "recovery")
        {
            continue;
        }
        let Ok(body) = file::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<FlightRecoveryRecord>(&body) else {
            continue;
        };
        if record.target == target {
            if let Some(backup) = record.backup {
                bail!(
                    "brew-cask: unresolved recovery for {} still preserves its original at {}",
                    target.display(),
                    backup.display()
                );
            }
            bail!(
                "brew-cask: unresolved recovery for newly created target {}",
                target.display()
            );
        }
    }
    Ok(())
}

fn recover_flight_backups_for_cask(token: &str) -> Result<()> {
    let root = flight_recovery_root();
    let token_dir = caskroom_token_dir(token);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("recovery") {
            continue;
        }
        let body = file::read_to_string(&path).wrap_err_with(|| {
            format!("failed to read flight recovery record {}", path.display())
        })?;
        let record: FlightRecoveryRecord = serde_json::from_str(&body)
            .wrap_err_with(|| format!("invalid flight recovery record {}", path.display()))?;
        if record
            .receipt_caskroom
            .as_deref()
            .and_then(Path::parent)
            .is_some_and(|parent| file::desymlink_path(parent) == file::desymlink_path(&token_dir))
        {
            recover_flight_backup(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn recover_flight_backups_in(root: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(());
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                warn!(
                    "brew-cask: failed to inspect a flight recovery entry in {}: {err:#}",
                    root.display()
                );
                continue;
            }
        };
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("recovery") => recover_flight_backup_or_warn(&path),
            // Atomic record writes may leave their temporary file behind if
            // the process dies before rename. It is not a recovery record.
            Some("tmp") => {
                if let Err(err) = file::remove_all(&path) {
                    warn!(
                        "brew-cask: failed to remove stale flight recovery file {}: {err:#}",
                        path.display()
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
fn recover_flight_backup_or_warn(path: &Path) {
    if let Err(err) = recover_flight_backup(path) {
        warn!(
            "brew-cask: leaving flight recovery record {} for a later retry: {err:#}",
            path.display()
        );
    }
}

fn recover_flight_backup(path: &Path) -> Result<()> {
    let body = file::read_to_string(path)
        .wrap_err_with(|| format!("failed to read flight recovery record {}", path.display()))?;
    let record: FlightRecoveryRecord = serde_json::from_str(&body)
        .wrap_err_with(|| format!("invalid flight recovery record {}", path.display()))?;
    let backup = ArtifactLinkBackup {
        target: record.target,
        backup: record.backup,
        target_parent: record.target_parent,
        backup_parent: record.backup_parent,
        elevate: record.elevate,
    };
    validate_backup_parents(&backup)?;
    if flight_target_claimed_by_receipt(&backup.target, record.receipt_caskroom.as_deref())? {
        if let Some(backup_path) = &backup.backup
            && backup_path.symlink_metadata().is_ok()
        {
            if backup.elevate {
                remove_artifact_target_elevating(backup_path)?;
            } else {
                remove_trusted_generic_target_from(
                    backup_path,
                    backup
                        .backup_parent
                        .as_ref()
                        .ok_or_else(|| eyre!("brew-cask: flight backup parent is missing"))?,
                )?;
            }
        }
        file::remove_all(path)?;
        return Ok(());
    }
    if let Some(backup_path) = &backup.backup {
        if backup_path.symlink_metadata().is_ok() {
            if backup.target.symlink_metadata().is_ok() {
                // A target created after the interrupted transaction may be user
                // data, a successfully activated replacement, or a replacement
                // that rollback failed to remove. Without enough information to
                // distinguish those cases, preserve both entries and leave the
                // original backup available for manual recovery.
                warn!(
                    "brew-cask: preserving interrupted flight target {} and its original backup {}",
                    backup.target.display(),
                    backup_path.display()
                );
                return Ok(());
            } else if backup.elevate {
                rename_elevating(backup_path, &backup.target)?;
            } else {
                rename_trusted_generic_target(backup_path, &backup.target, &backup.target_parent)?;
            }
        }
    } else if backup.target.symlink_metadata().is_ok()
        && !flight_target_claimed_by_receipt(&backup.target, record.receipt_caskroom.as_deref())?
    {
        if backup.elevate {
            remove_artifact_target_elevating(&backup.target)?;
        } else {
            remove_trusted_generic_target_from(&backup.target, &backup.target_parent)?;
        }
    }
    file::remove_all(path)?;
    Ok(())
}

fn flight_target_claimed_by_receipt(target: &Path, caskroom: Option<&Path>) -> Result<bool> {
    let Some(caskroom) = caskroom else {
        return Ok(false);
    };
    if let Some(receipt) = read_receipt(caskroom)? {
        if receipt.flight_directories.iter().any(|path| path == target) && target.is_dir() {
            return Ok(true);
        }
        for record in receipt
            .targets
            .iter()
            .filter(|record| record.path == target)
        {
            if cask_target_record_matches(record)? {
                return Ok(true);
            }
        }
    }
    let Some(token_dir) = caskroom.parent() else {
        return Ok(false);
    };
    let token = token_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("brew-cask: recovery Caskroom token is invalid"))?;
    if read_pending_cask_journal_in(&crate::dirs::STATE, token)?.is_some_and(|journal| {
        journal.recovery == CaskRecoveryMode::FinishCommit
            && journal
                .receipt_inventory_targets
                .iter()
                .any(|path| path == target)
    }) {
        return Ok(target.symlink_metadata().is_ok());
    }
    if token_dir.join(".metadata").symlink_metadata().is_err() {
        return Ok(false);
    }
    let receipt = receipt::read_cask_receipt(token_dir)?;
    for record in homebrew_receipt_targets(token, &receipt)? {
        if record.path == target && cask_target_record_matches(&record)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn resolved_parent(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("brew-cask: flight target has no parent"))?;
    Ok(path_with_resolved_existing_ancestor(parent))
}

fn validate_backup_parents(entry: &ArtifactLinkBackup) -> Result<()> {
    if resolved_parent(&entry.target)? != entry.target_parent
        || entry
            .backup
            .as_deref()
            .zip(entry.backup_parent.as_ref())
            .is_some_and(|(backup, expected)| {
                !resolved_parent(backup).is_ok_and(|current| current == *expected)
            })
    {
        bail!(
            "brew-cask: refusing to restore flight target through a changed parent: {}",
            entry.target.display()
        );
    }
    Ok(())
}

fn flight_backup_parent(target: &Path) -> Result<&Path> {
    if let Some(app) = target.ancestors().find(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    }) {
        return app
            .parent()
            .ok_or_else(|| eyre!("brew-cask: app flight target has no parent"));
    }
    target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: flight target has no parent"))
}

impl Drop for FlightTargetTransaction {
    fn drop(&mut self) {
        if !self.committed
            && let Err(err) = self.rollback()
        {
            warn!("brew-cask: failed to roll back flight targets: {err:#}");
        }
    }
}

impl FlightStep {
    fn kind(&self) -> &'static str {
        match self {
            Self::Move { .. } => "move",
            Self::Remove { .. } => "remove",
            Self::Copy { .. } => "copy",
            Self::Symlink { .. } => "symlink",
            Self::Run { .. } => "run",
            Self::TerminateProcess { .. } => "terminate_process",
            Self::SetOwnership { .. } => "set_ownership",
        }
    }
}

async fn execute_flight_step_async(
    cask: &Cask,
    step: &FlightStep,
    staged_path: &Path,
    appdir: &Path,
    targets: &mut FlightTargetTransaction,
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
        FlightStep::Copy {
            source,
            target,
            recursive,
            overwrite,
            source_glob,
            guards,
        } => {
            if !guards
                .iter()
                .map(|guard| flight_guard_matches(cask, guard, staged_path, appdir))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .all(|matches| matches)
            {
                return Ok(());
            }
            let sources = flight_symlink_sources(cask, source, *source_glob, staged_path, appdir)?;
            let [source] = sources.as_slice() else {
                bail!("brew-cask: structured copy source must resolve to exactly one path");
            };
            if !source.exists() {
                bail!(
                    "brew-cask: structured copy source '{}' was not found",
                    source.display()
                );
            }
            if source.is_dir() && !recursive {
                bail!("brew-cask: structured directory copy requires recursive=true");
            }
            let target = resolve_flight_path_with_context(cask, target, staged_path, appdir)?;
            let external = !target.starts_with(staged_path);
            let target_metadata = target.symlink_metadata().ok();
            if target_metadata.is_some() {
                if !overwrite {
                    bail!(
                        "brew-cask: structured copy target '{}' already exists",
                        target.display()
                    );
                }
                if external {
                    targets.protect(&target)?;
                } else {
                    file::remove_all(&target)?;
                }
            }
            if let Some(parent) = target.parent() {
                create_dir_all_elevating(parent)?;
            }
            if external && target_metadata.is_none() {
                // Bind an absent target to its resolved parent only after
                // creating that parent so rollback can validate its identity.
                targets.protect(&target)?;
            }
            copy_cask_artifact(source, &target)?;
            if external {
                targets.record_copied_files(source, &target)?;
            }
            // External copy trees may be modified during normal use. The
            // transaction backup is sufficient for rollback; recording them
            // would fingerprint their contents and force later reinstalls.
        }
        FlightStep::Symlink {
            source,
            target,
            force,
            uninstall,
            source_glob,
            sudo,
            guards,
        } => {
            if !guards
                .iter()
                .map(|guard| flight_guard_matches(cask, guard, staged_path, appdir))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .all(|matches| matches)
            {
                return Ok(());
            }
            let target = resolve_flight_path_with_context(cask, target, staged_path, appdir)?;
            let sources = flight_symlink_sources(cask, source, *source_glob, staged_path, appdir)?;
            if sources.is_empty() {
                bail!(
                    "brew-cask: structured symlink source '{}' did not match any paths",
                    source.path
                );
            }
            let target_metadata = target.symlink_metadata().ok();
            let target_is_real_dir = target_metadata
                .as_ref()
                .is_some_and(|metadata| metadata.is_dir());
            let target_is_dir = target_is_real_dir || sources.len() > 1;
            if sources.len() > 1 {
                let created_external_directory =
                    target_metadata.is_none() && !target.starts_with(staged_path);
                if target_metadata.is_some() && !target_is_real_dir {
                    if target.exists() && !force && !targets.previous_symlinks.contains(&target) {
                        bail!(
                            "brew-cask: structured symlink target '{}' already exists",
                            target.display()
                        );
                    }
                    targets.protect(&target)?;
                } else if created_external_directory {
                    // Record the absent directory itself so rollback removes
                    // the container after removing the links created below.
                    if let Some(parent) = target.parent() {
                        create_flight_dir_all(parent, *sudo)?;
                    }
                    targets.protect(&target)?;
                }
                create_flight_dir_all(&target, *sudo)?;
                if created_external_directory || targets.previous_directories.contains(&target) {
                    targets.record_installed_directory(target.clone());
                }
            }
            for source in sources {
                let link = if target_is_dir {
                    let source_name = Path::new(&source).file_name().ok_or_else(|| {
                        eyre!("brew-cask: structured symlink source has no file name")
                    })?;
                    target.join(source_name)
                } else {
                    target.clone()
                };
                let external = !link.starts_with(staged_path);
                let link_metadata = link.symlink_metadata().ok();
                if let Some(metadata) = &link_metadata {
                    if metadata.is_dir() {
                        bail!(
                            "brew-cask: refusing to replace structured symlink directory '{}'",
                            link.display()
                        );
                    }
                    if link.exists() && !force && !targets.previous_symlinks.contains(&link) {
                        bail!(
                            "brew-cask: structured symlink target '{}' already exists",
                            link.display()
                        );
                    }
                    if external {
                        targets.protect(&link)?;
                    } else if metadata.file_type().is_symlink() {
                        file::remove_file(&link)?;
                    } else {
                        file::remove_all(&link)?;
                    }
                }
                if let Some(parent) = link.parent() {
                    create_flight_dir_all(parent, *sudo)?;
                }
                if external && link_metadata.is_none() {
                    // Bind an absent target to its resolved parent only after
                    // creating that parent; otherwise rollback observes a
                    // different path identity and leaves the new link behind.
                    targets.protect(&link)?;
                }
                let source =
                    durable_internal_symlink_source(staged_path, &source, &link).unwrap_or(source);
                create_flight_symlink(&source, &link, *sudo)?;
                if external {
                    targets.record_installed_flight(link, *uninstall);
                }
            }
        }
        FlightStep::Run {
            command,
            args,
            env,
            sudo,
            network_access,
            guards,
        } => {
            if !guards
                .iter()
                .map(|guard| flight_guard_matches(cask, guard, staged_path, appdir))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .all(|matches| matches)
            {
                return Ok(());
            }
            let command_path =
                resolve_flight_path_with_context(cask, command, staged_path, appdir)?;
            validate_flight_run_command(cask, command, &command_path, staged_path, appdir)?;
            let command =
                expand_flight_template(cask, &command_path.to_string_lossy(), staged_path, appdir);
            let args = args
                .iter()
                .map(|arg| expand_flight_template(cask, arg, staged_path, appdir))
                .collect::<Vec<_>>();
            let env = env
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        expand_flight_template(cask, value, staged_path, appdir),
                    )
                })
                .collect::<Vec<_>>();
            execute_confined_flight_run(FlightRunRequest {
                cask,
                command: &command,
                args: &args,
                env: &env,
                sudo: *sudo,
                network_access: *network_access,
                staged_path,
                appdir,
            })
            .await?;
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
        FlightStep::SetOwnership { .. } => {
            bail!("brew-cask: set_ownership is supported only in uninstall preflight steps")
        }
    }
    Ok(())
}

fn validate_flight_run_command(
    cask: &Cask,
    declared: &FlightPath,
    command: &Path,
    staged_path: &Path,
    appdir: &Path,
) -> Result<()> {
    if command
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask:{}: structured run command contains a parent traversal: {}",
            cask.token,
            command.display()
        );
    }
    let contained = match declared.base {
        FlightPathBase::StagedPath => path_starts_with_resolved_root(command, staged_path),
        FlightPathBase::AppDir => path_starts_with_resolved_root(command, appdir),
        FlightPathBase::HomebrewPrefix => {
            path_starts_with_resolved_root(command, &prefix::prefix())
        }
        FlightPathBase::Literal => {
            !command.is_absolute()
                || ["/bin", "/usr/bin", "/usr/sbin", "/sbin"]
                    .iter()
                    .any(|root| path_starts_with_resolved_root(command, Path::new(root)))
        }
    };
    if !contained {
        bail!(
            "brew-cask:{}: structured run command escapes its declared execution root: {}",
            cask.token,
            command.display()
        );
    }
    Ok(())
}

struct FlightRunRequest<'a> {
    cask: &'a Cask,
    command: &'a str,
    args: &'a [String],
    env: &'a [(String, String)],
    sudo: bool,
    network_access: bool,
    staged_path: &'a Path,
    appdir: &'a Path,
}

async fn execute_confined_flight_run(request: FlightRunRequest<'_>) -> Result<()> {
    if request.sudo {
        bail!(
            "brew-cask:{}: structured sudo run is unsupported because it cannot retain process confinement",
            request.cask.token
        );
    }
    let temp = cask_step_home(request.cask);
    file::create_dir_all(&temp)?;
    let shared = prefix::prefix();
    let mut allow_write = vec![
        request.staged_path.to_path_buf(),
        request.appdir.to_path_buf(),
        temp.clone(),
    ];
    allow_write.extend(
        super::pour::LINK_DIRS
            .iter()
            .map(|directory| shared.join(directory)),
    );
    let mut sandbox = SandboxConfig {
        deny_write: true,
        deny_net: !request.network_access,
        deny_env: true,
        allow_write,
        deny_system_temp_write: true,
        ..Default::default()
    };
    sandbox.resolve_paths();
    let path = std::env::join_paths([
        shared.join("bin"),
        shared.join("sbin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ])?;
    let mut deterministic_env = BTreeMap::from([
        ("HOME".to_string(), temp.to_string_lossy().into_owned()),
        ("LANG".to_string(), "C".to_string()),
        ("LC_ALL".to_string(), "C".to_string()),
        ("PATH".to_string(), path.to_string_lossy().into_owned()),
        (
            "HOMEBREW_PREFIX".to_string(),
            shared.to_string_lossy().into_owned(),
        ),
        ("TMPDIR".to_string(), temp.to_string_lossy().into_owned()),
    ]);
    deterministic_env.extend(request.env.iter().cloned());
    let mut runner = CmdLineRunner::new(request.command)
        .args(request.args)
        .with_sandbox(sandbox);
    runner.apply_sandbox().await?;
    runner
        .env_clear()
        .envs(&deterministic_env)
        .raw(true)
        .execute_async()
        .await
}

fn cask_step_home(cask: &Cask) -> PathBuf {
    caskroom_tmp_dir(cask).join(".mise-step-home")
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

fn flight_guard_matches(
    cask: &Cask,
    guard: &FlightGuard,
    staged_path: &Path,
    appdir: &Path,
) -> Result<bool> {
    match guard {
        FlightGuard::OnMacos => Ok(cfg!(target_os = "macos")),
        FlightGuard::OnLinux => Ok(cfg!(target_os = "linux")),
        FlightGuard::IfExists(path) => flight_guard_path_exists(&resolve_flight_path_with_context(
            cask,
            path,
            staged_path,
            appdir,
        )?),
        FlightGuard::UnlessExists(path) => Ok(!flight_guard_path_exists(
            &resolve_flight_path_with_context(cask, path, staged_path, appdir)?,
        )?),
    }
}

fn flight_guard_path_exists(path: &Path) -> Result<bool> {
    match path.metadata() {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).wrap_err_with(|| {
            format!(
                "failed to evaluate structured lifecycle guard: {}",
                path.display()
            )
        }),
    }
}

fn flight_symlink_sources(
    cask: &Cask,
    source: &FlightPath,
    source_glob: bool,
    staged_path: &Path,
    appdir: &Path,
) -> Result<Vec<PathBuf>> {
    if source_glob {
        if source.base != FlightPathBase::StagedPath {
            bail!("brew-cask: structured symlink globs must use staged_path");
        }
        let pattern = expand_flight_template(cask, &source.path, staged_path, appdir);
        return expand_staged_glob(staged_path, &pattern);
    }
    Ok(vec![resolve_flight_path_with_context(
        cask,
        source,
        staged_path,
        appdir,
    )?])
}

fn create_flight_symlink(source: &Path, target: &Path, sudo: FlightSudo) -> Result<()> {
    match sudo {
        FlightSudo::Never => file::make_symlink(source, target).map(|_| ()),
        FlightSudo::IfNeeded => make_symlink_elevating(source, target),
        FlightSudo::Always => sudo::run("/bin/ln", &symlink_command_args(source, target), &[]),
    }
}

fn create_flight_dir_all(target: &Path, sudo: FlightSudo) -> Result<()> {
    match sudo {
        FlightSudo::Never => file::create_dir_all(target),
        FlightSudo::IfNeeded => create_dir_all_elevating(target),
        FlightSudo::Always => sudo::run(
            "/bin/mkdir",
            &["-p".into(), "--".into(), target.display().to_string()],
            &[],
        ),
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
    cask: &Cask,
    path: &FlightPath,
    staged_path: &Path,
    appdir: &Path,
) -> Result<PathBuf> {
    let expanded = expand_flight_template(cask, &path.path, staged_path, appdir);
    match path.base {
        FlightPathBase::StagedPath => {
            validate_flight_relative_path(&expanded)?;
            Ok(staged_path.join(expanded))
        }
        FlightPathBase::AppDir => {
            validate_flight_relative_path(&expanded)?;
            Ok(appdir.join(expanded))
        }
        FlightPathBase::HomebrewPrefix => {
            validate_flight_relative_path(&expanded)?;
            Ok(prefix::prefix().join(expanded))
        }
        FlightPathBase::Literal => Ok(PathBuf::from(expanded)),
    }
}

fn expand_flight_template(cask: &Cask, value: &str, staged_path: &Path, appdir: &Path) -> String {
    let caskroom_path = caskroom_token_dir(&cask.token);
    let version_major = cask
        .version
        .split(['.', ','])
        .next()
        .unwrap_or(&cask.version);
    let value = value
        .replace("{{version.major}}", version_major)
        .replace("{{caskroom_path}}", &caskroom_path.to_string_lossy());
    expand_cask_template(&value, staged_path, appdir, Some(&cask.version))
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
    if completion.source.starts_with("$APPDIR/") {
        staged_appdir_artifact_source(&completion.source, apps, caskroom)?.ok_or_else(|| {
            eyre!(
                "brew-cask: {} completion APPDIR artifact '{}' was not staged",
                completion.shell.name(),
                completion.source
            )
        })?;
        // Homebrew links APPDIR completions directly into the moved app. Do
        // not create a second Caskroom copy with different receipt topology.
        return Ok(());
    }
    find_completion_source(stage, caskroom, cask, apps, &completion.source)?.ok_or_else(|| {
        eyre!(
            "brew-cask: {} completion artifact '{}' was not found",
            completion.shell.name(),
            completion.source
        )
    })?;
    // Like every Homebrew Symlinked artifact, a declared completion links its
    // public target directly to the preserved staged source. Do not duplicate
    // it under a path derived from the public target.
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
        let staged_completion = generated_completion_staging_path(stage, &target)?;
        if let Some(parent) = staged_completion.parent() {
            file::create_dir_all(parent)?;
        }
        let output = generate_completion_output(&executable, completion, *shell)?;
        crate::file::write(staged_completion, output)?;
    }
    Ok(())
}

fn link_completion(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    caskroom: &Path,
    stage: &Path,
    target: &Path,
) -> Result<()> {
    let mut declared = artifacts
        .completions
        .iter()
        .filter(|completion| completion.target_path().is_ok_and(|path| path == target));
    let first_declared = declared.next();
    if first_declared.is_some() && declared.next().is_some() {
        bail!(
            "brew-cask:{}: multiple completion artifacts claim '{}'",
            cask.token,
            target.display()
        );
    }
    let generated = completion_target_is_generated(cask, artifacts, target)?;
    if usize::from(first_declared.is_some()) + usize::from(generated) != 1 {
        bail!(
            "brew-cask:{}: completion target '{}' has ambiguous artifact ownership",
            cask.token,
            target.display()
        );
    }
    let source = match first_declared {
        Some(completion) if completion.source.starts_with("$APPDIR/") => {
            appdir_artifact_source(&completion.source, &artifacts.apps)?.ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: completion APPDIR artifact '{}' is missing after app activation",
                    cask.token,
                    completion.source
                )
            })?
        }
        Some(completion) => find_completion_source(
            caskroom,
            caskroom,
            cask,
            &artifacts.apps,
            &completion.source,
        )?
        .ok_or_else(|| {
            eyre!(
                "brew-cask:{}: completion artifact '{}' is missing after Caskroom activation",
                cask.token,
                completion.source
            )
        })?,
        None => generated_completion_staging_path(stage, target)?,
    };
    if !source.is_file() {
        bail!(
            "brew-cask: completion artifact '{}' was not staged",
            target.display()
        );
    }
    if let Some(parent) = target.parent() {
        create_dir_all_elevating(parent)?;
    }
    ensure_completion_target_replaceable(cask, artifacts, target)?;
    if first_declared.is_some() {
        make_symlink_elevating(&source, target)?;
    } else {
        copy_cask_artifact(&source, target)?;
    }
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
    if let Some(source) = staged_appdir_artifact_source(source, apps, caskroom)? {
        return Ok(Some(source));
    }
    if let Some(source) = appdir_artifact_source(source, apps)? {
        return Ok(Some(source));
    }
    if let Some(source) = absolute_prefixed_source(source).filter(|source| source.is_file()) {
        return Ok(Some(source));
    }
    for root in [caskroom, stage] {
        let matches = find_file_artifacts(root, Path::new(source));
        match matches.as_slice() {
            [] => {}
            [source] => return Ok(Some(source.clone())),
            _ => {
                bail!(
                    "brew-cask: completion artifact '{}' is ambiguous: {}",
                    source,
                    matches
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
    Ok(None)
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
    if let Some(source) = staged_appdir_artifact_source(executable, apps, caskroom)? {
        return Ok(source);
    }
    if let Some(source) = appdir_artifact_source(executable, apps)? {
        return Ok(source);
    }
    if let Some(source) =
        declared_binary_source_for_completion(stage, caskroom, cask, apps, executable)?
    {
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

fn declared_binary_source_for_completion(
    stage: &Path,
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    executable: &str,
) -> Result<Option<PathBuf>> {
    let Some(target) = absolute_prefixed_source(executable) else {
        return Ok(None);
    };
    let appdir = cask_appdir(apps)?;
    let mut binaries = cask
        .artifacts
        .iter()
        .filter_map(parse_binary_artifact)
        .filter_map(|binary| {
            binary
                .target_path(&appdir)
                .is_ok_and(|candidate| candidate == target)
                .then_some(binary)
        });
    let Some(binary) = binaries.next() else {
        return Ok(None);
    };
    if binaries.next().is_some() {
        bail!(
            "brew-cask:{}: multiple binary artifacts provide completion executable '{}'",
            cask.token,
            executable
        );
    }
    if binary.source.starts_with("$APPDIR/") {
        return binary_appdir_artifact_source(&binary.source, apps);
    }
    find_binary_source(stage, caskroom, cask, &binary).map(Some)
}

fn appdir_artifact_source(source: &str, apps: &[AppArtifact]) -> Result<Option<PathBuf>> {
    appdir_artifact_source_matching(source, apps, false, true)
}

fn binary_appdir_artifact_source(source: &str, apps: &[AppArtifact]) -> Result<Option<PathBuf>> {
    appdir_artifact_source_matching(source, apps, true, true)
}

fn declared_binary_appdir_artifact_source(
    source: &str,
    apps: &[AppArtifact],
) -> Result<Option<PathBuf>> {
    appdir_artifact_source_matching(source, apps, true, false)
}

fn appdir_artifact_source_matching(
    source: &str,
    apps: &[AppArtifact],
    allow_directory: bool,
    require_node: bool,
) -> Result<Option<PathBuf>> {
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
        if !require_node || appdir_source_node_is_owned(&path, &target, allow_directory) {
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

fn appdir_source_node_is_owned(path: &Path, app: &Path, allow_directory: bool) -> bool {
    (path.is_file() || (allow_directory && path.is_dir()))
        && path_starts_with_resolved_root(path, app)
}

fn staged_appdir_artifact_source(
    source: &str,
    apps: &[AppArtifact],
    caskroom: &Path,
) -> Result<Option<PathBuf>> {
    staged_appdir_artifact_source_matching(source, apps, caskroom, false)
}

fn staged_binary_appdir_artifact_source(
    source: &str,
    apps: &[AppArtifact],
    caskroom: &Path,
) -> Result<Option<PathBuf>> {
    staged_appdir_artifact_source_matching(source, apps, caskroom, true)
}

fn staged_appdir_artifact_source_matching(
    source: &str,
    apps: &[AppArtifact],
    caskroom: &Path,
    allow_directory: bool,
) -> Result<Option<PathBuf>> {
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
        let staged_app = caskroom.join(app_bundle_name(app.target_name())?);
        let path = staged_app.join(&suffix);
        if appdir_source_node_is_owned(&path, &staged_app, allow_directory) {
            matches.push(path);
        }
    }
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => bail!(
            "brew-cask: staged APPDIR artifact '{}' is ambiguous: {}",
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

fn generated_completion_staging_path(stage: &Path, target: &Path) -> Result<PathBuf> {
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
    Ok(stage.join(".mise-generated-completions").join(relative))
}

fn generated_completion_matches_staging(stage: &Path, target: &Path) -> bool {
    let Ok(staged) = generated_completion_staging_path(stage, target) else {
        return false;
    };
    let Ok(target_metadata) = target.symlink_metadata() else {
        return false;
    };
    let Ok(staged_metadata) = staged.symlink_metadata() else {
        return false;
    };
    target_metadata.file_type().is_file()
        && staged_metadata.file_type().is_file()
        && target_metadata.len() == staged_metadata.len()
        && std::fs::read(target)
            .ok()
            .zip(std::fs::read(staged).ok())
            .is_some_and(|(target, staged)| target == staged)
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
    if binary.source.starts_with("$APPDIR/") {
        staged_binary_appdir_artifact_source(&binary.source, apps, caskroom)?.ok_or_else(|| {
            eyre!(
                "brew-cask: binary artifact '{}' was not found",
                binary.source
            )
        })?;
        return Ok(());
    }
    let source = find_binary_source(stage, caskroom, cask, binary)?;
    if !path_traverses_symlink_below(stage, &source)
        && !path_traverses_symlink_below(caskroom, &source)
    {
        return Ok(());
    }
    let appdir = cask_appdir(apps)?;
    let caskroom_binary = caskroom_binary_path(caskroom, &appdir, binary)?;
    file::remove_all(&caskroom_binary)?;
    if let Some(parent) = caskroom_binary.parent() {
        file::create_dir_all(parent)?;
    }
    if path_starts_with_resolved_root(&source, stage)
        || path_starts_with_resolved_root(&source, caskroom)
    {
        file::copy(&source, &caskroom_binary)?;
        file::make_executable(&caskroom_binary)?;
    } else {
        file::make_symlink(
            &durable_binary_link_target(&source, stage, caskroom),
            &caskroom_binary,
        )?;
    }
    Ok(())
}

fn caskroom_binary_path(
    caskroom: &Path,
    appdir: &Path,
    binary: &BinaryArtifact,
) -> Result<PathBuf> {
    let target = binary.target_path(appdir)?;
    let roots = if is_appdir_binary_target(&binary.target_name()?) {
        let mut roots = allowed_appdir_roots()?;
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

fn path_traverses_symlink_below(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if current
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return true;
        }
    }
    false
}

fn stage_command_wrapper(
    caskroom: &Path,
    appdir: &Path,
    cask: &Cask,
    wrapper: &CommandWrapperArtifact,
) -> Result<()> {
    let (content, readonly) = render_command_wrapper(appdir, cask, wrapper)?;
    let target = wrapper.caskroom_path(caskroom);
    file::remove_all(&target)?;
    if let Some(parent) = target.parent() {
        file::create_dir_all(parent)?;
    }
    file::write(&target, content)?;
    if readonly {
        set_command_wrapper_readonly_executable(&target)?;
    } else {
        file::make_executable(&target)?;
    }
    Ok(())
}

fn render_command_wrapper(
    appdir: &Path,
    cask: &Cask,
    wrapper: &CommandWrapperArtifact,
) -> Result<(String, bool)> {
    let (content, readonly) = match (&wrapper.content, &wrapper.executable) {
        (Some(content), None) => (expand_command_wrapper_content(content, appdir), false),
        (None, Some(executable)) => {
            let executable = expand_command_wrapper_value(executable, appdir, cask);
            validate_command_wrapper_double_quoted_value("executable", &executable)?;
            let args = wrapper
                .args
                .iter()
                .map(|arg| expand_command_wrapper_value(arg, appdir, cask))
                .map(|arg| homebrew_shell_escape(&arg))
                .collect::<Result<Vec<_>>>()?
                .join(" ");
            let mut env = String::new();
            for (key, value) in &wrapper.env {
                let value = expand_command_wrapper_value(value, appdir, cask);
                validate_command_wrapper_double_quoted_value("environment value", &value)?;
                env.push_str(&format!("{key}=\"{value}\" "));
            }
            (
                format!("#!/bin/bash\n{env}exec \"{executable}\" {args} \"$@\"\n"),
                true,
            )
        }
        _ => bail!(
            "brew-cask: command_wrapper '{}' must set exactly one of content or executable",
            wrapper.name
        ),
    };
    Ok((content, readonly))
}

fn validate_command_wrapper_double_quoted_value(kind: &str, value: &str) -> Result<()> {
    if value
        .chars()
        .any(|character| matches!(character, '\0' | '\n' | '\r' | '"' | '$' | '`' | '\\'))
    {
        bail!("brew-cask: command_wrapper {kind} cannot be represented safely");
    }
    Ok(())
}

fn homebrew_shell_escape(value: &str) -> Result<String> {
    if value.contains('\0') {
        bail!("brew-cask: command_wrapper argument contains a NUL byte");
    }
    if value.is_empty() {
        return Ok("''".to_string());
    }
    let mut escaped = String::new();
    for character in value.chars() {
        if character == '\n' {
            escaped.push('\'');
            escaped.push('\n');
            escaped.push('\'');
        } else if character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '.' | ',' | ':' | '/' | '@')
        {
            escaped.push(character);
        } else {
            escaped.push('\\');
            escaped.push(character);
        }
    }
    Ok(escaped)
}

#[cfg(unix)]
fn set_command_wrapper_readonly_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = path.metadata()?.permissions();
    permissions.set_mode(0o555);
    std::fs::set_permissions(path, permissions)
        .wrap_err_with(|| format!("failed to chmod 0555: {}", path.display()))
}

#[cfg(not(unix))]
fn set_command_wrapper_readonly_executable(path: &Path) -> Result<()> {
    file::make_executable(path)
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
    for root in [caskroom, stage] {
        let source = root.join(&binary.source);
        if source.is_file() {
            return Ok(source);
        }
    }
    for root in [caskroom, stage] {
        let matches = find_file_artifacts(root, Path::new(&binary.source));
        match matches.as_slice() {
            [] => {}
            [source] => return Ok(source.clone()),
            _ => {
                bail!(
                    "brew-cask: binary artifact '{}' is ambiguous: {}",
                    binary.source,
                    matches
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
    bail!(
        "brew-cask: binary artifact '{}' was not found",
        binary.source
    )
}

/// Where to point a caskroom binary link whose source is not stage content.
///
/// A source handed to us under the stage or the temporary caskroom, yet
/// resolving outside it, has to be linked at its real location: a link at the
/// literal path dangles as soon as staging tears that directory down. This is
/// reachable both from the walk, which returns a symlink entry it matched by
/// name, and from `generated_caskroom_artifact`, which maps a hook-generated
/// path onto either root.
///
/// Sources that already sit outside both roots — the absolute paths a pkg
/// installer creates, for instance — are linked verbatim, so the recorded
/// target stays the path the cask metadata named.
fn durable_binary_link_target(source: &Path, stage: &Path, caskroom: &Path) -> PathBuf {
    if source.starts_with(stage) || source.starts_with(caskroom) {
        file::desymlink_path(source)
    } else {
        source.to_path_buf()
    }
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

fn durable_internal_symlink_source(stage: &Path, source: &Path, link: &Path) -> Option<PathBuf> {
    if !source.is_absolute()
        || staged_relative_path(stage, source).is_none()
        || staged_relative_path(stage, link).is_none()
    {
        return None;
    }
    relative_path_between(link.parent()?, source)
}

fn relative_path_between(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = lexically_normalized_path(from);
    let to = lexically_normalized_path(to);
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(from, to)| from == to)
        .count();
    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        match component {
            Component::Normal(_) => relative.push(".."),
            Component::CurDir => {}
            _ => return None,
        }
    }
    for component in &to_components[common..] {
        match component {
            Component::Normal(_) | Component::CurDir => relative.push(component.as_os_str()),
            _ => return None,
        }
    }
    Some(if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    })
}

fn lexically_normalized_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component);
                }
            }
            _ => normalized.push(component),
        }
    }
    normalized
}

fn staged_relative_path(stage: &Path, path: &Path) -> Option<PathBuf> {
    let stage = lexically_normalized_path(stage);
    let path = lexically_normalized_path(path);
    path.strip_prefix(&stage)
        .or_else(|_| path.strip_prefix(file::desymlink_path(&stage)))
        .ok()
        .map(Path::to_path_buf)
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
    target_app_dir()
}

fn link_binary(
    caskroom: &Path,
    cask: &Cask,
    apps: &[AppArtifact],
    appdir: &Path,
    binary: &BinaryArtifact,
) -> Result<()> {
    let source = if binary.source.starts_with("$APPDIR/") {
        binary_appdir_artifact_source(&binary.source, apps)?.ok_or_else(|| {
            eyre!(
                "brew-cask: binary APPDIR artifact '{}' is missing after app activation",
                binary.source
            )
        })?
    } else {
        let source = find_binary_source(caskroom, caskroom, cask, binary)?;
        if !source.is_file() {
            if source
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                let target = std::fs::read_link(&source)?;
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
        source
    };
    if source
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && !source.exists()
    {
        bail!("brew-cask: binary artifact '{}' is dangling", binary.source);
    }
    let target = binary.target_path(appdir)?;
    if let Some(parent) = target.parent() {
        create_dir_all_elevating(parent)?;
    }
    make_symlink_elevating(&source, &target)?;
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

fn cask_artifacts(cask: &Cask) -> Result<CaskArtifacts> {
    parse_cask_artifacts(cask, true)
}

fn parse_cask_artifacts(cask: &Cask, require_installable_artifact: bool) -> Result<CaskArtifacts> {
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
        if let Some(installer) = parse_installer_artifact(artifact)? {
            artifacts.installers.push(installer);
            continue;
        }
        if let Some(artifact) = parse_generic_artifact(artifact)? {
            artifacts.generic.push(artifact);
            continue;
        }
        if let Some(font) = parse_font_artifact(artifact) {
            artifacts.fonts.push(font);
            continue;
        }
        if let Some(manpage) = parse_manpage_artifact(artifact)? {
            artifacts.manpages.push(manpage);
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
    if require_installable_artifact
        && artifacts.apps.is_empty()
        && artifacts.binaries.is_empty()
        && artifacts.command_wrappers.is_empty()
        && artifacts.pkgs.is_empty()
        && artifacts.installers.is_empty()
        && artifacts.generic.is_empty()
        && artifacts.fonts.is_empty()
        && artifacts.manpages.is_empty()
        && artifacts.completions.is_empty()
        && artifacts.generated_completions.is_empty()
    {
        bail!(
            "brew-cask:{}: no supported install artifact found",
            cask.token
        );
    }
    artifacts.pkg_ids.sort();
    artifacts.pkg_ids.dedup();
    if require_installable_artifact {
        if artifacts.pkgs.is_empty() {
            artifacts.pkg_ids.clear();
        } else if artifacts.pkg_ids.is_empty() {
            bail!(
                "brew-cask:{}: pkg artifacts require pkgutil ids in uninstall metadata",
                cask.token
            );
        }
    }
    Ok(artifacts)
}

fn validate_platform_support(cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    validate_catalog_platform_support(cask)?;
    #[cfg(not(target_os = "macos"))]
    if !artifacts.pkgs.is_empty() {
        bail!(
            "brew-cask:{}: pkg artifacts are only available on macOS",
            cask.token
        );
    }
    if let Some(kind) = cask.artifacts.iter().find_map(|artifact| {
        let kind = artifact_type(artifact);
        matches!(
            kind.as_str(),
            "uninstall_preflight" | "uninstall_postflight"
        )
        .then_some(kind)
    }) {
        bail!(
            "brew-cask:{}: {kind} cannot be replayed from Homebrew JSON metadata",
            cask.token
        );
    }
    validate_cask_uninstall_plan(cask)?;
    if artifacts
        .preflight_steps
        .iter()
        .chain(&artifacts.postflight_steps)
        .any(|step| matches!(step, FlightStep::SetOwnership { .. }))
    {
        bail!(
            "brew-cask:{}: set_ownership is supported only in uninstall preflight steps",
            cask.token
        );
    }
    if artifacts
        .preflight_steps
        .iter()
        .chain(&artifacts.postflight_steps)
        .any(|step| matches!(step, FlightStep::Run { sudo: true, .. }))
    {
        bail!(
            "brew-cask:{}: structured sudo run steps are unsupported because elevation would escape process confinement",
            cask.token
        );
    }
    let dirs = configured_cask_dirs()?;
    for wrapper in &artifacts.command_wrappers {
        render_command_wrapper(&dirs.appdir, cask, wrapper)?;
    }
    Ok(())
}

fn requires_auxiliary_cask_receipt(
    auto_updates: bool,
    metadata_only_apps: &BTreeSet<PathBuf>,
    executed_flight_targets: &[PathBuf],
    executed_flight_directories: &[PathBuf],
) -> bool {
    auto_updates
        || !metadata_only_apps.is_empty()
        || !executed_flight_targets.is_empty()
        || !executed_flight_directories.is_empty()
}

fn validate_catalog_platform_support(cask: &Cask) -> Result<()> {
    if super::tag::host_arch() == super::tag::Architecture::Unsupported {
        bail!("brew-cask:{}: host architecture is unsupported", cask.token);
    }
    let supported = platform_policy_supports(
        &cask.platform_policy,
        super::tag::host_os(),
        super::tag::host_arch(),
        super::tag::host_macos_major(),
        &super::tag::host_tag(),
    );
    if !supported {
        bail!(
            "brew-cask:{}: catalog metadata does not support host platform {}",
            cask.token,
            super::tag::host_tag()
        );
    }
    Ok(())
}

fn platform_policy_supports(
    policy: &CaskPlatformPolicy,
    host_os: super::tag::OperatingSystem,
    host_arch: super::tag::Architecture,
    host_macos_major: Option<u32>,
    host_tag: &str,
) -> bool {
    if host_arch == super::tag::Architecture::Unsupported {
        return false;
    }
    match policy {
        CaskPlatformPolicy::Unspecified => true,
        CaskPlatformPolicy::PublicSupported(platforms) => platforms.contains(host_tag),
        CaskPlatformPolicy::Internal(requirements) => {
            let os_matches = requirements
                .required_os
                .is_none_or(|required| required == host_os);
            let arch_matches = requirements
                .arch
                .is_none_or(|required| required == host_arch);
            let version_matches = if requirements.macos_min.is_some()
                || requirements.macos_max.is_some()
                || requirements.macos_exact.is_some()
            {
                host_macos_major.is_some_and(|host| {
                    requirements.macos_min.is_none_or(|min| host >= min)
                        && requirements.macos_max.is_none_or(|max| host <= max)
                        && requirements
                            .macos_exact
                            .as_ref()
                            .is_none_or(|exact| exact.contains(&host))
                })
            } else {
                true
            };
            os_matches && arch_matches && version_matches
        }
    }
}

fn unsupported_package_state(cask: &Cask, artifacts: &CaskArtifacts) -> Option<PackageState> {
    validate_platform_support(cask, artifacts)
        .err()
        .map(|err| PackageState::unsupported(err.to_string()))
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
                .collect::<Result<IndexMap<_, _>>>()
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

fn parse_installer_artifact(value: &Value) -> Result<Option<InstallerArtifact>> {
    let Some(installer) = value.as_object().and_then(|object| object.get("installer")) else {
        return Ok(None);
    };
    let values = installer
        .as_array()
        .ok_or_else(|| eyre!("brew-cask: installer metadata must be an array"))?;
    let script = values
        .first()
        .and_then(Value::as_object)
        .and_then(|value| value.get("script"))
        .and_then(Value::as_object)
        .ok_or_else(|| eyre!("brew-cask: only script installers are supported"))?;
    reject_unsupported_artifact_fields("installer script", script, &["executable", "args"])?;
    let executable = script
        .get("executable")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("brew-cask: installer script requires an executable"))?;
    let args = script
        .get("args")
        .map(|args| {
            args.as_array()
                .ok_or_else(|| eyre!("brew-cask: installer script args must be an array"))?
                .iter()
                .map(|arg| {
                    arg.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| eyre!("brew-cask: installer script args must be strings"))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Some(InstallerArtifact {
        executable: executable.to_string(),
        args,
    }))
}

fn reject_unsupported_artifact_fields(
    context: &str,
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<()> {
    let unsupported = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        bail!(
            "brew-cask: unsupported {context} field {}",
            unsupported.join(", ")
        );
    }
    Ok(())
}

fn parse_generic_artifact(value: &Value) -> Result<Option<GenericArtifact>> {
    let Some(artifact) = value.as_object().and_then(|object| object.get("artifact")) else {
        return Ok(None);
    };
    let values = artifact
        .as_array()
        .ok_or_else(|| eyre!("brew-cask: artifact metadata must be an array"))?;
    let source = values
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("brew-cask: artifact requires a source"))?;
    let target = artifact_target(value, values)
        .ok_or_else(|| eyre!("brew-cask: artifact requires a target"))?;
    Ok(Some(GenericArtifact {
        source: source.to_string(),
        target,
    }))
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

fn parse_manpage_artifact(value: &Value) -> Result<Option<ManpageArtifact>> {
    let Some(manpage) = value.as_object().and_then(|object| object.get("manpage")) else {
        return Ok(None);
    };
    let source = match manpage {
        Value::String(source) => source,
        Value::Array(values) => values
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("brew-cask: manpage requires a source path"))?,
        _ => bail!("brew-cask: manpage metadata must be a string or array"),
    };
    let path = Path::new(source);
    if path.is_absolute()
        || path.components().next().is_none()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        bail!("brew-cask: manpage source '{source}' must be a contained relative path");
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("brew-cask: invalid manpage source '{source}'"))?;
    let base = filename.strip_suffix(".gz").unwrap_or(filename);
    let section = base
        .rsplit_once('.')
        .map(|(_, section)| section)
        .filter(|section| {
            section.len() == 1
                && section
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| matches!(byte, b'1'..=b'8' | b'n' | b'l'))
        })
        .ok_or_else(|| eyre!("brew-cask: '{source}' is not a valid man page name"))?;
    Ok(Some(ManpageArtifact {
        source: source.to_string(),
        section: section.to_string(),
    }))
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
        "copy" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "copy step",
                object,
                &[
                    "type",
                    "source",
                    "target",
                    "recursive",
                    "overwrite",
                    "source_glob",
                    "guards",
                ],
            )?;
            Ok(FlightStep::Copy {
                source: parse_context_flight_path_value(
                    cask,
                    kind,
                    "copy source",
                    object.get("source"),
                )?,
                target: parse_context_flight_path_value(
                    cask,
                    kind,
                    "copy target",
                    object.get("target"),
                )?,
                recursive: parse_optional_flight_bool(cask, kind, object, "recursive", false)?,
                overwrite: parse_optional_flight_bool(cask, kind, object, "overwrite", true)?,
                source_glob: parse_optional_flight_bool(cask, kind, object, "source_glob", false)?,
                guards: parse_flight_guards(cask, kind, object.get("guards"))?,
            })
        }
        "symlink" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "symlink step",
                object,
                &[
                    "type",
                    "source",
                    "target",
                    "force",
                    "uninstall",
                    "source_glob",
                    "sudo",
                    "guards",
                ],
            )?;
            Ok(FlightStep::Symlink {
                source: parse_context_flight_path_value(
                    cask,
                    kind,
                    "symlink source",
                    object.get("source"),
                )?,
                target: parse_context_flight_path_value(
                    cask,
                    kind,
                    "symlink target",
                    object.get("target"),
                )?,
                force: parse_optional_flight_bool(cask, kind, object, "force", false)?,
                uninstall: parse_optional_flight_bool(cask, kind, object, "uninstall", false)?,
                source_glob: parse_optional_flight_bool(cask, kind, object, "source_glob", false)?,
                sudo: parse_flight_sudo(cask, kind, object.get("sudo"))?,
                guards: parse_flight_guards(cask, kind, object.get("guards"))?,
            })
        }
        "run" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "run step",
                object,
                &[
                    "type",
                    "command",
                    "args",
                    "env",
                    "sudo",
                    "guards",
                    "network_access",
                ],
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
            for key in env.keys() {
                if !is_shell_env_name(key)
                    || matches!(key.as_str(), "HOME" | "PATH" | "TMPDIR" | "HOMEBREW_PREFIX")
                {
                    bail!(
                        "brew-cask:{}: unsupported {kind} run environment name {key}",
                        cask.token
                    );
                }
            }
            let guards = parse_flight_guards(cask, kind, object.get("guards"))?;
            Ok(FlightStep::Run {
                command: parse_run_command(cask, kind, object.get("command"))?,
                args,
                env,
                sudo: object.get("sudo").and_then(Value::as_bool).unwrap_or(false),
                network_access: object
                    .get("network_access")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                guards,
            })
        }
        "set_ownership" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "set_ownership step",
                object,
                &["type", "paths", "user", "group", "non_recursive"],
            )?;
            let paths = object
                .get("paths")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: unsupported {kind} set_ownership paths metadata format",
                        cask.token
                    )
                })?
                .iter()
                .map(|path| {
                    parse_context_flight_path_value(cask, kind, "set_ownership path", Some(path))
                })
                .collect::<Result<Vec<_>>>()?;
            let parse_name = |field: &str| -> Result<Option<String>> {
                match object.get(field) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::String(value)) if valid_ownership_name(value) => {
                        Ok(Some(value.clone()))
                    }
                    Some(_) => bail!(
                        "brew-cask:{}: {kind} set_ownership {field} is invalid",
                        cask.token
                    ),
                }
            };
            Ok(FlightStep::SetOwnership {
                paths,
                user: parse_name("user")?,
                group: parse_name("group")?,
                recursive: !parse_optional_flight_bool(cask, kind, object, "non_recursive", false)?,
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

fn parse_flight_sudo(cask: &Cask, kind: &str, value: Option<&Value>) -> Result<FlightSudo> {
    match value {
        None | Some(Value::Bool(false)) => Ok(FlightSudo::Never),
        Some(Value::Bool(true)) => Ok(FlightSudo::Always),
        Some(Value::String(value)) if value == "if_needed" => Ok(FlightSudo::IfNeeded),
        _ => bail!("brew-cask:{}: unsupported {kind} sudo setting", cask.token),
    }
}

fn parse_flight_guards(cask: &Cask, kind: &str, value: Option<&Value>) -> Result<Vec<FlightGuard>> {
    value
        .map(|guards| {
            guards
                .as_array()
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: unsupported {kind} guards metadata format",
                        cask.token
                    )
                })?
                .iter()
                .map(|guard| parse_flight_guard(cask, kind, guard))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()
        .map(|guards| guards.unwrap_or_default())
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
        None => FlightPathBase::Literal,
    };
    let path_value = Path::new(path);
    let invalid_absolute_path = base == FlightPathBase::Literal
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
        Some("relative") => FlightPathBase::Literal,
        Some(base) => bail!(
            "brew-cask:{}: unsupported {kind} {field} base {}",
            cask.token,
            base
        ),
        None => FlightPathBase::Literal,
    };
    Ok(FlightPath {
        base,
        path: path.to_string(),
    })
}

fn parse_context_flight_path_value(
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
    reject_unsupported_flight_fields(cask, kind, field, object, &["base", "path"])?;
    parse_context_flight_path(cask, kind, field, object)
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
    if let Some(found) = case_insensitive {
        return Some(found);
    }
    // `WalkDir` defaults to `follow_links = false`, so the walk above never
    // descends into a symlink a flight step created: gcloud-cli's last
    // preflight step links `staged_path/google-cloud-sdk` at the SDK it copied
    // into the prefix, and every `binary` beneath it was unreachable. Resolving
    // `name` as an exact path under `root` traverses the link.
    //
    // This only ever fires for that symlinked case. When `root/name` is
    // reachable without traversing a link, its own relative path ends with
    // `name`, so the walk already returned it — which is also why the result is
    // desymlinked: the artifact's real location is what callers need to tell
    // ephemeral stage content apart from a durable directory that outlives the
    // install. Kept as a fallback rather than a fast path so the walk's
    // exact-then-case-insensitive precedence is unchanged on case-insensitive
    // filesystems.
    relative_artifact_path(root, name_path)
        .filter(|path| pred(path))
        .map(|path| file::desymlink_path(&path))
}

/// `name` resolved against `root`, or `None` when `name` cannot be interpreted
/// as a path contained by `root`.
fn relative_artifact_path(root: &Path, name: &Path) -> Option<PathBuf> {
    if name.is_absolute() {
        return None;
    }
    let mut named = false;
    for component in name.components() {
        match component {
            // `.` alone would resolve to `root` itself, and `install_app` would
            // then take the whole extraction root as the bundle.
            Component::Normal(component) if component != "__MACOSX" => named = true,
            // The walk skips `__MACOSX` resource-fork copies; an exact-path hit
            // must not reintroduce them.
            Component::Normal(_) => return None,
            Component::CurDir => {}
            _ => return None,
        }
    }
    named.then(|| root.join(name))
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
    let app_dir = target_app_dir()?;
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
            if path.starts_with(&app_dir) || path.starts_with(&prefix_app_dir) {
                return Ok(path);
            }
            // Casks routinely hardcode an absolute `/Applications/Foo.app`
            // target. When an override appdir is configured, relocate such a
            // target into it (preserving any subdirectories) rather than
            // rejecting it. `$HOMEBREW_PREFIX`-anchored targets are handled by
            // the check above and are never relocated.
            if app_dir != Path::new(DEFAULT_APP_DIR)
                && let Ok(rest) = path.strip_prefix(DEFAULT_APP_DIR)
            {
                return Ok(app_dir.join(rest));
            }
            bail!(
                "brew-cask: app target '{target_name}' must be under {}",
                app_dir.display()
            );
        }
        bail!("brew-cask: app target '{target_name}' must be an absolute path");
    }
    Ok(app_dir.join(target_name))
}

/// The directory `app` artifacts are linked into: Homebrew's platform default
/// unless [`APP_DIR_ENV`] overrides it.
///
/// The override is validated here rather than at the point of use because
/// `app_target_path` treats the result as a containment boundary for symlinks
/// that may be created with elevated privileges. An empty value falls back to
/// the default so that exporting `MISE_BREW_CASK_OPT_APPDIR=` cannot disable
/// that boundary: `Path::starts_with("")` is true for every path.
fn target_app_dir() -> Result<PathBuf> {
    let default = EffectiveCaskDirs::current().appdir;
    let Ok(dir) = crate::env::var(APP_DIR_ENV) else {
        return Ok(default);
    };
    if dir.is_empty() {
        return Ok(default);
    }
    let dir = PathBuf::from(dir);
    if !dir.is_absolute() {
        bail!(
            "brew-cask: {APP_DIR_ENV} '{}' must be an absolute path",
            dir.display()
        );
    }
    if dir
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "brew-cask: {APP_DIR_ENV} '{}' must not contain '..'",
            dir.display()
        );
    }
    // Resolve the override to a real absolute path: canonicalize its longest
    // existing prefix and re-append the components that do not exist yet. This
    // makes the appdir a symlink-free containment boundary — privileged cask
    // mutations then operate on resolved paths and cannot be redirected through
    // a symlinked component — and it collapses every spelling of the filesystem
    // root (`/`, `//`, `/.`, a symlink to `/`, ...) to `/` so they can all be
    // rejected together.
    let resolved = resolve_appdir(&dir);
    if !resolved
        .components()
        .any(|component| matches!(component, Component::Normal(_)))
    {
        bail!(
            "brew-cask: {APP_DIR_ENV} '{}' must not resolve to the filesystem root",
            dir.display()
        );
    }
    Ok(resolved)
}

/// Resolve `dir` by canonicalizing its longest existing ancestor and
/// re-appending the not-yet-existing tail. Symlinks in the existing portion are
/// followed, so the result is a real path the caller can safely use as a
/// containment boundary. Falls back to `dir` unchanged if nothing along the
/// path can be canonicalized (not expected for an absolute path, where `/`
/// always resolves).
fn resolve_appdir(dir: &Path) -> PathBuf {
    for ancestor in dir.ancestors() {
        if let Ok(real) = ancestor.canonicalize() {
            let tail = dir.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return if tail.as_os_str().is_empty() {
                real
            } else {
                real.join(tail)
            };
        }
    }
    dir.to_path_buf()
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

fn allowed_appdir_roots() -> Result<Vec<PathBuf>> {
    let mut roots = vec![EffectiveCaskDirs::current().appdir];
    for root in [target_app_dir()?, prefix::prefix().join("Applications")] {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    Ok(roots)
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
        if !allowed_appdir_roots()?.iter().any(|root| root == appdir) {
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
    let versions = installed_versions(token);
    match versions.as_slice() {
        [version] => Some(version.clone()),
        [] => None,
        _ => {
            warn!("brew-cask:{token}: multiple Caskroom versions found; reinstall to reconcile");
            None
        }
    }
}

fn installed_versions(token: &str) -> Vec<String> {
    let dir = caskroom_token_dir(token);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            entry
                .file_type()
                .ok()
                .filter(|ft| ft.is_dir() && name != ".metadata" && !name.starts_with(".mise-tmp-"))
                .map(|_| name)
        })
        .collect()
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
    let output = std::process::Command::new("/usr/sbin/pkgutil")
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

#[cfg(target_os = "macos")]
fn prepare_pkg_removal_plans(pkg_id: &str) -> Result<Vec<PkgRemovalPlan>> {
    if !valid_bundle_identifier(pkg_id) {
        bail!("brew-cask: unsupported pkgutil identifier: {pkg_id}");
    }
    let output = std::process::Command::new("/usr/sbin/pkgutil")
        .arg(format!("--pkgs={pkg_id}"))
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        bail!("brew-cask: pkgutil could not list package receipt {pkg_id}");
    }
    String::from_utf8(output.stdout)?
        .lines()
        .filter(|line| !line.is_empty())
        .map(|installed_id| {
            if installed_id != pkg_id || !valid_bundle_identifier(installed_id) {
                bail!("brew-cask: pkgutil identifier {pkg_id} unexpectedly matched {installed_id}");
            }
            prepare_installed_pkg_removal(installed_id)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn prepare_pkg_removal_plans(pkg_id: &str) -> Result<Vec<PkgRemovalPlan>> {
    bail!("brew-cask: pkgutil teardown for {pkg_id} is only available on macOS")
}

#[cfg(target_os = "macos")]
fn prepare_installed_pkg_removal(package_id: &str) -> Result<PkgRemovalPlan> {
    let info = std::process::Command::new("/usr/sbin/pkgutil")
        .args(["--pkg-info-plist", package_id])
        .stdin(Stdio::null())
        .output()?;
    if !info.status.success() {
        bail!("brew-cask: pkgutil could not read package receipt {package_id}");
    }
    let root = pkg_root_from_info(&info.stdout)?;
    let bom = std::process::Command::new("/usr/sbin/pkgutil")
        .args(["--files", package_id])
        .stdin(Stdio::null())
        .output()?;
    if !bom.status.success() {
        bail!("brew-cask: pkgutil could not read package BOM {package_id}");
    }
    pkg_removal_plan_from_bom(package_id, root, &String::from_utf8(bom.stdout)?)
}

#[cfg(any(target_os = "macos", test))]
fn pkg_root_from_info(xml: &[u8]) -> Result<PathBuf> {
    let value = plist::Value::from_reader_xml(xml)?;
    let dictionary = value
        .as_dictionary()
        .ok_or_else(|| eyre!("brew-cask: pkgutil info is not a plist dictionary"))?;
    let volume = dictionary
        .get("volume")
        .and_then(plist::Value::as_string)
        .ok_or_else(|| eyre!("brew-cask: pkgutil info has no volume"))?;
    if volume != "/" {
        bail!("brew-cask: package receipt volume is unsupported: {volume}");
    }
    let install_location = dictionary
        .get("install-location")
        .and_then(plist::Value::as_string)
        .ok_or_else(|| eyre!("brew-cask: pkgutil info has no install-location"))?;
    let install_location = Path::new(install_location);
    if install_location
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        bail!("brew-cask: package install location is not normalized");
    }
    Ok(Path::new("/").join(
        install_location
            .strip_prefix("/")
            .unwrap_or(install_location),
    ))
}

#[cfg(any(target_os = "macos", test))]
fn pkg_removal_plan_from_bom(package_id: &str, root: PathBuf, bom: &str) -> Result<PkgRemovalPlan> {
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("brew-cask: package receipt root is not absolute and normalized");
    }
    let mut files = Vec::new();
    let mut specials = Vec::new();
    let mut directories = Vec::new();
    let mut all_paths = BTreeSet::new();
    for entry in bom.lines().filter(|line| !line.is_empty()) {
        let relative = Path::new(entry);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            bail!("brew-cask: package BOM path is not relative and normalized: {entry}");
        }
        let path = root.join(relative);
        if !path.starts_with(&root) {
            bail!("brew-cask: package BOM path escapes its receipt root: {entry}");
        }
        all_paths.insert(path.clone());
        if cask_path_is_undeletable(&path) {
            continue;
        }
        let Ok(metadata) = path.symlink_metadata() else {
            continue;
        };
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            specials.push(path);
        } else if metadata.is_file() {
            files.push(path);
        } else {
            directories.push(path);
        }
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    Ok(PkgRemovalPlan {
        package_id: package_id.to_string(),
        root,
        files,
        specials,
        directories,
        all_paths,
    })
}

fn cask_path_is_undeletable(path: &Path) -> bool {
    let home = Path::new(&*crate::dirs::HOME);
    let brew_prefix = prefix::prefix();
    [
        Path::new("/"),
        Path::new("/Applications"),
        Path::new("/Library"),
        Path::new("/Network"),
        Path::new("/System"),
        Path::new("/Users"),
        Path::new("/Volumes"),
        Path::new("/bin"),
        Path::new("/etc"),
        Path::new("/private"),
        Path::new("/sbin"),
        Path::new("/usr"),
        Path::new("/var"),
        home,
        &brew_prefix,
    ]
    .contains(&path)
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

fn cask_target_plan(cask: &Cask, artifacts: &CaskArtifacts) -> Result<CaskTargetPlan> {
    let mut artifact_activation_targets = artifacts
        .apps
        .iter()
        .map(|app| app_target_path(app.target_name()))
        .collect::<Result<Vec<_>>>()?;
    artifact_activation_targets.extend(font_target_paths(artifacts)?);
    artifact_activation_targets.extend(binary_targets(artifacts)?);
    artifact_activation_targets.extend(manpage_target_paths(artifacts)?);
    artifact_activation_targets.extend(completion_target_paths(cask, artifacts)?);
    reject_duplicate_cask_targets(cask, &artifact_activation_targets)?;
    dedup_paths_preserving_order(&mut artifact_activation_targets);
    let mut receipt_inventory_targets = artifact_activation_targets.clone();
    receipt_inventory_targets.extend(generic_artifact_targets(artifacts)?);
    reject_duplicate_cask_targets(cask, &receipt_inventory_targets)?;
    dedup_paths_preserving_order(&mut receipt_inventory_targets);

    Ok(CaskTargetPlan {
        artifact_activation_targets,
        receipt_inventory_targets,
    })
}

fn reject_duplicate_cask_targets(cask: &Cask, targets: &[PathBuf]) -> Result<()> {
    let mut seen = BTreeSet::new();
    if let Some(target) = targets.iter().find(|target| !seen.insert(target.as_path())) {
        bail!(
            "brew-cask:{}: multiple artifacts claim target {}",
            cask.token,
            target.display()
        );
    }
    Ok(())
}

fn planned_flight_activation_targets(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    staged_path: &Path,
    appdir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut targets = generic_artifact_targets(artifacts)?;
    for step in artifacts
        .preflight_steps
        .iter()
        .chain(&artifacts.postflight_steps)
    {
        match step {
            FlightStep::Copy { target, guards, .. } => {
                if !guards
                    .iter()
                    .map(|guard| flight_guard_matches(cask, guard, staged_path, appdir))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .all(|matches| matches)
                {
                    continue;
                }
                let target = resolve_flight_path_with_context(cask, target, staged_path, appdir)?;
                if !target.starts_with(staged_path) {
                    targets.push(target);
                }
            }
            FlightStep::Symlink {
                source,
                target,
                source_glob,
                guards,
                ..
            } => {
                if !guards
                    .iter()
                    .map(|guard| flight_guard_matches(cask, guard, staged_path, appdir))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .all(|matches| matches)
                {
                    continue;
                }
                let sources =
                    flight_symlink_sources(cask, source, *source_glob, staged_path, appdir)?;
                if sources.is_empty() {
                    bail!(
                        "brew-cask:{}: structured symlink preflight matched no sources",
                        cask.token
                    );
                }
                let target = resolve_flight_path_with_context(cask, target, staged_path, appdir)?;
                let target_is_dir = target.is_dir() || sources.len() > 1;
                if sources.len() > 1 && !target.starts_with(staged_path) {
                    targets.push(target.clone());
                }
                for source in sources {
                    let link = if target_is_dir {
                        target.join(source.file_name().ok_or_else(|| {
                            eyre!("brew-cask: structured symlink source has no file name")
                        })?)
                    } else {
                        target.clone()
                    };
                    if !link.starts_with(staged_path) {
                        targets.push(link);
                    }
                }
            }
            FlightStep::Move { .. }
            | FlightStep::Remove { .. }
            | FlightStep::Run { .. }
            | FlightStep::TerminateProcess { .. }
            | FlightStep::SetOwnership { .. } => {}
        }
    }
    Ok(targets)
}

fn dedup_paths_preserving_order(paths: &mut Vec<PathBuf>) {
    let mut seen = BTreeSet::new();
    paths.retain(|path| seen.insert(path.clone()));
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

#[cfg(test)]
fn remove_obsolete_flight_directories(
    previous: &BTreeSet<PathBuf>,
    current: &[PathBuf],
) -> Result<()> {
    for directory in previous {
        if !current.contains(directory) {
            remove_empty_directory_elevating(directory)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn receipt_flight_symlink_targets(receipt: &CaskReceipt) -> Result<Vec<PathBuf>> {
    let standard_targets = receipt
        .apps
        .iter()
        .chain(&receipt.binaries)
        .chain(&receipt.fonts)
        .chain(&receipt.completions)
        .collect::<BTreeSet<_>>();
    let mut targets = Vec::new();
    for record in &receipt.targets {
        if record.fingerprint.kind == CaskTargetKind::Symlink
            && !standard_targets.contains(&record.path)
            && record.uninstall.unwrap_or(true)
            && cask_target_record_matches(record)?
        {
            targets.push(record.path.clone());
        }
    }
    Ok(targets)
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

#[cfg(test)]
fn installed_cask_version(cask: &Cask, artifacts: &CaskArtifacts) -> Result<Option<String>> {
    Ok(
        installed_cask_state_in(cask, artifacts, &prefix::prefix().join(".mise-test-state"))?
            .version(),
    )
}

#[cfg(not(test))]
fn installed_cask_state(cask: &Cask, artifacts: &CaskArtifacts) -> Result<InstalledCaskState> {
    installed_cask_state_in(cask, artifacts, &crate::dirs::STATE)
}

#[cfg(test)]
fn installed_cask_state(cask: &Cask, artifacts: &CaskArtifacts) -> Result<InstalledCaskState> {
    installed_cask_state_in(cask, artifacts, &prefix::prefix().join(".mise-test-state"))
}

#[cfg(not(test))]
fn installed_cask_state_for_token(token: &str) -> Result<InstalledCaskState> {
    installed_cask_state_for_token_in(token, &crate::dirs::STATE)
}

#[cfg(test)]
fn installed_cask_state_for_token(token: &str) -> Result<InstalledCaskState> {
    installed_cask_state_for_token_in(token, &prefix::prefix().join(".mise-test-state"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstalledCaskState {
    Installed(String),
    LegacyMise(Box<CaskReceipt>),
    Absent,
    NeedsRepair {
        installed: Option<String>,
        reason: String,
        /// A complete native receipt proved the predecessor's ownership and
        /// teardown vocabulary. Apply may replace that damaged installation;
        /// malformed, legacy, or interrupted state must still fail closed.
        replacement_safe: bool,
    },
}

fn existing_install_noop(
    state: &InstalledCaskState,
    cask: &Cask,
    upgrading: bool,
) -> Option<String> {
    match state {
        InstalledCaskState::Installed(version)
            if !upgrading || cask.auto_updates || version == &cask.version =>
        {
            Some(version.clone())
        }
        InstalledCaskState::Installed(_)
        | InstalledCaskState::LegacyMise(_)
        | InstalledCaskState::Absent
        | InstalledCaskState::NeedsRepair { .. } => None,
    }
}

#[cfg(test)]
impl InstalledCaskState {
    fn version(self) -> Option<String> {
        match self {
            Self::Installed(version) => Some(version),
            Self::LegacyMise(receipt) => Some(receipt.version),
            Self::Absent | Self::NeedsRepair { .. } => None,
        }
    }
}

fn installed_cask_state_in(
    cask: &Cask,
    _artifacts: &CaskArtifacts,
    state_dir: &Path,
) -> Result<InstalledCaskState> {
    installed_cask_state_for_token_in(&cask.token, state_dir)
}

fn installed_cask_state_for_token_in(token: &str, state_dir: &Path) -> Result<InstalledCaskState> {
    if let Some(reason) = pending_cask_transaction_reason_in(state_dir, token) {
        return Ok(InstalledCaskState::NeedsRepair {
            installed: installed_version(token),
            reason,
            replacement_safe: false,
        });
    }
    let token_dir = caskroom_token_dir(token);
    if token_dir.join(".metadata").symlink_metadata().is_ok() {
        return Ok(match super::receipt::read_cask_receipt(&token_dir) {
            Ok(receipt) => {
                let version = receipt.source.version.clone();
                let version_dir = caskroom_version_dir(token, &version);
                if !version_dir.is_dir() {
                    let replacement_safe =
                        homebrew_cask_replacement_safe(token, &receipt, &version_dir);
                    InstalledCaskState::NeedsRepair {
                        installed: Some(version.clone()),
                        reason: format!(
                            "brew-cask:{}: Homebrew receipt records version {version}, but {} is missing",
                            token,
                            version_dir.display()
                        ),
                        replacement_safe,
                    }
                } else if let Err(err) =
                    validate_installed_homebrew_cask_topology(token, &receipt, &version_dir)
                {
                    let replacement_safe =
                        homebrew_cask_replacement_safe(token, &receipt, &version_dir);
                    InstalledCaskState::NeedsRepair {
                        installed: Some(version),
                        reason: format!(
                            "brew-cask:{}: installed artifact topology is incomplete: {err:#}",
                            token
                        ),
                        replacement_safe,
                    }
                } else {
                    InstalledCaskState::Installed(version)
                }
            }
            Err(err) => InstalledCaskState::NeedsRepair {
                installed: installed_version(token),
                reason: format!("brew-cask:{token}: {err}"),
                replacement_safe: false,
            },
        });
    }
    let Some(version) = installed_version(token) else {
        return Ok(InstalledCaskState::Absent);
    };
    let version_dir = caskroom_version_dir(token, &version);
    let legacy_receipt = match read_receipt(&version_dir) {
        Ok(receipt) => receipt,
        Err(err) => {
            return Ok(InstalledCaskState::NeedsRepair {
                installed: Some(version),
                reason: format!(
                    "brew-cask:{}: legacy mise receipt cannot be parsed ({err}); reinstall with either 'brew install --cask {}' or mise apply after uninstalling",
                    token, token
                ),
                replacement_safe: false,
            });
        }
    };
    match legacy_receipt {
        Some(receipt) => {
            if receipt.schema_version > 3 {
                return Ok(legacy_needs_repair_for_token(
                    token,
                    &receipt.version,
                    "receipt schema is unsupported",
                ));
            }
            if receipt.schema_version < 2
                || (receipt.targets.is_empty() && receipt.pkg_ids.is_empty())
            {
                return Ok(legacy_needs_repair_for_token(
                    token,
                    &receipt.version,
                    "receipt has no provable target or package ownership evidence",
                ));
            }
            Ok(InstalledCaskState::LegacyMise(Box::new(receipt)))
        }
        None => Ok(InstalledCaskState::Absent),
    }
}

#[cfg(test)]
fn validate_installed_cask_topology(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    version_dir: &Path,
) -> Result<()> {
    validate_installed_cask_topology_with_metadata(
        cask,
        artifacts,
        version_dir,
        false,
        &BTreeSet::new(),
        None,
    )
}

fn validate_installed_cask_topology_with_metadata(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    version_dir: &Path,
    auto_updates: bool,
    metadata_only_apps: &BTreeSet<PathBuf>,
    recorded_targets: Option<&[CaskTargetRecord]>,
) -> Result<()> {
    for app in &artifacts.apps {
        let target = app_target_path(app.target_name())?;
        let backlink = caskroom_artifact_path(version_dir, &app.source, "app")?;
        if metadata_only_apps.contains(&target) {
            if !target.is_dir() {
                bail!("tracked app is missing: {}", target.display());
            }
            if let Ok(metadata) = backlink.symlink_metadata()
                && (!metadata.file_type().is_symlink() || !file::same_file(&backlink, &target))
            {
                bail!("tracked app has an invalid backlink: {}", target.display());
            }
            if !auto_updates {
                let record = recorded_targets
                    .and_then(|targets| targets.iter().find(|record| record.path == target))
                    .ok_or_else(|| {
                        eyre!(
                            "tracked app has no ownership fingerprint: {}",
                            target.display()
                        )
                    })?;
                if !cask_target_present(record) {
                    bail!("tracked app is missing: {}", target.display());
                }
            }
            continue;
        }
        if !target.is_dir()
            || !backlink
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            || !file::same_file(&backlink, &target)
        {
            bail!(
                "moved app is missing or has an invalid backlink: {}",
                target.display()
            );
        }
    }
    for font in &artifacts.fonts {
        let target = font_target_path(font)?;
        let backlink = caskroom_font_path(version_dir, font)?;
        if !target.is_file()
            || !backlink
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            || !file::same_file(&backlink, &target)
        {
            bail!(
                "moved font is missing or has an invalid backlink: {}",
                target.display()
            );
        }
    }
    let appdir = cask_appdir(&artifacts.apps)?;
    for binary in &artifacts.binaries {
        let target = binary.target_path(&appdir)?;
        let owned = if binary.source.starts_with("$APPDIR/") {
            declared_appdir_binary_symlink_is_owned(&binary.source, &artifacts.apps, &target)?
        } else {
            symlink_resolves_below(&target, version_dir)
        };
        if !owned {
            bail!(
                "binary is not an owned declared-source symlink: {}",
                target.display()
            );
        }
    }
    for wrapper in &artifacts.command_wrappers {
        let target = wrapper.target_path()?;
        if !symlink_resolves_below(&target, version_dir) {
            bail!(
                "wrapper is not an owned Caskroom symlink: {}",
                target.display()
            );
        }
    }
    for target in completion_target_paths(cask, artifacts)? {
        if !completion_target_is_owned(cask, artifacts, &target, version_dir)? {
            bail!(
                "completion is not an owned declared-source symlink: {}",
                target.display()
            );
        }
    }
    for manpage in &artifacts.manpages {
        let target = manpage_target_path(manpage)?;
        if !manpage_target_is_owned(manpage, &artifacts.apps, &target, version_dir)? {
            bail!(
                "manpage is not an owned declared-source symlink: {}",
                target.display()
            );
        }
    }
    for artifact in &artifacts.generic {
        let target = generic_artifact_target_path(&artifact.target)?;
        if !generic_artifact_is_owned(version_dir, artifact, &target)? {
            bail!(
                "generic artifact is missing or has an invalid Caskroom backlink: {}",
                target.display()
            );
        }
    }
    for record in structured_symlink_target_records(cask, artifacts, version_dir, false)? {
        if !structured_symlink_target_is_owned(&record.path, version_dir, &artifacts.apps)? {
            bail!(
                "structured symlink is missing or has an invalid source: {}",
                record.path.display()
            );
        }
    }
    structured_copy_target_records(cask, artifacts, version_dir, false)?;
    if !pkg_ids_installed(&artifacts.pkg_ids)? {
        bail!("one or more recorded package receipts are missing");
    }
    Ok(())
}

fn declared_appdir_symlink_is_owned(
    source: &str,
    apps: &[AppArtifact],
    target: &Path,
) -> Result<bool> {
    let Some(source) = appdir_artifact_source(source, apps)? else {
        return Ok(false);
    };
    Ok(target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && file::same_file(target, &source))
}

fn declared_appdir_binary_symlink_is_owned(
    source: &str,
    apps: &[AppArtifact],
    target: &Path,
) -> Result<bool> {
    let Some(source) = binary_appdir_artifact_source(source, apps)? else {
        return Ok(false);
    };
    Ok(target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && file::same_file(target, &source))
}

fn native_binary_target_is_owned(
    artifacts: &CaskArtifacts,
    target: &Path,
    version_dir: &Path,
) -> Result<bool> {
    let appdir = cask_appdir(&artifacts.apps)?;
    let mut claims = Vec::new();
    for binary in &artifacts.binaries {
        if binary.target_path(&appdir)? != target {
            continue;
        }
        claims.push(if binary.source.starts_with("$APPDIR/") {
            declared_appdir_binary_symlink_is_owned(&binary.source, &artifacts.apps, target)?
        } else {
            symlink_resolves_below(target, version_dir)
        });
    }
    for wrapper in &artifacts.command_wrappers {
        if wrapper.target_path()? == target {
            claims.push(symlink_resolves_below(target, version_dir));
        }
    }
    match claims.as_slice() {
        [owned] => Ok(*owned),
        [] => bail!(
            "missing native binary or command-wrapper artifact for {}",
            target.display()
        ),
        _ => bail!("ambiguous native binary target claim: {}", target.display()),
    }
}

fn manpage_target_is_owned(
    manpage: &ManpageArtifact,
    apps: &[AppArtifact],
    target: &Path,
    version_dir: &Path,
) -> Result<bool> {
    if manpage.source.starts_with("$APPDIR/") {
        declared_appdir_symlink_is_owned(&manpage.source, apps, target)
    } else {
        Ok(symlink_resolves_below(target, version_dir))
    }
}

fn completion_target_is_owned(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    target: &Path,
    version_dir: &Path,
) -> Result<bool> {
    let mut declared = artifacts
        .completions
        .iter()
        .filter(|completion| completion.target_path().is_ok_and(|path| path == target));
    let completion = declared.next();
    if completion.is_some() && declared.next().is_some() {
        bail!(
            "brew-cask:{}: multiple completion artifacts claim '{}'",
            cask.token,
            target.display()
        );
    }
    let generated = completion_target_is_generated(cask, artifacts, target)?;
    if usize::from(completion.is_some()) + usize::from(generated) != 1 {
        bail!(
            "brew-cask:{}: completion target '{}' has ambiguous artifact ownership",
            cask.token,
            target.display()
        );
    }
    match completion {
        Some(completion) if completion.source.starts_with("$APPDIR/") => {
            declared_appdir_symlink_is_owned(&completion.source, &artifacts.apps, target)
        }
        Some(completion) => {
            let Some(source) = find_completion_source(
                version_dir,
                version_dir,
                cask,
                &artifacts.apps,
                &completion.source,
            )?
            else {
                return Ok(false);
            };
            Ok(target
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
                && file::same_file(target, &source))
        }
        None => Ok(target
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_file())),
    }
}

fn completion_target_is_generated(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    target: &Path,
) -> Result<bool> {
    let mut generated = false;
    for completion in &artifacts.generated_completions {
        if completion
            .target_paths(cask)?
            .iter()
            .any(|candidate| candidate == target)
        {
            if generated {
                bail!(
                    "brew-cask:{}: multiple generated completions claim '{}'",
                    cask.token,
                    target.display()
                );
            }
            generated = true;
        }
    }
    Ok(generated)
}

fn validate_installed_homebrew_cask_topology(
    token: &str,
    receipt: &receipt::CaskReceipt,
    version_dir: &Path,
) -> Result<()> {
    let installed = cask_from_homebrew_receipt(token, receipt);
    let artifacts = parse_cask_artifacts(&installed, false)?;
    validate_homebrew_uninstall_artifacts(token, receipt)?;
    let token_dir = version_dir
        .parent()
        .ok_or_else(|| eyre!("brew-cask:{token}: cask version has no token directory"))?;
    validate_homebrew_cask_config(token, token_dir, receipt, &artifacts)?;
    let auxiliary = read_auxiliary_cask_receipt(token, &receipt.source.version)?;
    let auto_updates = auxiliary
        .as_ref()
        .is_some_and(|receipt| receipt.auto_updates);
    let metadata_only_apps = auxiliary
        .as_ref()
        .map(|receipt| receipt.metadata_only_apps.iter().cloned().collect())
        .unwrap_or_default();
    validate_installed_cask_topology_with_metadata(
        &installed,
        &artifacts,
        version_dir,
        auto_updates,
        &metadata_only_apps,
        auxiliary.as_ref().map(|receipt| receipt.targets.as_slice()),
    )
}

fn homebrew_cask_replacement_safe(
    token: &str,
    receipt: &receipt::CaskReceipt,
    version_dir: &Path,
) -> bool {
    if read_auxiliary_cask_receipt(token, &receipt.source.version).is_err() {
        return false;
    }
    let installed = cask_from_homebrew_receipt(token, receipt);
    let Ok(artifacts) = parse_cask_artifacts(&installed, false) else {
        return false;
    };
    if validate_homebrew_uninstall_artifacts(token, receipt).is_err() {
        return false;
    }
    let Some(token_dir) = version_dir.parent() else {
        return false;
    };
    if validate_homebrew_cask_config(token, token_dir, receipt, &artifacts).is_err() {
        return false;
    }
    surviving_cask_artifacts_are_owned(&installed, &artifacts, version_dir).unwrap_or(false)
}

/// Missing receipt-declared targets are repairable by a full transactional
/// replacement. Any surviving target must still prove that the installed
/// receipt owns it; otherwise replacement could overwrite foreign state.
fn replacement_target_metadata(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match path.symlink_metadata() {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).wrap_err_with(|| {
            format!(
                "failed to prove whether replacement target exists: {}",
                path.display()
            )
        }),
    }
}

fn surviving_cask_artifacts_are_owned(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    version_dir: &Path,
) -> Result<bool> {
    for app in &artifacts.apps {
        let target = app_target_path(app.target_name())?;
        let backlink = caskroom_artifact_path(version_dir, &app.source, "app")?;
        if replacement_target_metadata(&target)?.is_none() {
            if replacement_target_metadata(&backlink)?.is_some()
                && !symlink_declares_target(&backlink, &target)
            {
                return Ok(false);
            }
            continue;
        }
        if !target.is_dir()
            || !replacement_target_metadata(&backlink)?
                .is_some_and(|metadata| metadata.file_type().is_symlink())
            || !file::same_file(&backlink, &target)
        {
            return Ok(false);
        }
    }
    for font in &artifacts.fonts {
        let target = font_target_path(font)?;
        let backlink = caskroom_font_path(version_dir, font)?;
        if replacement_target_metadata(&target)?.is_none() {
            if replacement_target_metadata(&backlink)?.is_some()
                && !symlink_declares_target(&backlink, &target)
            {
                return Ok(false);
            }
            continue;
        }
        if !target.is_file()
            || !replacement_target_metadata(&backlink)?
                .is_some_and(|metadata| metadata.file_type().is_symlink())
            || !file::same_file(&backlink, &target)
        {
            return Ok(false);
        }
    }
    let appdir = cask_appdir(&artifacts.apps)?;
    for binary in &artifacts.binaries {
        let target = binary.target_path(&appdir)?;
        if replacement_target_metadata(&target)?.is_none() {
            continue;
        }
        let owned = if binary.source.starts_with("$APPDIR/") {
            let Some(source) =
                declared_binary_appdir_artifact_source(&binary.source, &artifacts.apps)?
            else {
                return Ok(false);
            };
            symlink_declares_target(&target, &source)
        } else {
            symlink_resolves_below(&target, version_dir)
        };
        if !owned {
            return Ok(false);
        }
    }
    for wrapper in &artifacts.command_wrappers {
        let target = wrapper.target_path()?;
        if replacement_target_metadata(&target)?.is_some()
            && !symlink_resolves_below(&target, version_dir)
        {
            return Ok(false);
        }
    }
    for target in completion_target_paths(cask, artifacts)? {
        if replacement_target_metadata(&target)?.is_some()
            && !completion_target_is_owned(cask, artifacts, &target, version_dir)?
        {
            return Ok(false);
        }
    }
    for manpage in &artifacts.manpages {
        let target = manpage_target_path(manpage)?;
        if replacement_target_metadata(&target)?.is_some()
            && !manpage_target_is_owned(manpage, &artifacts.apps, &target, version_dir)?
        {
            return Ok(false);
        }
    }
    for artifact in &artifacts.generic {
        let target = generic_artifact_target_path(&artifact.target)?;
        if replacement_target_metadata(&target)?.is_some()
            && !generic_artifact_is_owned(version_dir, artifact, &target)?
        {
            return Ok(false);
        }
    }
    for record in structured_symlink_target_records(cask, artifacts, version_dir, true)? {
        if replacement_target_metadata(&record.path)?.is_some()
            && !structured_symlink_target_is_owned(&record.path, version_dir, &artifacts.apps)?
        {
            return Ok(false);
        }
    }
    if structured_copy_target_records(cask, artifacts, version_dir, true).is_err() {
        return Ok(false);
    }
    Ok(artifacts.pkg_ids.is_empty())
}

fn generic_artifact_is_owned(
    version_dir: &Path,
    artifact: &GenericArtifact,
    target: &Path,
) -> Result<bool> {
    let Some(backlink) = find_artifact_matching(version_dir, &artifact.source, |_| true) else {
        return Ok(false);
    };
    Ok(target.symlink_metadata().is_ok()
        && backlink
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && file::same_file(&backlink, target))
}

fn structured_symlink_target_is_owned(
    target: &Path,
    version_dir: &Path,
    apps: &[AppArtifact],
) -> Result<bool> {
    if !target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Ok(false);
    }
    let source = std::fs::read_link(target)?;
    let source = resolve_symlink_target(target, source);
    if path_starts_with_resolved_root(&source, version_dir) {
        return Ok(true);
    }
    for app in apps {
        if path_starts_with_resolved_root(&source, &app_target_path(app.target_name())?) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn symlink_declares_target(link: &Path, expected: &Path) -> bool {
    link.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && std::fs::read_link(link)
            .map(|target| resolve_symlink_target(link, target) == expected)
            .unwrap_or(false)
}

fn validate_homebrew_cask_config(
    token: &str,
    token_dir: &Path,
    homebrew: &receipt::CaskReceipt,
    artifacts: &CaskArtifacts,
) -> Result<()> {
    let config = receipt::read_cask_config(token_dir)?;
    let expected = native_cask_config()?;
    let appdir_is_relevant = !artifacts.apps.is_empty()
        || artifacts.binaries.iter().any(|binary| {
            is_appdir_binary_target(&binary.source)
                || binary
                    .target
                    .as_deref()
                    .is_some_and(is_appdir_binary_target)
        })
        || homebrew_uninstall_delete_mentions_appdir(token, homebrew)?;
    let mut relevant = Vec::new();
    if appdir_is_relevant {
        relevant.push("appdir");
    }
    if !artifacts.fonts.is_empty() {
        relevant.push("fontdir");
    }
    for key in relevant {
        let actual = effective_cask_config_path(token, &config, key)?;
        let expected = effective_cask_config_path(token, &expected, key)?;
        if actual != expected {
            bail!(
                "brew-cask:{token}: installed Homebrew config uses unsupported custom {key} {} (mise expects {})",
                actual.display(),
                expected.display()
            );
        }
    }
    Ok(())
}

fn homebrew_uninstall_delete_mentions_appdir(
    token: &str,
    homebrew: &receipt::CaskReceipt,
) -> Result<bool> {
    for artifact in &homebrew.uninstall_artifacts {
        let Some(uninstall) = artifact.get("uninstall") else {
            continue;
        };
        let entries = uninstall
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_else(|| std::slice::from_ref(uninstall));
        for entry in entries {
            let object = entry.as_object().ok_or_else(|| {
                eyre!("brew-cask:{token}: recorded uninstall directive is not an object")
            })?;
            let Some(delete) = object.get("delete") else {
                continue;
            };
            let paths = delete
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(delete));
            for path in paths {
                let path = path.as_str().ok_or_else(|| {
                    eyre!("brew-cask:{token}: recorded uninstall delete value is not a string")
                })?;
                if path.contains("$APPDIR")
                    || path.contains("#{appdir}")
                    || path.contains("{{appdir}}")
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn effective_cask_config_path(
    token: &str,
    config: &receipt::CaskConfig,
    key: &str,
) -> Result<PathBuf> {
    for (layer, value) in [
        ("explicit", &config.explicit),
        ("env", &config.env),
        ("default", &config.default),
    ] {
        let Some(value) = value.as_object().and_then(|object| object.get(key)) else {
            continue;
        };
        let value = value.as_str().ok_or_else(|| {
            eyre!("brew-cask:{token}: installed Homebrew config {layer}.{key} is not a path")
        })?;
        return Ok(PathBuf::from(value));
    }
    bail!("brew-cask:{token}: installed Homebrew config has no {key}")
}

fn legacy_needs_repair(cask: &Cask, version: &str, detail: &str) -> InstalledCaskState {
    legacy_needs_repair_for_token(&cask.token, version, detail)
}

fn legacy_needs_repair_for_token(token: &str, version: &str, detail: &str) -> InstalledCaskState {
    InstalledCaskState::NeedsRepair {
        installed: Some(version.to_string()),
        reason: format!(
            "brew-cask:{}: legacy mise install cannot be converted ({detail}); reinstall with either 'brew install --cask {}' or mise apply after uninstalling",
            token, token
        ),
        replacement_safe: false,
    }
}

// legacy .mise-cask.toml backfill — remove when fleet converged
#[cfg(test)]
fn reconcile_legacy_cask(cask: &Cask, state: InstalledCaskState) -> Result<InstalledCaskState> {
    if !matches!(state, InstalledCaskState::LegacyMise(_)) {
        return Ok(state);
    }
    let _lock = lock_cask(&cask.token)?;
    reconcile_legacy_cask_locked(cask, state)
}

fn reconcile_legacy_cask_locked(
    cask: &Cask,
    state: InstalledCaskState,
) -> Result<InstalledCaskState> {
    let state = validate_legacy_cask(cask, state)?;
    let InstalledCaskState::LegacyMise(legacy) = state else {
        return Ok(state);
    };
    let runtime_dependencies = cask_runtime_dependencies(cask)?;
    let version_dir = caskroom_version_dir(&cask.token, &legacy.version);
    convert_legacy_moved_artifacts(cask, &legacy, &version_dir)?;
    write_homebrew_metadata(&version_dir, cask, &runtime_dependencies, false)?;
    file::remove_file(version_dir.join(".mise-cask.toml"))?;
    Ok(InstalledCaskState::Installed(legacy.version))
}

fn convert_legacy_moved_artifacts(
    cask: &Cask,
    legacy: &CaskReceipt,
    version_dir: &Path,
) -> Result<()> {
    let artifacts = cask_artifacts(cask)?;
    for app in &artifacts.apps {
        let target = app_target_path(app.target_name())?;
        let old_source = version_dir.join(app_bundle_name(app.target_name())?);
        let source = caskroom_artifact_path(version_dir, &app.source, "app")?;
        migrate_legacy_backlink(&old_source, &source)?;
        convert_legacy_moved_artifact(&source, &target, legacy)?;
    }
    for font in &artifacts.fonts {
        let target = font_target_path(font)?;
        let old_source = version_dir.join(font_filename(font)?);
        let source = caskroom_font_path(version_dir, font)?;
        migrate_legacy_backlink(&old_source, &source)?;
        convert_legacy_moved_artifact(&source, &target, legacy)?;
    }
    Ok(())
}

fn migrate_legacy_backlink(old_source: &Path, source: &Path) -> Result<()> {
    if old_source == source
        || old_source.symlink_metadata().is_err()
        || source.symlink_metadata().is_ok()
    {
        return Ok(());
    }
    if let Some(parent) = source.parent() {
        file::create_dir_all(parent)?;
    }
    file::rename(old_source, source)
}

fn convert_legacy_moved_artifact(source: &Path, target: &Path, legacy: &CaskReceipt) -> Result<()> {
    if source
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && file::same_file(source, target)
    {
        return Ok(());
    }
    let record = legacy
        .targets
        .iter()
        .find(|record| record.path == target)
        .ok_or_else(|| {
            eyre!(
                "brew-cask: legacy moved target has no ownership record: {}",
                target.display()
            )
        })?;
    if !cask_target_record_matches(record)?
        || cask_target_fingerprint(source)? != record.fingerprint
    {
        bail!(
            "brew-cask: legacy Caskroom payload and public target differ: {}",
            target.display()
        );
    }
    let backup = source.with_file_name(format!(
        ".mise-legacy-backup-{}",
        hash::hash_to_str(&source.display().to_string())
    ));
    file::remove_all(&backup)?;
    file::rename(source, &backup)?;
    if let Err(err) = file::make_symlink(target, source) {
        file::rename(&backup, source)?;
        return Err(err);
    }
    file::remove_all(backup)
}

fn validate_legacy_cask(cask: &Cask, state: InstalledCaskState) -> Result<InstalledCaskState> {
    let InstalledCaskState::LegacyMise(legacy) = state else {
        return Ok(state);
    };
    if legacy.version != cask.version {
        return Ok(legacy_needs_repair(
            cask,
            &legacy.version,
            &format!("installed {} != catalog {}", legacy.version, cask.version),
        ));
    }
    let artifacts = match cask_artifacts(cask) {
        Ok(artifacts) => artifacts,
        Err(err) => {
            return Ok(legacy_needs_repair(
                cask,
                &legacy.version,
                &format!("catalog artifact inventory could not be classified: {err}"),
            ));
        }
    };
    let expected_apps = artifacts
        .apps
        .iter()
        .map(|app| app_target_path(app.target_name()))
        .collect::<Result<Vec<_>>>()?;
    let expected_binaries = binary_targets(&artifacts)?;
    let expected_fonts = artifacts
        .fonts
        .iter()
        .map(font_target_path)
        .collect::<Result<Vec<_>>>()?;
    let expected_manpages = manpage_target_paths(&artifacts)?;
    let expected_completions = completion_target_paths(cask, &artifacts)?;
    if legacy.apps != expected_apps
        || legacy.binaries != expected_binaries
        || legacy.fonts != expected_fonts
        || legacy.manpages != expected_manpages
        || legacy.completions != expected_completions
        || legacy.pkg_ids != artifacts.pkg_ids
    {
        return Ok(legacy_needs_repair(
            cask,
            &legacy.version,
            "recorded artifact inventory does not match the installed-version catalog",
        ));
    }
    if !legacy.targets.iter().all(cask_target_present) {
        return Ok(legacy_needs_repair(
            cask,
            &legacy.version,
            "recorded target is missing or has changed kind or symlink destination",
        ));
    }
    match pkg_ids_installed(&legacy.pkg_ids) {
        Ok(true) => {}
        Ok(false) => {
            return Ok(legacy_needs_repair(
                cask,
                &legacy.version,
                "recorded package receipt is missing",
            ));
        }
        Err(err) => {
            return Ok(legacy_needs_repair(
                cask,
                &legacy.version,
                &format!("recorded package receipt could not be verified: {err}"),
            ));
        }
    }
    Ok(InstalledCaskState::LegacyMise(legacy))
}

fn cask_prune_blocker(cask: &Cask, artifacts: &CaskArtifacts) -> Option<String> {
    if !artifacts.pkgs.is_empty() {
        return Some("pkg artifacts require uninstall support".to_string());
    }
    if !artifacts.installers.is_empty() {
        return Some("installer artifacts may have untracked side effects".to_string());
    }
    if !artifacts.command_wrappers.is_empty() {
        return Some("command wrapper artifacts are not supported for pruning".to_string());
    }
    if !artifacts.generic.is_empty() {
        return Some("generic artifacts may install external trees".to_string());
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

#[cfg(test)]
fn write_receipt(caskroom: &Path, cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    write_receipt_with_flight_targets(
        caskroom,
        cask,
        artifacts,
        &[],
        &BTreeMap::new(),
        &[],
        &BTreeSet::new(),
    )
}

#[cfg(test)]
fn write_receipt_with_flight_targets(
    caskroom: &Path,
    cask: &Cask,
    artifacts: &CaskArtifacts,
    inventory_targets: &[PathBuf],
    flight_uninstall_targets: &BTreeMap<PathBuf, bool>,
    flight_directories: &[PathBuf],
    metadata_only_apps: &BTreeSet<PathBuf>,
) -> Result<()> {
    let receipt = build_receipt_with_flight_targets(
        cask,
        artifacts,
        inventory_targets,
        flight_uninstall_targets,
        flight_directories,
        metadata_only_apps,
        true,
    )?;
    let body = toml::to_string_pretty(&receipt)?;
    write_durable_file(&caskroom.join(".mise-cask.toml"), body.as_bytes())
}

fn write_auxiliary_cask_receipt_with_flight_targets(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    inventory_targets: &[PathBuf],
    flight_uninstall_targets: &BTreeMap<PathBuf, bool>,
    flight_directories: &[PathBuf],
    metadata_only_apps: &BTreeSet<PathBuf>,
) -> Result<()> {
    let receipt = build_receipt_with_flight_targets(
        cask,
        artifacts,
        inventory_targets,
        flight_uninstall_targets,
        flight_directories,
        metadata_only_apps,
        false,
    )?;
    let body = toml::to_string_pretty(&receipt)?;
    let path = auxiliary_cask_receipt_path(&cask.token)?.ok_or_else(|| {
        eyre!(
            "brew-cask:{}: native receipt is missing before auxiliary ownership commit",
            cask.token
        )
    })?;
    write_durable_file(&path, body.as_bytes())
}

fn build_receipt_with_flight_targets(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    inventory_targets: &[PathBuf],
    flight_uninstall_targets: &BTreeMap<PathBuf, bool>,
    flight_directories: &[PathBuf],
    metadata_only_apps: &BTreeSet<PathBuf>,
    block_metadata_only_prune: bool,
) -> Result<CaskReceipt> {
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
    target_paths.extend(manpage_target_paths(artifacts)?);
    target_paths.extend(inventory_targets.iter().cloned());
    target_paths.sort();
    target_paths.dedup();
    let targets = target_paths
        .iter()
        .map(|path| {
            Ok(CaskTargetRecord {
                path: path.clone(),
                fingerprint: cask_target_fingerprint(path)?,
                uninstall: flight_uninstall_targets.get(path).copied(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let prune_blocker = cask_prune_blocker(cask, artifacts).or_else(|| {
        (block_metadata_only_prune && !metadata_only_apps.is_empty()).then(|| {
            "metadata-only app ownership cannot be proven safely during pruning".to_string()
        })
    });
    Ok(CaskReceipt {
        schema_version: 3,
        version: cask.version.clone(),
        auto_updates: cask.auto_updates,
        metadata_only_apps: metadata_only_apps.iter().cloned().collect(),
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
        manpages: manpage_target_paths(artifacts)?,
        completions: completion_target_paths(cask, artifacts)?,
        flight_directories: flight_directories.to_vec(),
        generic: generic_artifact_targets(artifacts)?,
        pkg_ids: artifacts.pkg_ids.clone(),
        targets,
        prune_safe: prune_blocker.is_none(),
        prune_blocker,
    })
}

fn write_homebrew_metadata(
    caskroom: &Path,
    cask: &Cask,
    runtime_dependencies: &serde_json::Map<String, Value>,
    retain_backup: bool,
) -> Result<()> {
    let token_dir = caskroom
        .parent()
        .ok_or_else(|| eyre!("brew-cask:{}: caskroom has no token directory", cask.token))?;
    let metadata = token_dir.join(".metadata.mise-tmp");
    let destination = token_dir.join(".metadata");
    let backup = token_dir.join(".metadata.mise-backup");
    file::remove_all(&metadata)?;
    file::remove_all(&backup)?;
    let now = chrono::Local::now();
    let timestamp = now.format("%Y%m%d%H%M%S%.3f").to_string();
    let uninstall_artifacts = cask_uninstall_artifacts(cask)?;
    let snapshot_bytes = installed_cask_snapshot(cask, &uninstall_artifacts)?;
    let snapshot = metadata
        .join(&cask.version)
        .join(timestamp)
        .join("Casks")
        .join(format!("{}.json", cask.token));
    let receipt = receipt::CaskReceipt {
        homebrew_version: receipt::EMULATED_BREW_VERSION.to_string(),
        loaded_from_api: true,
        // mise fetches the public per-cask API, not Homebrew's signed internal API.
        loaded_from_internal_api: cask.loaded_from_internal_api,
        uninstall_flight_blocks: cask.artifacts.iter().any(|artifact| {
            matches!(
                artifact_type(artifact).as_str(),
                "uninstall_preflight" | "uninstall_postflight"
            )
        }),
        installed_on_request: true,
        time: now.timestamp().try_into()?,
        runtime_dependencies: runtime_dependencies.clone(),
        source: receipt::CaskSource {
            tap: cask
                .tap
                .clone()
                .ok_or_else(|| eyre!("brew-cask:{}: definition has no source tap", cask.token))?,
            tap_git_head: cask.tap_git_head.clone(),
            version: cask.version.clone(),
            path: Some(cask.definition_source.clone()),
            extra: serde_json::Map::new(),
        },
        arch: match std::env::consts::ARCH {
            "aarch64" => "arm64".to_string(),
            arch => arch.to_string(),
        },
        uninstall_artifacts,
        built_on: native_build_system_info()?,
        extra: serde_json::Map::new(),
    };
    let config = native_cask_config()?;
    write_durable_file(
        &metadata.join("INSTALL_RECEIPT.json"),
        &receipt.to_json_bytes()?,
    )?;
    write_durable_file(&metadata.join("config.json"), &config.to_json_bytes()?)?;
    write_durable_file(&snapshot, &snapshot_bytes)?;
    let had_previous = destination.symlink_metadata().is_ok();
    if had_previous {
        file::rename(&destination, &backup)?;
    }
    if let Err(err) = file::rename(&metadata, &destination) {
        if had_previous {
            file::rename(&backup, &destination)?;
        }
        return Err(err);
    }
    if !retain_backup {
        file::remove_all(backup)?;
    }
    Ok(())
}

fn rollback_homebrew_metadata(caskroom: &Path, had_previous: bool) -> Result<()> {
    let Some(token_dir) = caskroom.parent() else {
        return Ok(());
    };
    let destination = token_dir.join(".metadata");
    let backup = token_dir.join(".metadata.mise-backup");
    if backup.symlink_metadata().is_ok() {
        file::remove_all(&destination)?;
        file::rename(&backup, &destination)?;
    } else if !had_previous && destination.symlink_metadata().is_ok() {
        file::remove_all(&destination)?;
    }
    file::remove_all(token_dir.join(".metadata.mise-tmp"))
}

fn commit_homebrew_metadata(caskroom: &Path) -> Result<()> {
    let Some(token_dir) = caskroom.parent() else {
        return Ok(());
    };
    file::remove_all(token_dir.join(".metadata.mise-backup"))?;
    file::remove_all(token_dir.join(".metadata.mise-tmp"))
}

fn installed_cask_snapshot(cask: &Cask, uninstall_artifacts: &[Value]) -> Result<Vec<u8>> {
    if cask.artifacts.iter().any(|artifact| {
        matches!(
            artifact_type(artifact).as_str(),
            "uninstall_preflight" | "uninstall_postflight"
        )
    }) {
        bail!(
            "brew-cask:{}: uninstall Ruby flight blocks require a verbatim Ruby snapshot",
            cask.token
        );
    }
    let mut installed = serde_json::Map::new();
    if let Some(only_path) = &cask.url_specs.only_path {
        installed.insert(
            "url_specs".to_string(),
            serde_json::json!({ "only_path": only_path }),
        );
    }
    if uninstall_artifacts.is_empty() {
        installed.insert("artifacts".to_string(), Value::Array(Vec::new()));
    }
    Ok(serde_json::to_vec_pretty(&Value::Object(installed))?)
}

fn homebrew_artifact_rank(kind: &str) -> Option<u8> {
    match kind {
        "preflight_steps" => Some(0),
        "uninstall_preflight_steps" => Some(1),
        "preflight" | "uninstall_preflight" => Some(2),
        "uninstall" => Some(3),
        "generated_script" => Some(4),
        "installer" => Some(5),
        "pkg" => Some(6),
        "app" | "appimage" | "suite" | "artifact" | "colorpicker" | "prefpane" | "qlplugin"
        | "mdimporter" | "dictionary" | "font" | "service" | "input_method" | "internet_plugin"
        | "keyboard_layout" | "audio_unit_plugin" | "vst_plugin" | "vst3_plugin"
        | "screen_saver" => Some(7),
        "binary" | "command_wrapper" => Some(8),
        "manpage" => Some(9),
        "bash_completion" | "fish_completion" | "zsh_completion" => Some(10),
        "generate_completions_from_executable" => Some(11),
        "postflight_steps" => Some(12),
        "uninstall_postflight_steps" => Some(13),
        "postflight" | "uninstall_postflight" => Some(14),
        "zap" => Some(15),
        _ => None,
    }
}

fn homebrew_artifact_is_uninstallable(kind: &str) -> bool {
    !matches!(
        kind,
        "preflight" | "postflight" | "generated_script" | "installer" | "pkg"
    )
}

#[cfg(unix)]
#[derive(Clone, Copy)]
#[repr(C)]
struct HomebrewArtifactOrder {
    rank: u8,
    index: usize,
}

#[cfg(unix)]
unsafe extern "C" fn compare_homebrew_artifact_order(
    left: *const nix::libc::c_void,
    right: *const nix::libc::c_void,
) -> nix::libc::c_int {
    let left = unsafe { &*left.cast::<HomebrewArtifactOrder>() };
    let right = unsafe { &*right.cast::<HomebrewArtifactOrder>() };
    match left.rank.cmp(&right.rank) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

#[cfg(unix)]
fn sort_homebrew_artifact_order(entries: &[(u8, Value)]) -> Vec<usize> {
    let mut order = entries
        .iter()
        .enumerate()
        .map(|(index, (rank, _))| HomebrewArtifactOrder { index, rank: *rank })
        .collect::<Vec<_>>();
    // Ruby Array#sort delegates to the platform C qsort implementation. The
    // comparison returns equal for artifacts sharing Homebrew's type rank, so
    // use that same primitive to reproduce native receipt byte order exactly.
    unsafe {
        nix::libc::qsort(
            order.as_mut_ptr().cast(),
            order.len(),
            std::mem::size_of::<HomebrewArtifactOrder>(),
            Some(compare_homebrew_artifact_order),
        );
    }
    order.into_iter().map(|entry| entry.index).collect()
}

#[cfg(not(unix))]
fn sort_homebrew_artifact_order(entries: &[(u8, Value)]) -> Vec<usize> {
    let mut order = (0..entries.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| (entries[*index].0, *index));
    order
}

fn cask_uninstall_artifacts(cask: &Cask) -> Result<Vec<Value>> {
    let mut entries = Vec::with_capacity(cask.artifacts.len());
    for artifact in &cask.artifacts {
        let object = artifact
            .as_object()
            .ok_or_else(|| eyre!("brew-cask:{}: artifact is not an object", cask.token))?;
        let key = object
            .keys()
            .find(|key| key.as_str() != "target")
            .ok_or_else(|| eyre!("brew-cask:{}: artifact has no type", cask.token))?;
        let rank = homebrew_artifact_rank(key).ok_or_else(|| {
            eyre!(
                "brew-cask:{}: unsupported artifact type {key:?}",
                cask.token
            )
        })?;
        if !homebrew_artifact_is_uninstallable(key) {
            continue;
        }
        let mut entry = serde_json::Map::new();
        let value = if key == "generate_completions_from_executable" {
            let generated = parse_generated_completion_artifact(artifact)?.ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: malformed generated completion artifact",
                    cask.token
                )
            })?;
            let mut args = vec![Value::String(generated.executable)];
            args.extend(generated.args.into_iter().map(Value::String));
            args.push(serde_json::json!({
                "base_name": generated.base_name,
                "shell_parameter_format": generated.shell_parameter_format,
                "shells": generated
                    .shells
                    .into_iter()
                    .map(CompletionShell::name)
                    .collect::<Vec<_>>(),
            }));
            Value::Array(args)
        } else {
            object.get(key).cloned().unwrap_or(Value::Null)
        };
        entry.insert(key.clone(), expand_homebrew_cask_placeholders(value)?);
        entries.push((rank, Value::Object(entry)));
    }

    Ok(sort_homebrew_artifact_order(&entries)
        .into_iter()
        .map(|index| entries[index].1.clone())
        .collect())
}

/// Homebrew removes API placeholders while constructing artifact objects, so
/// its installed receipt records the resolved arguments rather than the public
/// API placeholders.
fn expand_homebrew_cask_placeholders(value: Value) -> Result<Value> {
    let appdir = target_app_dir()?;
    Ok(expand_homebrew_cask_placeholders_with_appdir(
        value, &appdir,
    ))
}

fn expand_homebrew_cask_placeholders_with_appdir(value: Value, appdir: &Path) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| expand_homebrew_cask_placeholders_with_appdir(value, appdir))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        expand_homebrew_cask_placeholders_with_appdir(value, appdir),
                    )
                })
                .collect(),
        ),
        Value::String(value) => {
            let prefix = prefix::prefix();
            let cellar = prefix.join("Cellar");
            Value::String(
                value
                    .replace("/$HOME", &crate::dirs::HOME.to_string_lossy())
                    .replace("$HOMEBREW_PREFIX", &prefix.to_string_lossy())
                    .replace("$HOMEBREW_CELLAR", &cellar.to_string_lossy())
                    .replace("$APPDIR", &appdir.to_string_lossy()),
            )
        }
        value => value,
    }
}

/// Homebrew expands path placeholders before persisting uninstall artifacts.
/// Restore only paths contained by the current authoritative roots so the
/// installed-receipt parser can apply the same ownership rules as catalog
/// metadata without accepting arbitrary absolute archive sources.
fn restore_homebrew_cask_placeholders(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(restore_homebrew_cask_placeholders)
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, restore_homebrew_cask_placeholders(value)))
                .collect(),
        ),
        Value::String(value) => {
            let path = Path::new(&value);
            if !path.is_absolute() {
                return Value::String(value);
            }
            let prefix = prefix::prefix();
            let cellar = prefix.join("Cellar");
            for (root, placeholder) in [
                (EffectiveCaskDirs::current().appdir, "$APPDIR"),
                (cellar, "$HOMEBREW_CELLAR"),
                (prefix, "$HOMEBREW_PREFIX"),
            ] {
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                let restored = if relative.as_os_str().is_empty() {
                    placeholder.to_string()
                } else {
                    format!("{placeholder}/{}", relative.to_string_lossy())
                };
                return Value::String(restored);
            }
            Value::String(value)
        }
        value => value,
    }
}

fn cask_runtime_dependencies(cask: &Cask) -> Result<serde_json::Map<String, Value>> {
    let mut cask_dependencies = IndexMap::<String, Value>::new();
    let mut formula_dependencies = IndexMap::<String, Value>::new();

    for dependency in &cask.resolved_cask_dependencies {
        let token_dir = caskroom_token_dir(&dependency.token);
        let receipt = receipt::read_cask_receipt(&token_dir).wrap_err_with(|| {
            format!(
                "brew-cask:{}: installed cask dependency {} has no readable native receipt",
                cask.token, dependency.token
            )
        })?;
        if receipt.source.version != dependency.version {
            bail!(
                "brew-cask:{}: installed cask dependency {} receipt version {} does not match resolved version {}",
                cask.token,
                dependency.token,
                receipt.source.version,
                dependency.version
            );
        }
        merge_cask_runtime_dependency_group(
            &mut cask_dependencies,
            receipt.runtime_dependencies.get("cask"),
            false,
        )?;
        merge_cask_runtime_dependency_group(
            &mut formula_dependencies,
            receipt.runtime_dependencies.get("formula"),
            false,
        )?;
        cask_dependencies.insert(
            dependency.token.clone(),
            serde_json::json!({
                "full_name": dependency.token,
                "version": receipt.source.version,
                "declared_directly": true,
            }),
        );
    }

    if !cask.depends_on.cask.is_empty() && cask.resolved_cask_dependencies.is_empty() {
        bail!(
            "brew-cask:{}: cask dependency provenance was not resolved before receipt creation",
            cask.token
        );
    }

    let runtime_formulae = runtime_formula_dependency_names(cask)?;
    for resolved in &cask.resolved_formula_dependencies {
        let formula = &resolved.formula;
        if !runtime_formulae.contains(&formula.name) {
            continue;
        }
        let version = formula.versions.stable.as_deref().ok_or_else(|| {
            eyre!(
                "brew-cask:{}: formula dependency {} has no stable version",
                cask.token,
                formula.name
            )
        })?;
        let pkg_version = formula.pkg_version()?;
        let keg = super::pour::keg_path(&formula.name, &pkg_version);
        let installed_receipt: receipt::FormulaReceipt = serde_json::from_slice(
            &std::fs::read(keg.join("INSTALL_RECEIPT.json")).wrap_err_with(|| {
                format!(
                    "brew-cask:{}: installed formula dependency {} has no readable native receipt",
                    cask.token, formula.name
                )
            })?,
        )
        .wrap_err_with(|| {
            format!(
                "brew-cask:{}: installed formula dependency {} has an invalid native receipt",
                cask.token, formula.name
            )
        })?;
        if installed_receipt.source.versions.stable.as_deref() != Some(version) {
            bail!(
                "brew-cask:{}: installed formula dependency {} receipt version does not match resolved version {}",
                cask.token,
                formula.name,
                version
            );
        }
        let declared_directly = cask
            .depends_on
            .formula
            .iter()
            .any(|name| name == &formula.name || formula.aliases.contains(name));
        let dependency = serde_json::json!({
            "full_name": formula.name,
            "version": version,
            "revision": formula.revision,
            "bottle_rebuild": formula
                .bottle
                .get("stable")
                .map(|bottle| bottle.rebuild)
                .unwrap_or_default(),
            "pkg_version": pkg_version,
            "declared_directly": declared_directly,
        });
        formula_dependencies.insert(formula.name.clone(), dependency);
    }

    if !cask.depends_on.formula.is_empty() && cask.resolved_formula_dependencies.is_empty() {
        bail!(
            "brew-cask:{}: formula dependency provenance was not resolved before receipt creation",
            cask.token
        );
    }

    let mut runtime = serde_json::Map::new();
    if !cask_dependencies.is_empty() {
        runtime.insert(
            "cask".to_string(),
            Value::Array(cask_dependencies.into_values().collect()),
        );
    }
    if !formula_dependencies.is_empty() {
        runtime.insert(
            "formula".to_string(),
            Value::Array(formula_dependencies.into_values().collect()),
        );
    }
    Ok(runtime)
}

fn merge_cask_runtime_dependency_group(
    output: &mut IndexMap<String, Value>,
    group: Option<&Value>,
    declared_directly: bool,
) -> Result<()> {
    let Some(group) = group else {
        return Ok(());
    };
    let entries = group
        .as_array()
        .ok_or_else(|| eyre!("brew-cask: native runtime dependency group is not an array"))?;
    for entry in entries {
        let mut entry = entry
            .as_object()
            .cloned()
            .ok_or_else(|| eyre!("brew-cask: native runtime dependency is not an object"))?;
        let full_name = entry
            .get("full_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| eyre!("brew-cask: native runtime dependency has no full_name"))?
            .to_string();
        if !entry.get("version").is_some_and(Value::is_string) {
            bail!("brew-cask: native runtime dependency {full_name} has no version");
        }
        entry.insert(
            "declared_directly".to_string(),
            Value::Bool(declared_directly),
        );
        output.insert(full_name, Value::Object(entry));
    }
    Ok(())
}

fn runtime_formula_dependency_names(cask: &Cask) -> Result<BTreeSet<String>> {
    let mut runtime = BTreeSet::new();
    let host_tag = super::tag::host_tag();
    let roots = cask
        .resolved_formula_dependencies
        .iter()
        .filter(|resolved| resolved.on_request)
        .map(|resolved| resolved.formula.name.clone())
        .collect::<Vec<_>>();
    let mut pending = roots;
    while let Some(name) = pending.pop() {
        if !runtime.insert(name.clone()) {
            continue;
        }
        let resolved = cask
            .resolved_formula_dependencies
            .iter()
            .find(|resolved| resolved.formula.name == name)
            .ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: resolved formula dependency {} is missing from its closure",
                    cask.token,
                    name
                )
            })?;
        let tag = super::resolve::dep_tag(&resolved.formula, &host_tag);
        for dependency in resolved.formula.dependencies_for(&tag) {
            let canonical = cask
                .resolved_formula_dependencies
                .iter()
                .find(|candidate| {
                    candidate.formula.name == *dependency
                        || candidate.formula.aliases.contains(dependency)
                })
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: runtime formula dependency {} is missing from the resolved closure",
                        cask.token,
                        dependency
                    )
                })?;
            pending.push(canonical.formula.name.clone());
        }
    }
    Ok(runtime)
}

fn native_cask_config() -> Result<receipt::CaskConfig> {
    #[cfg(not(target_os = "linux"))]
    let home = crate::dirs::HOME.to_string_lossy();
    let dirs = configured_cask_dirs()?;
    let languages = native_cask_languages();
    #[cfg(target_os = "linux")]
    let default = serde_json::json!({
        "languages": languages,
        "appdir": dirs.appdir,
        "appimagedir": dirs.appimagedir,
        "fontdir": dirs.fontdir,
        "vst_plugindir": dirs.vst_plugindir,
        "vst3_plugindir": dirs.vst3_plugindir,
    });
    #[cfg(not(target_os = "linux"))]
    let default = serde_json::json!({
        "languages": languages,
        "appdir": dirs.appdir,
        "appimagedir": dirs.appimagedir,
        "keyboard_layoutdir": "/Library/Keyboard Layouts",
        "colorpickerdir": format!("{home}/Library/ColorPickers"),
        "prefpanedir": format!("{home}/Library/PreferencePanes"),
        "qlplugindir": format!("{home}/Library/QuickLook"),
        "mdimporterdir": format!("{home}/Library/Spotlight"),
        "dictionarydir": format!("{home}/Library/Dictionaries"),
        "fontdir": dirs.fontdir,
        "servicedir": format!("{home}/Library/Services"),
        "input_methoddir": format!("{home}/Library/Input Methods"),
        "internet_plugindir": format!("{home}/Library/Internet Plug-Ins"),
        "audio_unit_plugindir": format!("{home}/Library/Audio/Plug-Ins/Components"),
        "vst_plugindir": dirs.vst_plugindir,
        "vst3_plugindir": dirs.vst3_plugindir,
        "screen_saverdir": format!("{home}/Library/Screen Savers")
    });
    Ok(receipt::CaskConfig {
        default,
        env: serde_json::json!({}),
        explicit: serde_json::json!({}),
        extra: serde_json::Map::new(),
    })
}

fn split_homebrew_languages(value: &str) -> Vec<String> {
    value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | ',' | '"'))
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(target_os = "macos")]
fn native_cask_languages() -> Vec<String> {
    command_output("/usr/bin/defaults", &["read", "-g", "AppleLanguages"])
        .or_else(|| {
            command_output(
                "/usr/bin/defaults",
                &[
                    "read",
                    "/Library/Preferences/.GlobalPreferences",
                    "AppleLanguages",
                ],
            )
        })
        .map(|languages| split_homebrew_languages(&languages))
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn native_cask_languages() -> Vec<String> {
    let locales = command_output("localectl", &["list-locales"])
        .map(|output| split_homebrew_languages(&output))
        .filter(|languages| !languages.is_empty())
        .unwrap_or_else(|| {
            let mut languages = crate::env::vars_safe()
                .filter(|(key, _)| key == "LANG" || key == "LANGUAGE" || key.starts_with("LC_"))
                .collect::<Vec<_>>();
            languages.sort_by(|left, right| left.0.cmp(&right.0));
            let languages = languages
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Vec<_>>();
            if languages.is_empty() {
                vec!["en_US.utf8".to_string()]
            } else {
                languages
            }
        });
    locales
        .into_iter()
        .map(|locale| {
            locale
                .split('.')
                .next()
                .unwrap_or(&locale)
                .replace('_', "-")
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn native_cask_languages() -> Vec<String> {
    Vec::new()
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn native_build_system_info() -> Result<receipt::BuiltOn> {
    receipt::native_build_system_info().map_err(|error| {
        eyre!("brew-cask: cannot determine Homebrew build-system metadata: {error}")
    })
}

fn cask_target_record_matches(record: &CaskTargetRecord) -> Result<bool> {
    let Ok(actual) = cask_target_fingerprint(&record.path) else {
        return Ok(false);
    };
    Ok(actual == record.fingerprint)
}

/// Whether a receipt target still exists at the recorded path and kind.
///
/// Directory and file targets ignore content drift so status/apply stay cheap
/// and do not reinstall app bundles (which resets macOS TCC grants). Symlink
/// targets still compare the recorded link destination — that is a cheap
/// `readlink` — and require the link to resolve, so dangling or retargeted
/// binaries/completions stay repairable on apply.
fn cask_target_present(record: &CaskTargetRecord) -> bool {
    let Ok(metadata) = record.path.symlink_metadata() else {
        return false;
    };
    match record.fingerprint.kind {
        CaskTargetKind::Symlink => {
            if !metadata.file_type().is_symlink() {
                return false;
            }
            let Ok(target) = std::fs::read_link(&record.path) else {
                return false;
            };
            let digest = hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes()));
            if digest != record.fingerprint.digest {
                return false;
            }
            // Follow the link so a dangling binary/completion is not "present".
            std::fs::metadata(&record.path).is_ok()
        }
        CaskTargetKind::File => metadata.is_file(),
        CaskTargetKind::Directory => metadata.is_dir(),
    }
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

fn pending_cask_transaction_reason_in(state_dir: &Path, token: &str) -> Option<String> {
    let directory = state_dir.join("brew-cask").join(token);
    let mut paths = directory
        .read_dir()
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    paths.sort();
    let [path] = paths.as_slice() else {
        return Some(format!(
            "brew-cask:{token}: multiple or unreadable transaction journals require manual recovery"
        ));
    };
    let body = match std::fs::read(path) {
        Ok(body) => body,
        Err(err) => {
            return Some(format!(
                "brew-cask:{token}: transaction journal cannot be read ({err}); manual recovery required"
            ));
        }
    };
    match parse_cask_transaction_journal(&body) {
        Ok(journal) if journal.token == token => Some(format!(
            "brew-cask:{token}: transaction interrupted during {:?} ({:?}); apply may recover only the recorded safe mode",
            journal.phase, journal.recovery
        )),
        Ok(_) => Some(format!(
            "brew-cask:{token}: transaction journal token mismatch; manual recovery required"
        )),
        Err(err) => Some(format!(
            "brew-cask:{token}: transaction journal is unsupported or corrupt ({err}); manual recovery required"
        )),
    }
}

fn parse_cask_transaction_journal(body: &[u8]) -> Result<CaskTransactionJournal> {
    let header: CaskTransactionJournalHeader = serde_json::from_slice(body)?;
    match header.schema_version {
        2 => Ok(serde_json::from_slice(body)?),
        1 => {
            let legacy: LegacyCaskTransactionJournal = serde_json::from_slice(body)?;
            let (phase, recovery) = match legacy.completed.last() {
                Some(action) => (
                    CaskTransactionPhase::RunningExternalAction {
                        action: format!("legacy_v1_completed:{action}"),
                    },
                    CaskRecoveryMode::Manual,
                ),
                None => (
                    CaskTransactionPhase::Prepared,
                    CaskRecoveryMode::DiscardStaging,
                ),
            };
            Ok(CaskTransactionJournal {
                schema_version: 2,
                token: legacy.token,
                version: legacy.version,
                phase,
                recovery,
                receipt_inventory_targets: Vec::new(),
                activation_targets: Vec::new(),
                predecessor_targets: Vec::new(),
                had_predecessor_metadata: false,
                reopen_bundle_ids: Vec::new(),
                completed: legacy.completed,
            })
        }
        schema_version => {
            bail!("unsupported cask transaction journal schema {schema_version}")
        }
    }
}

fn read_pending_cask_journal_in(
    state_dir: &Path,
    token: &str,
) -> Result<Option<CaskTransactionJournal>> {
    let directory = state_dir.join("brew-cask").join(token);
    let Ok(entries) = directory.read_dir() else {
        return Ok(None);
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let [path] = paths.as_slice() else {
        bail!("brew-cask:{token}: expected exactly one transaction journal");
    };
    let journal = parse_cask_transaction_journal(&std::fs::read(path)?)?;
    if journal.schema_version != 2 || journal.token != token {
        bail!("brew-cask:{token}: transaction journal is unsupported or belongs to another cask");
    }
    Ok(Some(journal))
}

fn recover_cask_transaction(cask: &Cask) -> Result<()> {
    let Some(journal) = read_pending_cask_journal_in(&crate::dirs::STATE, &cask.token)? else {
        return Ok(());
    };
    if journal.version != cask.version {
        bail!(
            "brew-cask:{}: interrupted version {} must be recovered before installing {}; reinstall with real Homebrew or remove the exact interrupted state manually",
            cask.token,
            journal.version,
            cask.version
        );
    }
    match journal.recovery {
        CaskRecoveryMode::DiscardStaging => {
            if !matches!(
                journal.phase,
                CaskTransactionPhase::Prepared | CaskTransactionPhase::Staging
            ) {
                bail!(
                    "brew-cask:{}: journal recovery mode and phase disagree; manual recovery required",
                    cask.token
                );
            }
            file::remove_all(caskroom_tmp_dir(cask))?;
            file::remove_all(cask_extract_dir(cask))?;
            remove_cask_journals(&cask.token)
        }
        CaskRecoveryMode::RestoreFilesystem => restore_interrupted_cask_filesystem(cask, &journal),
        CaskRecoveryMode::FinishCommit => finish_interrupted_cask_commit(cask, &journal),
        CaskRecoveryMode::Manual => bail!(
            "brew-cask:{}: interrupted external action in phase {:?} has unknown outcome; reinstall with real Homebrew or complete manual recovery before retrying",
            cask.token,
            journal.phase
        ),
    }
}

fn restore_interrupted_cask_filesystem(
    cask: &Cask,
    journal: &CaskTransactionJournal,
) -> Result<()> {
    for target in journal.activation_targets.iter().rev() {
        let backup = artifact_backup_path(target)?;
        if backup.symlink_metadata().is_ok() {
            if target.symlink_metadata().is_ok() && !successor_owns_public_target(cask, target) {
                bail!(
                    "brew-cask:{}: cannot restore predecessor because successor target ownership is ambiguous: {}",
                    cask.token,
                    target.display()
                );
            }
            remove_artifact_target_elevating(target)?;
            rename_elevating(&backup, target)?;
        } else if target.symlink_metadata().is_ok() {
            if !successor_owns_public_target(cask, target) {
                bail!(
                    "brew-cask:{}: interrupted target is not provably owned: {}",
                    cask.token,
                    target.display()
                );
            }
            remove_artifact_target_elevating(target)?;
        }
    }
    let destination = caskroom_version_dir(&cask.token, &cask.version);
    let backup = caskroom_backup_dir(cask);
    rollback_homebrew_metadata(&destination, journal.had_predecessor_metadata)?;
    if backup.symlink_metadata().is_ok() {
        file::remove_all(&destination)?;
        file::rename(&backup, &destination)?;
    } else if destination.symlink_metadata().is_ok() {
        file::remove_all(&destination)?;
    }
    file::remove_all(caskroom_tmp_dir(cask))?;
    file::remove_all(cask_extract_dir(cask))?;
    remove_cask_journals(&cask.token)
}

fn finish_interrupted_cask_commit(cask: &Cask, journal: &CaskTransactionJournal) -> Result<()> {
    for target in &journal.activation_targets {
        if !successor_owns_public_target(cask, target) {
            bail!(
                "brew-cask:{}: committed successor target is missing or changed: {}",
                cask.token,
                target.display()
            );
        }
        remove_artifact_target_elevating(&artifact_backup_path(target)?)?;
    }
    file::remove_all(caskroom_backup_dir(cask))?;
    commit_homebrew_metadata(&caskroom_version_dir(&cask.token, &cask.version))?;
    for target in &journal.predecessor_targets {
        if !journal.receipt_inventory_targets.contains(&target.path)
            && cask_target_record_matches(target)?
        {
            remove_artifact_target_elevating(&target.path)?;
        }
    }
    remove_stale_versions(&caskroom_token_dir(&cask.token), &cask.version)?;
    file::remove_all(cask_extract_dir(cask))?;
    remove_cask_journals(&cask.token)
}

fn successor_owns_public_target(cask: &Cask, target: &Path) -> bool {
    let version_dir = caskroom_version_dir(&cask.token, &cask.version);
    let artifacts = cask_artifacts(cask).ok();
    if target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        let Ok(link) = std::fs::read_link(target) else {
            return false;
        };
        if path_starts_with_resolved_root(&resolve_symlink_target(target, link), &version_dir) {
            return true;
        }
        let Some(artifacts) = artifacts.as_ref() else {
            return false;
        };
        if native_binary_target_is_owned(artifacts, target, &version_dir).unwrap_or(false) {
            return true;
        }
        if completion_target_is_owned(cask, artifacts, target, &version_dir).unwrap_or(false) {
            return true;
        }
        return artifacts.manpages.iter().any(|manpage| {
            manpage_target_path(manpage).is_ok_and(|path| path == target)
                && manpage_target_is_owned(manpage, &artifacts.apps, target, &version_dir)
                    .unwrap_or(false)
        });
    }
    if let Some(artifacts) = artifacts.as_ref()
        && completion_target_is_generated(cask, artifacts, target).unwrap_or(false)
    {
        return generated_completion_matches_staging(&cask_extract_dir(cask), target);
    }
    let mut backlinks = artifacts
        .into_iter()
        .flat_map(|artifacts| {
            let app_backlinks = artifacts.apps.into_iter().filter_map(|app| {
                (app_target_path(app.target_name()).ok().as_deref() == Some(target))
                    .then(|| caskroom_artifact_path(&version_dir, &app.source, "app").ok())
                    .flatten()
            });
            let font_backlinks = artifacts.fonts.into_iter().filter_map(|font| {
                (font_target_path(&font).ok().as_deref() == Some(target))
                    .then(|| caskroom_font_path(&version_dir, &font).ok())
                    .flatten()
            });
            app_backlinks.chain(font_backlinks)
        })
        .collect::<Vec<_>>();
    if let Some(name) = target.file_name() {
        backlinks.push(version_dir.join(name));
    }
    if let Ok(relative) = target.strip_prefix(font_dir()) {
        backlinks.push(version_dir.join(relative));
    }
    backlinks.into_iter().any(|backlink| {
        backlink
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
            && file::same_file(&backlink, target)
    })
}

fn write_cask_journal(journal: &CaskTransactionJournal) -> Result<()> {
    write_cask_journal_in(&crate::dirs::STATE, journal)
}

fn write_cask_journal_in(state_dir: &Path, journal: &CaskTransactionJournal) -> Result<()> {
    let path = cask_journal_path_in(state_dir, &journal.token, &journal.version);
    let body = serde_json::to_vec_pretty(journal)?;
    write_durable_file(&path, &body)
}

fn record_cask_action(journal: &mut CaskTransactionJournal, action: &str) -> Result<()> {
    record_cask_action_in(&crate::dirs::STATE, journal, action)
}

fn record_cask_action_in(
    state_dir: &Path,
    journal: &mut CaskTransactionJournal,
    action: &str,
) -> Result<()> {
    journal.completed.push(action.to_string());
    write_cask_journal_in(state_dir, journal)
}

fn set_cask_phase(journal: &mut CaskTransactionJournal, phase: CaskTransactionPhase) -> Result<()> {
    set_cask_phase_in(&crate::dirs::STATE, journal, phase)
}

fn set_cask_phase_in(
    state_dir: &Path,
    journal: &mut CaskTransactionJournal,
    phase: CaskTransactionPhase,
) -> Result<()> {
    journal.phase = phase;
    write_cask_journal_in(state_dir, journal)
}

fn set_cask_external_action(journal: &mut CaskTransactionJournal, action: &str) -> Result<()> {
    set_cask_external_action_in(&crate::dirs::STATE, journal, action)
}

fn set_cask_external_action_in(
    state_dir: &Path,
    journal: &mut CaskTransactionJournal,
    action: &str,
) -> Result<()> {
    journal.phase = CaskTransactionPhase::RunningExternalAction {
        action: action.to_string(),
    };
    journal.recovery = CaskRecoveryMode::Manual;
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

fn auxiliary_cask_receipt_path(token: &str) -> Result<Option<PathBuf>> {
    let native_receipt = caskroom_token_dir(token)
        .join(".metadata")
        .join("INSTALL_RECEIPT.json");
    if !native_receipt.is_file() {
        return Ok(None);
    }
    let native_digest = hash::file_hash_sha256(&native_receipt, None).wrap_err_with(|| {
        format!(
            "brew-cask:{token}: failed to bind auxiliary ownership to {}",
            native_receipt.display()
        )
    })?;
    Ok(Some(
        crate::dirs::STATE
            .join("system-brew-cask-native-receipts")
            .join(token)
            .join(format!("{native_digest}.toml")),
    ))
}

fn read_auxiliary_cask_receipt(token: &str, version: &str) -> Result<Option<CaskReceipt>> {
    let Some(path) = auxiliary_cask_receipt_path(token)? else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let body = crate::file::read_to_string(&path)?;
    let receipt: CaskReceipt =
        toml::from_str(&body).wrap_err_with(|| format!("failed to parse {}", path.display()))?;
    if receipt.schema_version != 3 || receipt.version != version {
        bail!("brew-cask:{token}: auxiliary ownership receipt is incompatible");
    }
    let unique_metadata_only_apps = receipt.metadata_only_apps.iter().collect::<BTreeSet<_>>();
    if receipt
        .metadata_only_apps
        .iter()
        .any(|path| !receipt.apps.contains(path))
        || unique_metadata_only_apps.len() != receipt.metadata_only_apps.len()
        || receipt.metadata_only_apps.iter().any(|path| {
            receipt
                .targets
                .iter()
                .filter(|record| record.path == *path)
                .count()
                != 1
        })
    {
        bail!("brew-cask:{token}: auxiliary app ownership inventory is incomplete");
    }
    Ok(Some(receipt))
}

fn cask_from_homebrew_receipt(token: &str, receipt: &receipt::CaskReceipt) -> Cask {
    Cask {
        token: token.to_string(),
        aliases: Vec::new(),
        old_tokens: Vec::new(),
        version: receipt.source.version.clone(),
        url: String::new(),
        url_specs: CaskUrlSpecs::default(),
        sha256: None,
        artifacts: receipt
            .uninstall_artifacts
            .clone()
            .into_iter()
            .map(restore_homebrew_cask_placeholders)
            .collect(),
        ruby_source_path: None,
        ruby_source_checksum: None,
        tap_git_head: receipt.source.tap_git_head.clone(),
        tap: Some(receipt.source.tap.clone()),
        auto_updates: false,
        depends_on: CaskDependencies::default(),
        conflicts_with: CaskConflicts::default(),
        raw_base: None,
        definition_source: receipt.source.path.clone().unwrap_or_default(),
        loaded_from_internal_api: receipt.loaded_from_internal_api,
        platform_policy: CaskPlatformPolicy::Unspecified,
        resolved_formula_dependencies: Vec::new(),
        resolved_cask_dependencies: Vec::new(),
    }
}

fn homebrew_receipt_targets(
    token: &str,
    homebrew: &receipt::CaskReceipt,
) -> Result<Vec<CaskTargetRecord>> {
    if let Some(auxiliary) = read_auxiliary_cask_receipt(token, &homebrew.source.version)? {
        return Ok(auxiliary.targets);
    }
    let cask = cask_from_homebrew_receipt(token, homebrew);
    let artifacts = parse_cask_artifacts(&cask, false)?;
    validate_homebrew_cask_config(token, &caskroom_token_dir(token), homebrew, &artifacts)?;
    let mut paths = artifacts
        .apps
        .iter()
        .map(|app| app_target_path(app.target_name()))
        .collect::<Result<Vec<_>>>()?;
    paths.extend(binary_targets(&artifacts)?);
    paths.extend(
        artifacts
            .fonts
            .iter()
            .map(font_target_path)
            .collect::<Result<Vec<_>>>()?,
    );
    paths.extend(completion_target_paths(&cask, &artifacts)?);
    paths.extend(manpage_target_paths(&artifacts)?);
    paths.extend(generic_artifact_targets(&artifacts)?);
    let structured = structured_symlink_target_records(
        &cask,
        &artifacts,
        &caskroom_version_dir(token, &homebrew.source.version),
        false,
    )?;
    let structured_copies = structured_copy_target_records(
        &cask,
        &artifacts,
        &caskroom_version_dir(token, &homebrew.source.version),
        false,
    )?;
    paths.sort();
    paths.dedup();
    let mut records = paths
        .into_iter()
        .filter(|path| path.symlink_metadata().is_ok())
        .map(|path| {
            Ok(CaskTargetRecord {
                fingerprint: cask_target_fingerprint(&path)?,
                path,
                uninstall: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    records.extend(structured);
    records.extend(structured_copies);
    records.sort_by(|left, right| left.path.cmp(&right.path));
    if records.windows(2).any(|pair| pair[0].path == pair[1].path) {
        bail!("brew-cask:{token}: installed receipt has ambiguous target claims");
    }
    Ok(records)
}

fn structured_copy_target_records(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    staged_path: &Path,
    allow_missing: bool,
) -> Result<Vec<CaskTargetRecord>> {
    let appdir = cask_appdir(&artifacts.apps)?;
    let mut records = Vec::new();
    for step in artifacts
        .preflight_steps
        .iter()
        .chain(&artifacts.postflight_steps)
    {
        let FlightStep::Copy {
            source,
            target,
            source_glob,
            guards,
            ..
        } = step
        else {
            continue;
        };
        if !guards
            .iter()
            .filter(|guard| matches!(guard, FlightGuard::OnMacos | FlightGuard::OnLinux))
            .map(|guard| flight_guard_matches(cask, guard, staged_path, &appdir))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .all(|matches| matches)
        {
            continue;
        }
        let sources = flight_symlink_sources(cask, source, *source_glob, staged_path, &appdir)?;
        let [source] = sources.as_slice() else {
            bail!(
                "brew-cask:{}: structured copy receipt source is missing or ambiguous",
                cask.token
            );
        };
        let target = resolve_flight_path_with_context(cask, target, staged_path, &appdir)?;
        if target.starts_with(staged_path) {
            continue;
        }
        if replacement_target_metadata(&target)?.is_none() {
            if !allow_missing {
                bail!(
                    "brew-cask:{}: structured copy target is missing: {}",
                    cask.token,
                    target.display()
                );
            }
            continue;
        }
        if cask_target_fingerprint(source)? != cask_target_fingerprint(&target)? {
            bail!(
                "brew-cask:{}: structured copy target was modified after installation: {}",
                cask.token,
                target.display()
            );
        }
        records.push(CaskTargetRecord {
            fingerprint: cask_target_fingerprint(&target)?,
            path: target,
            uninstall: Some(false),
        });
    }
    records.sort_by(|left, right| left.path.cmp(&right.path));
    if records.windows(2).any(|pair| pair[0].path == pair[1].path) {
        bail!(
            "brew-cask:{}: structured copy steps have ambiguous target claims",
            cask.token
        );
    }
    Ok(records)
}

fn structured_symlink_target_records(
    cask: &Cask,
    artifacts: &CaskArtifacts,
    staged_path: &Path,
    allow_missing: bool,
) -> Result<Vec<CaskTargetRecord>> {
    let appdir = cask_appdir(&artifacts.apps)?;
    let mut records = Vec::new();
    for step in artifacts
        .preflight_steps
        .iter()
        .chain(&artifacts.postflight_steps)
    {
        let FlightStep::Symlink {
            source,
            target,
            uninstall,
            source_glob,
            guards,
            ..
        } = step
        else {
            continue;
        };
        if !guards
            .iter()
            .map(|guard| flight_guard_matches(cask, guard, staged_path, &appdir))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .all(|matches| matches)
        {
            continue;
        }
        let sources = flight_symlink_sources(cask, source, *source_glob, staged_path, &appdir)?;
        if sources.is_empty() && !allow_missing {
            bail!(
                "brew-cask:{}: structured symlink source is missing",
                cask.token
            );
        }
        let target = resolve_flight_path_with_context(cask, target, staged_path, &appdir)?;
        let target_is_dir = target.is_dir() || sources.len() > 1;
        for source in sources {
            let path = if target_is_dir {
                target.join(source.file_name().ok_or_else(|| {
                    eyre!("brew-cask: structured symlink source has no file name")
                })?)
            } else {
                target.clone()
            };
            if path.starts_with(staged_path) {
                continue;
            }
            if replacement_target_metadata(&path)?.is_none() {
                if !allow_missing {
                    bail!(
                        "brew-cask:{}: structured symlink target is missing: {}",
                        cask.token,
                        path.display()
                    );
                }
                continue;
            }
            records.push(CaskTargetRecord {
                fingerprint: cask_target_fingerprint(&path)?,
                path,
                uninstall: Some(*uninstall),
            });
        }
    }
    records.sort_by(|left, right| left.path.cmp(&right.path));
    if records.windows(2).any(|pair| pair[0].path == pair[1].path) {
        bail!(
            "brew-cask:{}: structured symlink steps have ambiguous target claims",
            cask.token
        );
    }
    Ok(records)
}

fn synthetic_homebrew_prune_receipt(
    token: &str,
    homebrew: &receipt::CaskReceipt,
) -> Result<CaskReceipt> {
    if let Some(auxiliary) = read_auxiliary_cask_receipt(token, &homebrew.source.version)? {
        return Ok(auxiliary);
    }
    let targets = homebrew_receipt_targets(token, homebrew)?;
    let completion_roots = [
        CompletionShell::Bash,
        CompletionShell::Fish,
        CompletionShell::Zsh,
        CompletionShell::Pwsh,
    ]
    .map(default_completion_dir);
    let mut apps = Vec::new();
    let mut binaries = Vec::new();
    let mut fonts = Vec::new();
    let mut manpages = Vec::new();
    let mut completions = Vec::new();
    let appdir_roots = allowed_appdir_roots()?;
    for target in &targets {
        if target.fingerprint.kind == CaskTargetKind::Directory
            && appdir_roots
                .iter()
                .any(|root| path_is_below(&target.path, root))
        {
            apps.push(target.path.clone());
        } else if path_is_below(&target.path, &font_dir()) {
            fonts.push(target.path.clone());
        } else if path_is_below(&target.path, &EffectiveCaskDirs::current().manpagedir) {
            manpages.push(target.path.clone());
        } else if completion_roots
            .iter()
            .any(|root| path_is_below(&target.path, root))
        {
            completions.push(target.path.clone());
        } else {
            binaries.push(target.path.clone());
        }
    }
    Ok(CaskReceipt {
        schema_version: 3,
        version: homebrew.source.version.clone(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps,
        binaries,
        fonts,
        manpages,
        completions,
        flight_directories: Vec::new(),
        generic: Vec::new(),
        pkg_ids: Vec::new(),
        targets,
        prune_safe: true,
        prune_blocker: None,
    })
}

pub(crate) async fn cask_prune_plan(configured: &[PackageRequest]) -> Result<CaskPrunePlan> {
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

    for entry in tokens {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                claims_complete = false;
                plan.skipped.push(CaskPruneSkip {
                    token: "Caskroom".to_string(),
                    reason: format!("Caskroom entry could not be read: {err}"),
                });
                continue;
            }
        };
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(err) => {
                claims_complete = false;
                plan.skipped.push(CaskPruneSkip {
                    token: entry.file_name().to_string_lossy().to_string(),
                    reason: format!("Caskroom entry type could not be read: {err}"),
                });
                continue;
            }
        };
        if !kind.is_dir() {
            continue;
        }
        let Some(token) = entry.file_name().to_str().map(str::to_string) else {
            claims_complete = false;
            plan.skipped.push(CaskPruneSkip {
                token: entry.file_name().to_string_lossy().to_string(),
                reason: "Caskroom token name is not valid UTF-8".to_string(),
            });
            continue;
        };
        if token.starts_with('.') {
            continue;
        }
        let configured = keep.contains(&token);
        let Ok(version_entries) = std::fs::read_dir(entry.path()) else {
            claims_complete = false;
            if !configured {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "Caskroom directory could not be read".to_string(),
                });
            }
            continue;
        };
        let mut versions = Vec::new();
        let mut version_error = None;
        for version in version_entries {
            let version = match version {
                Ok(version) => version,
                Err(err) => {
                    claims_complete = false;
                    version_error =
                        Some(format!("Caskroom version entry could not be read: {err}"));
                    continue;
                }
            };
            let kind = match version.file_type() {
                Ok(kind) => kind,
                Err(err) => {
                    claims_complete = false;
                    version_error = Some(format!(
                        "Caskroom version entry type could not be read: {err}"
                    ));
                    continue;
                }
            };
            if kind.is_dir() && !version.file_name().to_string_lossy().starts_with('.') {
                versions.push(version);
            }
        }
        if let Some(reason) = version_error {
            if !configured {
                plan.skipped.push(CaskPruneSkip { token, reason });
            }
            continue;
        }
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
        if entry.path().join(".metadata").symlink_metadata().is_ok() {
            let homebrew = match receipt::read_cask_receipt(&entry.path()) {
                Ok(receipt) => receipt,
                Err(err) => {
                    claims_complete = false;
                    if !configured {
                        plan.skipped.push(CaskPruneSkip {
                            token,
                            reason: format!("Homebrew receipt could not be read: {err}"),
                        });
                    }
                    continue;
                }
            };
            let on_disk_version = version.file_name().to_string_lossy().to_string();
            if homebrew.source.version != on_disk_version {
                claims_complete = false;
                if !configured {
                    plan.skipped.push(CaskPruneSkip {
                        token,
                        reason: format!(
                            "Homebrew receipt version {} does not match Caskroom version {on_disk_version}",
                            homebrew.source.version
                        ),
                    });
                }
                continue;
            }
            let synthetic = match synthetic_homebrew_prune_receipt(&token, &homebrew) {
                Ok(receipt) => receipt,
                Err(err) => {
                    claims_complete = false;
                    if !configured {
                        plan.skipped.push(CaskPruneSkip {
                            token,
                            reason: format!("recorded artifacts cannot be indexed safely: {err:#}"),
                        });
                    }
                    continue;
                }
            };
            for target in &synthetic.targets {
                claims
                    .entry(target.path.clone())
                    .or_default()
                    .insert(token.clone());
            }
            if configured {
                continue;
            }
            if let Err(err) = validate_homebrew_uninstall_artifacts(&token, &homebrew) {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: format!("recorded artifacts cannot be removed safely: {err:#}"),
                });
                continue;
            }
            if cask_journal_pending_in(state_dir, &token) {
                plan.skipped.push(CaskPruneSkip {
                    token,
                    reason: "an incomplete cask transaction is pending".to_string(),
                });
                continue;
            }
            candidates.push(CaskPruneCandidate {
                token,
                version: on_disk_version,
                version_dir,
                receipt: synthetic,
                homebrew_receipt: Some(homebrew),
            });
            continue;
        }
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
            homebrew_receipt: None,
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

pub(crate) fn apply_cask_prune_plan(plan: &CaskPrunePlan, dry_run: bool) -> Result<usize> {
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

    let _caskroom_locks = lock_casks(plan.remove.iter().map(|candidate| candidate.token.as_str()))?;
    for candidate in &plan.remove {
        validate_cask_prune_candidate(candidate)?;
        validate_cask_prune_claims(candidate)?;
    }
    let mut removed = 0;
    for candidate in &plan.remove {
        let mut journal = CaskTransactionJournal {
            schema_version: 2,
            token: candidate.token.clone(),
            version: candidate.version.clone(),
            phase: CaskTransactionPhase::Pruning,
            recovery: CaskRecoveryMode::Manual,
            receipt_inventory_targets: candidate
                .receipt
                .targets
                .iter()
                .map(|target| target.path.clone())
                .collect(),
            activation_targets: Vec::new(),
            predecessor_targets: candidate.receipt.targets.clone(),
            had_predecessor_metadata: candidate.homebrew_receipt.is_some(),
            reopen_bundle_ids: Vec::new(),
            completed: Vec::new(),
        };
        write_cask_journal_in(state_dir, &journal)?;
        if let Some(homebrew) = &candidate.homebrew_receipt {
            execute_uninstall_recording_in(
                state_dir,
                candidate,
                homebrew,
                &mut journal,
                "uninstall",
                CaskTransactionPhase::Pruning,
                false,
            )?;
        }
        for (index, target) in candidate.receipt.targets.iter().enumerate() {
            if !target.uninstall.unwrap_or(true) {
                continue;
            }
            remove_artifact_target_elevating(&target.path)?;
            record_cask_action_in(state_dir, &mut journal, &format!("prune_target[{index}]"))?;
        }
        file::remove_all(&candidate.version_dir)?;
        record_cask_action_in(state_dir, &mut journal, "prune_caskroom")?;
        if let Some(token_dir) = candidate.version_dir.parent() {
            if token_dir.join(".metadata").exists() {
                file::remove_all(token_dir.join(".metadata"))?;
            }
            file::remove_dir(token_dir)?;
        }
        remove_cask_journals_in(state_dir, &candidate.token)?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(unix)]
type HomebrewCaskLock = nix::fcntl::Flock<std::fs::File>;

#[cfg(not(unix))]
type HomebrewCaskLock = fslock::LockFile;

fn homebrew_cask_lock_path(token: &str) -> Result<PathBuf> {
    if token.is_empty()
        || token == "."
        || token == ".."
        || token.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.' | b'_' | b'@'))
        })
    {
        bail!("brew-cask: invalid cask lock token '{token}'");
    }
    Ok(prefix::prefix()
        .join("var/homebrew/locks")
        .join(format!("{token}.cask.lock")))
}

#[cfg(unix)]
fn lock_cask(token: &str) -> Result<HomebrewCaskLock> {
    use std::os::unix::fs::MetadataExt;

    let path = homebrew_cask_lock_path(token)?;
    let directory = path
        .parent()
        .ok_or_else(|| eyre!("brew-cask: lock path has no parent"))?;
    if directory
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        bail!(
            "brew-cask:{token}: refusing symlinked Homebrew lock directory {}",
            directory.display()
        );
    }
    file::create_dir_all(directory)?;
    for _ in 0..8 {
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!(
                "brew-cask:{token}: refusing symlinked Homebrew lock file {}",
                path.display()
            );
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let lock = nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
            .map_err(|(_, err)| {
                eyre!(
                    "brew-cask:{token}: another Homebrew-compatible operation holds {} ({err})",
                    path.display()
                )
            })?;
        let descriptor = lock.metadata()?;
        let on_disk = match path.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if descriptor.dev() == on_disk.dev() && descriptor.ino() == on_disk.ino() {
            return Ok(lock);
        }
    }
    bail!(
        "brew-cask:{token}: Homebrew lock file identity changed repeatedly: {}",
        path.display()
    )
}

#[cfg(not(unix))]
fn lock_cask(token: &str) -> Result<HomebrewCaskLock> {
    let path = homebrew_cask_lock_path(token)?;
    if let Some(parent) = path.parent() {
        file::create_dir_all(parent)?;
    }
    let mut lock = fslock::LockFile::open(&path)?;
    if !lock.try_lock()? {
        bail!("brew-cask:{token}: another Homebrew-compatible operation is in progress");
    }
    Ok(lock)
}

fn lock_casks<'a>(tokens: impl IntoIterator<Item = &'a str>) -> Result<Vec<HomebrewCaskLock>> {
    let mut tokens = tokens.into_iter().collect::<Vec<_>>();
    tokens.sort_unstable();
    tokens.dedup();
    tokens.into_iter().map(lock_cask).collect()
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
        let token_dir = entry.path();
        for version in std::fs::read_dir(&token_dir)? {
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
        if token_dir.join(".metadata").symlink_metadata().is_ok() {
            let homebrew = receipt::read_cask_receipt(&token_dir)?;
            let installed_version = token_dir.join(&homebrew.source.version);
            if !installed_version.is_dir() || installed_version.is_symlink() {
                bail!(
                    "brew-cask:{token}: Homebrew receipt version {} has no matching Caskroom directory",
                    homebrew.source.version
                );
            }
            for target in homebrew_receipt_targets(&token, &homebrew)? {
                claims.entry(target.path).or_default().insert(token.clone());
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum HomebrewUninstallAction {
    Pkgutil(String),
    Delete(PathBuf),
    Trash(PathBuf),
    Quit(String),
    Launchctl(String),
    Signal {
        signal: String,
        bundle_id: String,
    },
    Script {
        executable: String,
        args: Vec<String>,
        sudo: bool,
        must_succeed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HomebrewUninstallOwnershipStep {
    paths: Vec<FlightPath>,
    user: Option<String>,
    group: Option<String>,
    recursive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HomebrewUninstallFlightPlan {
    preflight: Vec<HomebrewUninstallOwnershipStep>,
}

#[derive(Debug, Clone)]
struct PkgRemovalPlan {
    package_id: String,
    root: PathBuf,
    files: Vec<PathBuf>,
    specials: Vec<PathBuf>,
    directories: Vec<PathBuf>,
    all_paths: BTreeSet<PathBuf>,
}

fn valid_bundle_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

fn valid_ownership_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn parse_homebrew_uninstall_flight_plan(
    cask: &Cask,
    artifacts: &[Value],
) -> Result<HomebrewUninstallFlightPlan> {
    let mut plan = HomebrewUninstallFlightPlan::default();
    for artifact in artifacts {
        for kind in ["uninstall_preflight_steps", "uninstall_postflight_steps"] {
            let Some(steps) = parse_flight_steps(cask, artifact, kind)? else {
                continue;
            };
            if kind == "uninstall_postflight_steps" && !steps.is_empty() {
                bail!(
                    "brew-cask:{}: recorded uninstall_postflight_steps are unsupported",
                    cask.token
                );
            }
            for step in steps {
                let FlightStep::SetOwnership {
                    paths,
                    user,
                    group,
                    recursive,
                } = step
                else {
                    bail!(
                        "brew-cask:{}: unsupported recorded {kind} step {}",
                        cask.token,
                        step.kind()
                    );
                };
                plan.preflight.push(HomebrewUninstallOwnershipStep {
                    paths,
                    user,
                    group,
                    recursive,
                });
            }
        }
    }
    Ok(plan)
}

fn validate_homebrew_uninstall_flight_plan(
    cask: &Cask,
    plan: &HomebrewUninstallFlightPlan,
    owned_app_targets: &[PathBuf],
) -> Result<()> {
    let appdir = cask_appdir(&[])?;
    for step in &plan.preflight {
        if step.paths.is_empty() {
            bail!(
                "brew-cask:{}: uninstall set_ownership requires at least one path",
                cask.token
            );
        }
        if step
            .user
            .as_deref()
            .is_some_and(|user| !valid_ownership_name(user))
            || step
                .group
                .as_deref()
                .is_some_and(|group| !valid_ownership_name(group))
        {
            bail!(
                "brew-cask:{}: uninstall set_ownership user or group is invalid",
                cask.token
            );
        }
        for path in &step.paths {
            if path.base != FlightPathBase::AppDir {
                bail!(
                    "brew-cask:{}: uninstall set_ownership is restricted to appdir",
                    cask.token
                );
            }
            let target = resolve_flight_path_with_context(
                cask,
                path,
                &caskroom_version_dir(&cask.token, &cask.version),
                &appdir,
            )?;
            if !owned_app_targets.iter().any(|owned| owned == &target) {
                bail!(
                    "brew-cask:{}: uninstall set_ownership target is not a receipt-owned app: {}",
                    cask.token,
                    target.display()
                );
            }
        }
    }
    Ok(())
}

fn cask_uninstall_flight_plan(cask: &Cask) -> Result<HomebrewUninstallFlightPlan> {
    let plan = parse_homebrew_uninstall_flight_plan(cask, &cask.artifacts)?;
    let owned_apps = cask
        .artifacts
        .iter()
        .filter_map(parse_app_artifact)
        .map(|app| app_target_path(app.target_name()))
        .collect::<Result<Vec<_>>>()?;
    validate_homebrew_uninstall_flight_plan(cask, &plan, &owned_apps)?;
    Ok(plan)
}

fn homebrew_receipt_uninstall_flight_plan(
    token: &str,
    homebrew: &receipt::CaskReceipt,
) -> Result<HomebrewUninstallFlightPlan> {
    let cask = cask_from_homebrew_receipt(token, homebrew);
    let plan = parse_homebrew_uninstall_flight_plan(&cask, &homebrew.uninstall_artifacts)?;
    if plan.preflight.is_empty() {
        return Ok(plan);
    }
    let appdir_roots = allowed_appdir_roots()?;
    let owned_apps = homebrew_receipt_targets(token, homebrew)?
        .into_iter()
        .filter(|record| {
            record.fingerprint.kind == CaskTargetKind::Directory
                && appdir_roots
                    .iter()
                    .any(|root| path_is_below(&record.path, root))
        })
        .map(|record| record.path)
        .collect::<Vec<_>>();
    validate_homebrew_uninstall_flight_plan(&cask, &plan, &owned_apps)?;
    Ok(plan)
}

fn homebrew_uninstall_actions(
    token: &str,
    homebrew: &receipt::CaskReceipt,
) -> Result<Vec<HomebrewUninstallAction>> {
    homebrew_uninstall_actions_from_artifacts(token, &homebrew.uninstall_artifacts)
}

fn homebrew_uninstall_actions_from_artifacts(
    token: &str,
    artifacts: &[Value],
) -> Result<Vec<HomebrewUninstallAction>> {
    let mut directives = BTreeMap::<&'static str, Vec<Value>>::new();
    for artifact in artifacts {
        validate_recorded_install_steps_uninstall(token, artifact)?;
        if let Some(kind) = artifact.as_object().and_then(|object| {
            object.keys().find(|kind| {
                matches!(
                    kind.as_str(),
                    "uninstall_preflight"
                        | "uninstall_preflight_steps"
                        | "uninstall_postflight"
                        | "uninstall_postflight_steps"
                )
            })
        }) {
            if matches!(
                kind.as_str(),
                "uninstall_preflight" | "uninstall_postflight"
            ) {
                bail!("brew-cask:{token}: unsupported recorded uninstall directive {kind}");
            }
            continue;
        }
        let Some(uninstall) = artifact.get("uninstall") else {
            continue;
        };
        let entries = uninstall
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_else(|| std::slice::from_ref(uninstall));
        for entry in entries {
            let object = entry.as_object().ok_or_else(|| {
                eyre!("brew-cask:{token}: recorded uninstall directive is not an object")
            })?;
            for (kind, value) in object {
                if kind == "on_upgrade" {
                    let valid = value.is_string()
                        || value
                            .as_array()
                            .is_some_and(|values| values.iter().all(Value::is_string));
                    if !valid {
                        bail!("brew-cask:{token}: recorded uninstall on_upgrade value is invalid");
                    }
                    continue;
                }
                let key = match kind.as_str() {
                    "launchctl" => "launchctl",
                    "quit" => "quit",
                    "signal" => "signal",
                    "script" => "script",
                    "pkgutil" => "pkgutil",
                    "delete" => "delete",
                    "trash" => "trash",
                    _ => {
                        bail!("brew-cask:{token}: unsupported recorded uninstall directive {kind}")
                    }
                };
                if matches!(key, "signal" | "script") {
                    directives.entry(key).or_default().push(value.clone());
                    continue;
                }
                let values = value
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_else(|| std::slice::from_ref(value));
                for value in values {
                    if !value.is_string() {
                        bail!("brew-cask:{token}: recorded uninstall {kind} value is not a string");
                    }
                    directives.entry(key).or_default().push(value.clone());
                }
            }
        }
    }
    let mut actions = Vec::new();
    // Homebrew 6.0.17 AbstractUninstall::ORDERED_DIRECTIVES. Unsupported
    // directives fail above; zap artifacts are intentionally never included.
    for kind in [
        "launchctl",
        "quit",
        "signal",
        "script",
        "pkgutil",
        "delete",
        "trash",
    ] {
        for value in directives.remove(kind).unwrap_or_default() {
            match kind {
                "launchctl" => actions.push(HomebrewUninstallAction::Launchctl(
                    value.as_str().unwrap().to_string(),
                )),
                "quit" => actions.push(HomebrewUninstallAction::Quit(
                    value.as_str().unwrap().to_string(),
                )),
                "signal" => actions.extend(parse_uninstall_signals(token, &value)?),
                "script" => actions.push(parse_uninstall_script(token, &value)?),
                "pkgutil" => actions.push(HomebrewUninstallAction::Pkgutil(
                    value.as_str().unwrap().to_string(),
                )),
                "delete" => actions.push(HomebrewUninstallAction::Delete(PathBuf::from(
                    value.as_str().unwrap(),
                ))),
                "trash" => actions.push(HomebrewUninstallAction::Trash(PathBuf::from(
                    value.as_str().unwrap(),
                ))),
                _ => unreachable!(),
            }
        }
    }
    Ok(actions)
}

fn parse_uninstall_signals(token: &str, value: &Value) -> Result<Vec<HomebrewUninstallAction>> {
    let values = value
        .as_array()
        .ok_or_else(|| eyre!("brew-cask:{token}: recorded uninstall signal must be an array"))?;
    if values.is_empty() || values.len() % 2 != 0 {
        bail!("brew-cask:{token}: recorded uninstall signal must contain signal/bundle-id pairs");
    }
    values
        .chunks_exact(2)
        .map(|pair| {
            let signal = pair[0].as_str().ok_or_else(|| {
                eyre!("brew-cask:{token}: recorded uninstall signal name is not a string")
            })?;
            let bundle_id = pair[1].as_str().ok_or_else(|| {
                eyre!("brew-cask:{token}: recorded uninstall signal bundle id is not a string")
            })?;
            Ok(HomebrewUninstallAction::Signal {
                signal: signal.to_string(),
                bundle_id: bundle_id.to_string(),
            })
        })
        .collect()
}

fn parse_uninstall_script(token: &str, value: &Value) -> Result<HomebrewUninstallAction> {
    let object = value
        .as_object()
        .ok_or_else(|| eyre!("brew-cask:{token}: recorded uninstall script must be an object"))?;
    reject_unsupported_artifact_fields(
        "uninstall script",
        object,
        &["executable", "args", "sudo", "must_succeed"],
    )?;
    let executable = object
        .get("executable")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| eyre!("brew-cask:{token}: recorded uninstall script has no executable"))?;
    let args = object
        .get("args")
        .map(|args| {
            args.as_array()
                .ok_or_else(|| {
                    eyre!("brew-cask:{token}: recorded uninstall script args must be an array")
                })?
                .iter()
                .map(|arg| {
                    arg.as_str().map(str::to_string).ok_or_else(|| {
                        eyre!(
                            "brew-cask:{token}: recorded uninstall script arguments must be strings"
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let parse_bool = |field: &str, default| match object.get(field) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!("brew-cask:{token}: recorded uninstall script {field} must be boolean"),
    };
    Ok(HomebrewUninstallAction::Script {
        executable: executable.to_string(),
        args,
        sudo: parse_bool("sudo", false)?,
        must_succeed: parse_bool("must_succeed", true)?,
    })
}

fn validate_recorded_install_steps_uninstall(token: &str, artifact: &Value) -> Result<()> {
    for kind in ["preflight_steps", "postflight_steps"] {
        let Some(groups) = artifact.as_object().and_then(|object| object.get(kind)) else {
            continue;
        };
        let groups = groups
            .as_array()
            .ok_or_else(|| eyre!("brew-cask:{token}: recorded {kind} metadata is not an array"))?;
        for group in groups {
            let steps = group
                .as_object()
                .and_then(|group| group.get("steps"))
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    eyre!("brew-cask:{token}: recorded {kind} step group is malformed")
                })?;
            for step in steps {
                let object = step
                    .as_object()
                    .ok_or_else(|| eyre!("brew-cask:{token}: recorded {kind} step is malformed"))?;
                let step_type = object.get("type").and_then(Value::as_str).ok_or_else(|| {
                    eyre!("brew-cask:{token}: recorded {kind} step type is missing")
                })?;
                if step_type == "symlink" {
                    match object.get("uninstall") {
                        Some(Value::Bool(true)) => bail!(
                            "brew-cask:{token}: recorded {kind} symlink uninstall step is unsupported"
                        ),
                        Some(Value::Bool(false)) | None => {}
                        Some(_) => bail!(
                            "brew-cask:{token}: recorded {kind} symlink uninstall flag is invalid"
                        ),
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_homebrew_uninstall_artifacts(
    token: &str,
    homebrew: &receipt::CaskReceipt,
) -> Result<()> {
    if homebrew.uninstall_flight_blocks {
        bail!(
            "brew-cask:{token}: installed uninstall flight blocks cannot be replayed from JSON metadata"
        );
    }
    homebrew_receipt_uninstall_flight_plan(token, homebrew)?;
    let actions = homebrew_uninstall_actions(token, homebrew)?;
    validate_homebrew_uninstall_actions(token, &homebrew.source.version, &actions)
}

fn validate_homebrew_uninstall_actions(
    token: &str,
    version: &str,
    actions: &[HomebrewUninstallAction],
) -> Result<()> {
    for action in actions {
        match action {
            HomebrewUninstallAction::Delete(path) | HomebrewUninstallAction::Trash(path) => {
                let expanded = expand_cask_template(
                    &path.to_string_lossy(),
                    &caskroom_version_dir(token, version),
                    &cask_appdir(&[])?,
                    Some(version),
                );
                let expanded = Path::new(&expanded);
                if !expanded.is_absolute()
                    || expanded
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    bail!(
                        "brew-cask:{token}: recorded uninstall delete path is not an absolute normalized path: {}",
                        expanded.display()
                    );
                }
                validate_cask_delete_pattern(token, expanded)?;
            }
            HomebrewUninstallAction::Quit(bundle_id) if !valid_bundle_identifier(bundle_id) => {
                bail!("brew-cask:{token}: recorded quit bundle identifier is invalid: {bundle_id}");
            }
            HomebrewUninstallAction::Launchctl(label) if !valid_bundle_identifier(label) => {
                bail!("brew-cask:{token}: recorded launchctl label is unsupported: {label}");
            }
            HomebrewUninstallAction::Signal { signal, bundle_id } => {
                validate_uninstall_signal(token, signal, bundle_id)?;
            }
            HomebrewUninstallAction::Script {
                executable, args, ..
            } => validate_uninstall_script(token, version, executable, args)?,
            HomebrewUninstallAction::Pkgutil(pkg_id) if !valid_bundle_identifier(pkg_id) => {
                bail!("brew-cask:{token}: recorded pkgutil identifier is unsupported: {pkg_id}");
            }
            _ => {}
        }
    }
    #[cfg(target_os = "macos")]
    let _ = &actions;
    #[cfg(not(target_os = "macos"))]
    if actions.iter().any(|action| {
        matches!(
            action,
            HomebrewUninstallAction::Pkgutil(_)
                | HomebrewUninstallAction::Quit(_)
                | HomebrewUninstallAction::Launchctl(_)
                | HomebrewUninstallAction::Signal { .. }
        )
    }) {
        bail!("brew-cask:{token}: recorded macOS uninstall directive is unavailable on this host");
    }
    Ok(())
}

fn validate_cask_uninstall_plan(cask: &Cask) -> Result<()> {
    cask_uninstall_flight_plan(cask)?;
    let actions = homebrew_uninstall_actions_from_artifacts(&cask.token, &cask.artifacts)?;
    validate_homebrew_uninstall_actions(&cask.token, &cask.version, &actions)
}

fn validate_uninstall_signal(token: &str, signal: &str, bundle_id: &str) -> Result<()> {
    if !matches!(
        signal,
        "HUP" | "INT" | "QUIT" | "KILL" | "TERM" | "USR1" | "USR2"
    ) {
        bail!("brew-cask:{token}: recorded uninstall signal is unsupported: {signal}");
    }
    if !valid_bundle_identifier(bundle_id) {
        bail!(
            "brew-cask:{token}: recorded uninstall signal bundle identifier is invalid: {bundle_id}"
        );
    }
    Ok(())
}

fn validate_uninstall_script(
    token: &str,
    version: &str,
    executable: &str,
    args: &[String],
) -> Result<()> {
    if executable.contains('\0') || args.iter().any(|arg| arg.contains('\0')) {
        bail!("brew-cask:{token}: recorded uninstall script contains a NUL byte");
    }
    let appdir = cask_appdir(&[])?;
    let expanded = expand_cask_template(
        executable,
        &caskroom_version_dir(token, version),
        &appdir,
        Some(version),
    );
    let expanded = Path::new(&expanded);
    if !expanded.is_absolute()
        || expanded
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || (!expanded.starts_with(&appdir)
            && !expanded.starts_with(caskroom_version_dir(token, version)))
    {
        bail!(
            "brew-cask:{token}: recorded uninstall script executable is outside owned cask roots: {}",
            expanded.display()
        );
    }
    Ok(())
}

fn execute_homebrew_uninstall_action(
    candidate: &CaskPruneCandidate,
    action: HomebrewUninstallAction,
    _quit_was_running: bool,
    pkg_plans: &BTreeMap<String, Vec<PkgRemovalPlan>>,
) -> Result<()> {
    match action {
        HomebrewUninstallAction::Pkgutil(id) => {
            let plans = pkg_plans.get(&id).ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: package teardown was not preflighted: {id}",
                    candidate.token
                )
            })?;
            for plan in plans {
                execute_pkg_removal_plan(&candidate.token, plan)?;
            }
        }
        HomebrewUninstallAction::Delete(path) | HomebrewUninstallAction::Trash(path) => {
            let raw = path.to_string_lossy();
            let expanded = expand_cask_template(
                &raw,
                &candidate.version_dir,
                &cask_appdir(&[])?,
                Some(&candidate.version),
            );
            let expanded = Path::new(&expanded);
            if !expanded.is_absolute()
                || expanded
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                bail!(
                    "brew-cask:{}: expanded uninstall delete path is not an absolute normalized path: {}",
                    candidate.token,
                    expanded.display()
                );
            }
            validate_cask_delete_pattern(&candidate.token, expanded)?;
            for target in expand_cask_delete_pattern(expanded)? {
                if target.starts_with("/System") {
                    if target.symlink_metadata().is_ok() {
                        bail!(
                            "brew-cask:{}: refusing protected uninstall target that appeared after preflight: {}",
                            candidate.token,
                            target.display()
                        );
                    }
                    continue;
                }
                remove_artifact_target_elevating(&target)?;
            }
        }
        HomebrewUninstallAction::Quit(bundle_id) => {
            #[cfg(not(target_os = "macos"))]
            bail!(
                "brew-cask:{}: quit uninstall for {bundle_id} is only available on macOS",
                candidate.token,
            );
            #[cfg(target_os = "macos")]
            quit_bundle(&candidate.token, &bundle_id, _quit_was_running)?;
        }
        HomebrewUninstallAction::Launchctl(label) => {
            #[cfg(not(target_os = "macos"))]
            bail!(
                "brew-cask:{}: launchctl uninstall for {label} is only available on macOS",
                candidate.token,
            );
            #[cfg(target_os = "macos")]
            remove_launchctl_service(&candidate.token, &label)?;
        }
        HomebrewUninstallAction::Signal { signal, bundle_id } => {
            #[cfg(not(target_os = "macos"))]
            bail!(
                "brew-cask:{}: signal {signal} uninstall for {bundle_id} is only available on macOS",
                candidate.token,
            );
            #[cfg(target_os = "macos")]
            signal_bundle_processes(&candidate.token, &signal, &bundle_id)?;
        }
        HomebrewUninstallAction::Script {
            executable,
            args,
            sudo,
            must_succeed,
        } => execute_uninstall_script(candidate, &executable, &args, sudo, must_succeed)?,
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum PkgRemovalPathKind {
    File,
    Special,
}

#[cfg(unix)]
fn live_pkg_removal_paths(
    token: &str,
    paths: &[PathBuf],
    expected: PkgRemovalPathKind,
) -> Result<Vec<PathBuf>> {
    let mut live = Vec::new();
    for path in paths {
        let metadata = match path.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).wrap_err_with(|| {
                    format!(
                        "brew-cask:{token}: cannot revalidate package path {}",
                        path.display()
                    )
                });
            }
        };
        let matches = match expected {
            PkgRemovalPathKind::File => metadata.is_file(),
            PkgRemovalPathKind::Special => {
                metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir())
            }
        };
        if !matches {
            bail!(
                "brew-cask:{token}: package path changed type after preflight: {}",
                path.display()
            );
        }
        live.push(path.clone());
    }
    Ok(live)
}

#[cfg(unix)]
fn execute_pkg_removal_plan(token: &str, plan: &PkgRemovalPlan) -> Result<()> {
    for (paths, expected) in [
        (&plan.files, PkgRemovalPathKind::File),
        (&plan.specials, PkgRemovalPathKind::Special),
    ] {
        // Homebrew classifies the live BOM after earlier uninstall directives.
        // Our complete preflight happens before those directives, so repeat only
        // the type/existence check here and tolerate paths an owned script removed.
        let paths = live_pkg_removal_paths(token, paths, expected)?;
        if paths.is_empty() {
            continue;
        }
        let mut input = Vec::new();
        for path in &paths {
            input.extend_from_slice(path.as_os_str().as_bytes());
            input.push(0);
        }
        sudo::run_with_input(
            "/usr/bin/xargs",
            &[
                "-0".to_string(),
                "--".to_string(),
                "/bin/rm".to_string(),
                "-f".to_string(),
                "--".to_string(),
            ],
            &input,
        )?;
    }
    for directory in &plan.directories {
        let metadata = match directory.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).wrap_err_with(|| {
                    format!(
                        "brew-cask:{token}: cannot revalidate package directory {}",
                        directory.display()
                    )
                });
            }
        };
        if !metadata.is_dir() {
            bail!(
                "brew-cask:{token}: package directory changed type after preflight: {}",
                directory.display()
            );
        }
        let ds_store = directory.join(".DS_Store");
        if ds_store.symlink_metadata().is_ok() {
            sudo::run(
                "/bin/rm",
                &[
                    "-f".to_string(),
                    "--".to_string(),
                    ds_store.display().to_string(),
                ],
                &[],
            )?;
        }
        let output = sudo::output(
            "/bin/rmdir",
            &["--".to_string(), directory.display().to_string()],
            &[],
        )?;
        if !output.status.success() {
            match directory.symlink_metadata() {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Ok(metadata) if metadata.is_dir() => {
                    // Match Homebrew's rmdir helper: retain non-empty user data.
                }
                Ok(_) => bail!(
                    "brew-cask:{token}: package directory changed type during teardown: {}",
                    directory.display()
                ),
                Err(err) => {
                    return Err(err).wrap_err_with(|| {
                        format!(
                            "brew-cask:{token}: cannot inspect package directory {}",
                            directory.display()
                        )
                    });
                }
            }
        }
    }
    if !cask_path_is_undeletable(&plan.root) {
        let _ = sudo::output(
            "/bin/rmdir",
            &["--".to_string(), plan.root.display().to_string()],
            &[],
        )?;
    }
    if pkg_id_installed(&plan.package_id)? {
        sudo::run(
            "/usr/sbin/pkgutil",
            &["--forget".to_string(), plan.package_id.clone()],
            &[],
        )?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn execute_pkg_removal_plan(token: &str, plan: &PkgRemovalPlan) -> Result<()> {
    bail!(
        "brew-cask:{token}: package teardown is unavailable for {}",
        plan.package_id
    )
}

fn execute_uninstall_script(
    candidate: &CaskPruneCandidate,
    executable: &str,
    args: &[String],
    elevate: bool,
    must_succeed: bool,
) -> Result<()> {
    validate_uninstall_script(&candidate.token, &candidate.version, executable, args)?;
    let executable = expand_cask_template(
        executable,
        &candidate.version_dir,
        &cask_appdir(&[])?,
        Some(&candidate.version),
    );
    if !Path::new(&executable).is_file() {
        bail!(
            "brew-cask:{}: uninstall script does not exist: {executable}",
            candidate.token
        );
    }
    if elevate {
        if must_succeed {
            sudo::run(&executable, args, &[])
        } else {
            let output = sudo::output(&executable, args, &[])?;
            if !output.status.success() {
                warn!(
                    "brew-cask:{}: optional uninstall script failed with {}",
                    candidate.token, output.status
                );
            }
            Ok(())
        }
    } else {
        let runner = CmdLineRunner::new(&executable).args(args).raw(true);
        if must_succeed {
            runner.execute()
        } else {
            if let Err(error) = runner.execute() {
                warn!(
                    "brew-cask:{}: optional uninstall script failed: {error:#}",
                    candidate.token
                );
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
fn signal_bundle_processes(token: &str, signal: &str, bundle_id: &str) -> Result<()> {
    validate_uninstall_signal(token, signal, bundle_id)?;
    let output = std::process::Command::new("/bin/launchctl")
        .arg("list")
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        bail!("brew-cask:{token}: launchctl list failed while resolving {bundle_id}");
    }
    let mut pids = String::from_utf8(output.stdout)?
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<i32>().ok()?;
            let _state = fields.next()?;
            let label = fields.next()?;
            (pid > 0 && launchctl_label_matches_bundle(label, bundle_id)).then_some(pid)
        })
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    if pids.is_empty() {
        return Ok(());
    }
    let current_uid = nix::unistd::getuid().as_raw();
    for pid in &pids {
        let output = std::process::Command::new("/bin/ps")
            .args(["-o", "uid=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .output()?;
        let owner = String::from_utf8(output.stdout)?.trim().parse::<u32>()?;
        if !output.status.success() || owner != current_uid {
            bail!(
                "brew-cask:{token}: refusing to signal PID {pid} not proven owned by uid {current_uid}"
            );
        }
    }
    let signal = match signal {
        "HUP" => nix::sys::signal::Signal::SIGHUP,
        "INT" => nix::sys::signal::Signal::SIGINT,
        "QUIT" => nix::sys::signal::Signal::SIGQUIT,
        "KILL" => nix::sys::signal::Signal::SIGKILL,
        "TERM" => nix::sys::signal::Signal::SIGTERM,
        "USR1" => nix::sys::signal::Signal::SIGUSR1,
        "USR2" => nix::sys::signal::Signal::SIGUSR2,
        _ => unreachable!(),
    };
    for pid in pids {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal)?;
    }
    std::thread::sleep(std::time::Duration::from_secs(3));
    Ok(())
}

#[cfg(target_os = "macos")]
fn launchctl_label_matches_bundle(label: &str, bundle_id: &str) -> bool {
    let label = label.strip_prefix("application.").unwrap_or(label);
    if label == bundle_id {
        return true;
    }
    let Some(suffix) = label
        .strip_prefix(bundle_id)
        .and_then(|value| value.strip_prefix('.'))
    else {
        return false;
    };
    let components = suffix.split('.').collect::<Vec<_>>();
    !components.is_empty()
        && components.len() <= 2
        && components.iter().all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn validate_cask_delete_pattern(token: &str, path: &Path) -> Result<()> {
    if cask_path_is_undeletable(path) {
        bail!(
            "brew-cask:{token}: refusing recorded uninstall delete of protected path {}",
            path.display()
        );
    }
    let raw = path.to_string_lossy();
    if raw.contains(['*', '?', '[']) {
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("brew-cask:{token}: uninstall glob has no parent"))?;
        if parent.to_string_lossy().contains(['*', '?', '[']) || cask_path_is_undeletable(parent) {
            bail!(
                "brew-cask:{token}: uninstall glob has an ambiguous or protected parent: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn expand_cask_delete_pattern(pattern: &Path) -> Result<Vec<PathBuf>> {
    let raw = pattern.to_string_lossy();
    if !raw.contains(['*', '?', '[', '{']) {
        return Ok(vec![pattern.to_path_buf()]);
    }
    let mut paths = Vec::new();
    for pattern in expand_braces(&raw) {
        paths.extend(
            glob::glob(&pattern)
                .wrap_err_with(|| {
                    format!("brew-cask: invalid recorded uninstall glob '{pattern}'")
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        );
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn execute_predecessor_uninstall_recording(
    candidate: &CaskPruneCandidate,
    homebrew: &receipt::CaskReceipt,
    journal: &mut CaskTransactionJournal,
) -> Result<()> {
    execute_uninstall_recording_in(
        &crate::dirs::STATE,
        candidate,
        homebrew,
        journal,
        "predecessor_uninstall",
        CaskTransactionPhase::Staging,
        true,
    )
}

#[derive(Debug)]
struct PreparedUninstallOwnershipStep {
    paths: Vec<PathBuf>,
    owner: String,
    recursive: bool,
}

fn prepare_uninstall_ownership_steps(
    candidate: &CaskPruneCandidate,
    homebrew: &receipt::CaskReceipt,
) -> Result<Vec<PreparedUninstallOwnershipStep>> {
    let cask = cask_from_homebrew_receipt(&candidate.token, homebrew);
    let plan = homebrew_receipt_uninstall_flight_plan(&candidate.token, homebrew)?;
    let appdir = cask_appdir(&[])?;
    let current_user = nix::unistd::User::from_uid(nix::unistd::geteuid())?
        .map(|user| user.name)
        .ok_or_else(|| {
            eyre!(
                "brew-cask:{}: could not determine current user",
                candidate.token
            )
        })?;
    let mut prepared = Vec::with_capacity(plan.preflight.len());
    for step in plan.preflight {
        let user = step.user.unwrap_or_else(|| current_user.clone());
        let group = step.group.unwrap_or_else(|| "staff".to_string());
        if nix::unistd::User::from_name(&user)?.is_none() {
            bail!(
                "brew-cask:{}: uninstall set_ownership user does not exist: {user}",
                candidate.token
            );
        }
        if nix::unistd::Group::from_name(&group)?.is_none() {
            bail!(
                "brew-cask:{}: uninstall set_ownership group does not exist: {group}",
                candidate.token
            );
        }
        let mut paths = Vec::with_capacity(step.paths.len());
        for path in &step.paths {
            let target =
                resolve_flight_path_with_context(&cask, path, &candidate.version_dir, &appdir)?;
            let record = candidate
                .receipt
                .targets
                .iter()
                .find(|record| record.path == target)
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: uninstall set_ownership target is not receipt-owned: {}",
                        candidate.token,
                        target.display()
                    )
                })?;
            if record.fingerprint.kind != CaskTargetKind::Directory
                || !cask_target_record_matches(record)?
            {
                bail!(
                    "brew-cask:{}: uninstall set_ownership target changed before mutation: {}",
                    candidate.token,
                    target.display()
                );
            }
            paths.push(target);
        }
        prepared.push(PreparedUninstallOwnershipStep {
            paths,
            owner: format!("{user}:{group}"),
            recursive: step.recursive,
        });
    }
    Ok(prepared)
}

fn execute_uninstall_ownership_step(
    candidate: &CaskPruneCandidate,
    step: &PreparedUninstallOwnershipStep,
) -> Result<()> {
    for path in &step.paths {
        let record = candidate
            .receipt
            .targets
            .iter()
            .find(|record| record.path == *path)
            .ok_or_else(|| {
                eyre!(
                    "brew-cask:{}: uninstall set_ownership target lost its receipt claim: {}",
                    candidate.token,
                    path.display()
                )
            })?;
        if !cask_target_record_matches(record)? {
            bail!(
                "brew-cask:{}: uninstall set_ownership target changed after preflight: {}",
                candidate.token,
                path.display()
            );
        }
    }
    let mut args = Vec::with_capacity(step.paths.len() + 3);
    if step.recursive {
        args.push("-R".to_string());
    }
    args.push("--".to_string());
    args.push(step.owner.clone());
    args.extend(step.paths.iter().map(|path| path.display().to_string()));
    sudo::run("chown", &args, &[])
}

fn execute_uninstall_recording_in(
    state_dir: &Path,
    candidate: &CaskPruneCandidate,
    homebrew: &receipt::CaskReceipt,
    journal: &mut CaskTransactionJournal,
    label_prefix: &str,
    completed_phase: CaskTransactionPhase,
    reopen_quit_apps: bool,
) -> Result<()> {
    let ownership_steps = prepare_uninstall_ownership_steps(candidate, homebrew)?;
    let actions = homebrew_uninstall_actions(&candidate.token, homebrew)?;
    let mut pkg_plans = BTreeMap::new();
    let mut package_paths = BTreeSet::new();
    for pkg_id in actions.iter().filter_map(|action| match action {
        HomebrewUninstallAction::Pkgutil(pkg_id) => Some(pkg_id),
        _ => None,
    }) {
        let plans = prepare_pkg_removal_plans(pkg_id)?;
        package_paths.extend(plans.iter().flat_map(|plan| plan.all_paths.iter().cloned()));
        pkg_plans.insert(pkg_id.clone(), plans);
    }
    preflight_homebrew_uninstall_actions(candidate, &actions, &package_paths)?;
    for (index, step) in ownership_steps.iter().enumerate() {
        let label = format!("{label_prefix}_preflight[{index}]:set_ownership");
        set_cask_external_action_in(state_dir, journal, &label)?;
        execute_uninstall_ownership_step(candidate, step)?;
        record_cask_action_in(state_dir, journal, &label)?;
        set_cask_phase_in(state_dir, journal, completed_phase.clone())?;
    }
    for (index, action) in actions.into_iter().enumerate() {
        let label = format!("{label_prefix}[{index}]");
        let quit_was_running = quit_action_is_running(&action)?;
        if reopen_quit_apps
            && quit_was_running
            && let HomebrewUninstallAction::Quit(bundle_id) = &action
        {
            record_reopen_bundle_in(state_dir, journal, bundle_id)?;
        }
        set_cask_external_action_in(state_dir, journal, &label)?;
        execute_homebrew_uninstall_action(candidate, action, quit_was_running, &pkg_plans)?;
        record_cask_action_in(state_dir, journal, &label)?;
        set_cask_phase_in(state_dir, journal, completed_phase.clone())?;
    }
    Ok(())
}

fn preflight_homebrew_uninstall_actions(
    candidate: &CaskPruneCandidate,
    actions: &[HomebrewUninstallAction],
    package_paths: &BTreeSet<PathBuf>,
) -> Result<()> {
    for action in actions {
        match action {
            HomebrewUninstallAction::Delete(path) | HomebrewUninstallAction::Trash(path) => {
                let expanded = expand_cask_template(
                    &path.to_string_lossy(),
                    &candidate.version_dir,
                    &cask_appdir(&[])?,
                    Some(&candidate.version),
                );
                let expanded = Path::new(&expanded);
                validate_cask_delete_pattern(&candidate.token, expanded)?;
                if expanded.starts_with("/System") && expanded.symlink_metadata().is_ok() {
                    bail!(
                        "brew-cask:{}: protected uninstall target appeared before mutation: {}",
                        candidate.token,
                        expanded.display()
                    );
                }
            }
            HomebrewUninstallAction::Script {
                executable, args, ..
            } => {
                validate_uninstall_script(&candidate.token, &candidate.version, executable, args)?;
                let executable = PathBuf::from(expand_cask_template(
                    executable,
                    &candidate.version_dir,
                    &cask_appdir(&[])?,
                    Some(&candidate.version),
                ));
                let receipt_owned = candidate.receipt.targets.iter().any(|record| {
                    executable == record.path || executable.starts_with(&record.path)
                });
                let package_owned = package_paths.contains(&executable);
                if !receipt_owned
                    && !package_owned
                    && !executable.starts_with(&candidate.version_dir)
                {
                    bail!(
                        "brew-cask:{}: uninstall script is not proven receipt-owned: {}",
                        candidate.token,
                        executable.display()
                    );
                }
                if !executable.is_file() {
                    bail!(
                        "brew-cask:{}: uninstall script does not exist: {}",
                        candidate.token,
                        executable.display()
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn quit_action_is_running(action: &HomebrewUninstallAction) -> Result<bool> {
    let HomebrewUninstallAction::Quit(bundle_id) = action else {
        return Ok(false);
    };
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
        Ok(false)
    }
    #[cfg(target_os = "macos")]
    bundle_is_running(bundle_id)
}

fn record_reopen_bundle_in(
    state_dir: &Path,
    journal: &mut CaskTransactionJournal,
    bundle_id: &str,
) -> Result<()> {
    if !valid_bundle_identifier(bundle_id) {
        bail!(
            "brew-cask:{}: recorded quit bundle identifier is invalid: {bundle_id}",
            journal.token
        );
    }
    if !journal
        .reopen_bundle_ids
        .iter()
        .any(|recorded| recorded == bundle_id)
    {
        journal.reopen_bundle_ids.push(bundle_id.to_string());
        write_cask_journal_in(state_dir, journal)?;
    }
    Ok(())
}

fn reopen_predecessor_apps_recording(
    cask: &Cask,
    journal: &mut CaskTransactionJournal,
) -> Result<()> {
    for (index, bundle_id) in journal.reopen_bundle_ids.clone().iter().enumerate() {
        let label = format!("reopen_bundle[{index}]");
        set_cask_external_action(journal, &label)?;
        reopen_bundle(&cask.token, bundle_id)?;
        record_cask_action(journal, &label)?;
        set_cask_phase(journal, CaskTransactionPhase::Activated)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn bundle_is_running(bundle_id: &str) -> Result<bool> {
    const SCRIPT: &str = r#"
'use strict';
ObjC.import('stdlib');
function run(argv) {
  try { if (Application(argv[0]).running()) $.exit(0); } catch (err) {}
  $.exit(1);
}
"#;
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", SCRIPT, bundle_id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(status.success())
}

#[cfg(target_os = "macos")]
fn quit_bundle(token: &str, bundle_id: &str, was_running: bool) -> Result<()> {
    if !valid_bundle_identifier(bundle_id) {
        bail!("brew-cask:{token}: recorded quit bundle identifier is invalid: {bundle_id}");
    }
    if !was_running {
        return Ok(());
    }
    const QUIT_SCRIPT: &str = r#"
'use strict';
ObjC.import('stdlib');
function run(argv) {
  try { Application(argv[0]).quit(); } catch (err) { $.exit(1); }
}
"#;
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", QUIT_SCRIPT, bundle_id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    for _ in 0..40 {
        if !bundle_is_running(bundle_id)? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    warn!("brew-cask:{token}: application {bundle_id} did not quit within 10 seconds");
    Ok(())
}

#[cfg(target_os = "macos")]
fn reopen_bundle(token: &str, bundle_id: &str) -> Result<()> {
    if !valid_bundle_identifier(bundle_id) {
        bail!("brew-cask:{token}: recorded quit bundle identifier is invalid: {bundle_id}");
    }
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-b", bundle_id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        warn!("brew-cask:{token}: application {bundle_id} could not be reopened");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn reopen_bundle(token: &str, bundle_id: &str) -> Result<()> {
    bail!("brew-cask:{token}: reopening {bundle_id} is only available on macOS")
}

#[cfg(target_os = "macos")]
fn remove_launchctl_service(token: &str, label: &str) -> Result<()> {
    if !valid_bundle_identifier(label) {
        bail!("brew-cask:{token}: recorded launchctl label is unsupported: {label}");
    }
    // Homebrew treats an absent service and launchctl removal failure as a
    // non-fatal condition, then removes matching user/system plist files.
    let _ = std::process::Command::new("/bin/launchctl")
        .args(["remove", label])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    for path in [
        crate::dirs::HOME.join(format!("Library/LaunchAgents/{label}.plist")),
        PathBuf::from(format!("/Library/LaunchAgents/{label}.plist")),
        PathBuf::from(format!("/Library/LaunchDaemons/{label}.plist")),
    ] {
        if path.symlink_metadata().is_ok() {
            remove_artifact_target_elevating(&path)?;
        }
    }
    Ok(())
}

fn validate_cask_prune_candidate(candidate: &CaskPruneCandidate) -> Result<()> {
    let receipt = &candidate.receipt;
    let native_artifacts = candidate
        .homebrew_receipt
        .as_ref()
        .map(|homebrew| {
            parse_cask_artifacts(
                &cask_from_homebrew_receipt(&candidate.token, homebrew),
                false,
            )
        })
        .transpose()?;
    if let Some(homebrew) = &candidate.homebrew_receipt {
        let token_dir = candidate
            .version_dir
            .parent()
            .ok_or_else(|| eyre!("cask version has no token directory"))?;
        if receipt::read_cask_receipt(token_dir)? != *homebrew {
            bail!("Homebrew receipt has changed");
        }
        validate_homebrew_uninstall_artifacts(&candidate.token, homebrew)?;
    } else {
        if homebrew_metadata_present(&candidate.token) {
            bail!("Homebrew metadata appeared after planning");
        }
        if read_receipt(&candidate.version_dir)?.as_ref() != Some(receipt) {
            bail!("ownership receipt has changed");
        }
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
        .chain(&receipt.manpages)
        .chain(&receipt.completions)
        .cloned()
        .collect::<BTreeSet<_>>();
    if (expected.is_empty() && candidate.homebrew_receipt.is_none())
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
            || !allowed_appdir_roots()?
                .iter()
                .any(|root| path_is_below(path, root))
            || !moved_staged_target_matches_anywhere(record, &candidate.version_dir)
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
        let (source_is_owned, target_root_is_allowed) = match &native_artifacts {
            Some(artifacts) => {
                // Parsing the installed native artifact computed this exact
                // target through binary_target_path/target_path, including the
                // separately bounded $APPDIR target form.
                (
                    native_binary_target_is_owned(artifacts, path, &candidate.version_dir)?,
                    true,
                )
            }
            None => (
                symlink_resolves_below(path, &candidate.version_dir),
                allowed_binary_target_roots()
                    .iter()
                    .any(|root| path_is_below(path, root)),
            ),
        };
        if record.fingerprint.kind != CaskTargetKind::Symlink
            || !target_root_is_allowed
            || !source_is_owned
        {
            bail!(
                "binary target is not an owned installed-source symlink: {}",
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
            || !moved_staged_target_matches_anywhere(record, &candidate.version_dir)
        {
            bail!(
                "font target is outside the platform font directory: {}",
                path.display()
            );
        }
    }
    for path in &receipt.manpages {
        let record = records
            .get(path)
            .ok_or_else(|| eyre!("missing manpage target record"))?;
        let source_is_owned = match &native_artifacts {
            Some(artifacts) => {
                let manpage = artifacts
                    .manpages
                    .iter()
                    .find(|manpage| {
                        manpage_target_path(manpage).is_ok_and(|target| target == *path)
                    })
                    .ok_or_else(|| {
                        eyre!("missing native manpage artifact for {}", path.display())
                    })?;
                manpage_target_is_owned(manpage, &artifacts.apps, path, &candidate.version_dir)?
            }
            None => symlink_resolves_below(path, &candidate.version_dir),
        };
        if record.fingerprint.kind != CaskTargetKind::Symlink
            || !path_is_below(path, &EffectiveCaskDirs::current().manpagedir)
            || !source_is_owned
        {
            bail!(
                "manpage target is not an owned Caskroom symlink: {}",
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
        let (source_is_owned, expected_kind) = match &native_artifacts {
            Some(artifacts) => {
                let installed = cask_from_homebrew_receipt(
                    &candidate.token,
                    candidate.homebrew_receipt.as_ref().expect("checked above"),
                );
                let generated = completion_target_is_generated(&installed, artifacts, path)?;
                (
                    completion_target_is_owned(
                        &installed,
                        artifacts,
                        path,
                        &candidate.version_dir,
                    )?,
                    if generated {
                        CaskTargetKind::File
                    } else {
                        CaskTargetKind::Symlink
                    },
                )
            }
            None => (
                symlink_resolves_below(path, &candidate.version_dir),
                CaskTargetKind::Symlink,
            ),
        };
        if record.fingerprint.kind != expected_kind
            || !completion_roots
                .iter()
                .any(|root| path_is_below(path, root))
            || !source_is_owned
        {
            bail!(
                "completion target has invalid ownership or topology: {}",
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

fn moved_staged_target_matches(record: &CaskTargetRecord, staged: &Path) -> bool {
    staged
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && file::same_file(staged, &record.path)
        && cask_target_record_matches(record).unwrap_or(false)
}

fn moved_staged_target_matches_anywhere(record: &CaskTargetRecord, version_dir: &Path) -> bool {
    walkdir::WalkDir::new(version_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .any(|entry| staged_app_matches_target(record, entry.path()))
}

fn staged_app_matches_target(record: &CaskTargetRecord, staged: &Path) -> bool {
    let Ok(metadata) = staged.symlink_metadata() else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        // Preserve pruning support for casks installed before app artifacts
        // switched from retained copies to Homebrew-compatible symlinks.
        return cask_target_fingerprint(staged)
            .is_ok_and(|fingerprint| fingerprint == record.fingerprint);
    }
    moved_staged_target_matches(record, staged)
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
    target_parent: PathBuf,
    backup_parent: Option<PathBuf>,
    elevate: bool,
}

#[derive(Debug)]
struct ArtifactLinkTransaction {
    backups: Vec<ArtifactLinkBackup>,
}

fn artifact_backup_path(target: &Path) -> Result<PathBuf> {
    let parent = target
        .parent()
        .ok_or_else(|| eyre!("brew-cask: artifact target has no parent"))?;
    Ok(parent.join(format!(
        ".mise-link-backup-{}",
        hash::hash_to_str(&target.display().to_string())
    )))
}

fn validate_activation_target_claims(
    plan: &CaskTargetPlan,
    predecessor_targets: &[CaskTargetRecord],
) -> Result<()> {
    let predecessors = predecessor_targets
        .iter()
        .map(|record| (record.path.as_path(), record))
        .collect::<BTreeMap<_, _>>();
    if predecessors.len() != predecessor_targets.len() {
        bail!("brew-cask: predecessor target inventory contains duplicates");
    }
    for target in &plan.artifact_activation_targets {
        if target.symlink_metadata().is_err() {
            continue;
        }
        let predecessor = predecessors.get(target.as_path()).ok_or_else(|| {
            eyre!(
                "brew-cask: artifact target '{}' already exists with ambiguous ownership",
                target.display()
            )
        })?;
        if !cask_target_record_matches(predecessor)? {
            bail!(
                "brew-cask: predecessor artifact target '{}' changed after ownership validation",
                target.display()
            );
        }
    }
    Ok(())
}

impl ArtifactLinkTransaction {
    fn begin(mut targets: Vec<PathBuf>, predecessor_targets: &[CaskTargetRecord]) -> Result<Self> {
        dedup_paths_preserving_order(&mut targets);
        validate_activation_target_claims(
            &CaskTargetPlan {
                receipt_inventory_targets: targets.clone(),
                artifact_activation_targets: targets.clone(),
            },
            predecessor_targets,
        )?;
        let mut transaction = Self {
            backups: Vec::with_capacity(targets.len()),
        };
        for target in targets {
            let entry = (|| -> Result<ArtifactLinkBackup> {
                let backup = if target.symlink_metadata().is_ok() {
                    let backup = artifact_backup_path(&target)?;
                    remove_artifact_target_elevating(&backup)?;
                    rename_elevating(&target, &backup)?;
                    Some(backup)
                } else {
                    None
                };
                let target_parent = resolved_parent(&target)?;
                let backup_parent = backup.as_deref().map(resolved_parent).transpose()?;
                Ok(ArtifactLinkBackup {
                    target,
                    backup,
                    target_parent,
                    backup_parent,
                    elevate: true,
                })
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

impl Drop for ArtifactLinkTransaction {
    fn drop(&mut self) {
        if !self.backups.is_empty()
            && let Err(error) = self.rollback()
        {
            warn!("brew-cask: failed to roll back artifact targets: {error:#}");
        }
    }
}

#[derive(Debug)]
struct CaskroomActivationTransaction {
    destination: PathBuf,
    backup: PathBuf,
    had_previous: bool,
    had_previous_metadata: bool,
}

impl CaskroomActivationTransaction {
    fn rollback(&mut self) -> Result<()> {
        rollback_homebrew_metadata(&self.destination, self.had_previous_metadata)?;
        file::remove_all(&self.destination)?;
        if self.had_previous && self.backup.symlink_metadata().is_ok() {
            file::rename(&self.backup, &self.destination)?;
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        file::remove_all(&self.backup)?;
        commit_homebrew_metadata(&self.destination)?;
        Ok(())
    }
}

fn replace_caskroom(
    cask: &Cask,
    staged: &Path,
    destination: &Path,
    link_artifacts: impl FnOnce() -> Result<()>,
) -> Result<CaskroomActivationTransaction> {
    let backup = caskroom_backup_dir(cask);
    file::remove_all(&backup)?;
    let had_previous = destination.symlink_metadata().is_ok();
    let had_previous_metadata = destination
        .parent()
        .is_some_and(|parent| parent.join(".metadata").symlink_metadata().is_ok());
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
            rollback_homebrew_metadata(destination, had_previous_metadata)?;
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
    Ok(CaskroomActivationTransaction {
        destination: destination.to_path_buf(),
        backup,
        had_previous,
        had_previous_metadata,
    })
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

    use crate::test::EnvVarGuard;

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

    fn write_homebrew_cask_receipt(token: &str, version: &str, mutate: impl FnOnce(&mut Value)) {
        let token_dir = caskroom_token_dir(token);
        file::create_dir_all(token_dir.join(".metadata")).unwrap();
        file::create_dir_all(token_dir.join(version)).unwrap();
        let mut receipt: Value =
            serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json")).unwrap();
        receipt["source"]["version"] = Value::String(version.to_string());
        mutate(&mut receipt);
        file::write(
            token_dir.join(".metadata/INSTALL_RECEIPT.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        file::write(
            token_dir.join(".metadata/config.json"),
            native_cask_config().unwrap().to_json_bytes().unwrap(),
        )
        .unwrap();
    }

    /// A temporary directory whose ancestors pass `ensure_trusted_appdir`'s
    /// trust checks.
    ///
    /// The system temp directory is unusable for these tests on Linux because
    /// `/tmp` is world-writable (mode 1777) and the trusted walk correctly
    /// refuses to operate through it. Real application directories
    /// (`/Applications`, `~/Applications`) are never world-writable, so anchor
    /// the fixture under the test home instead.
    fn trusted_tempdir() -> Result<tempfile::TempDir> {
        let base = &*crate::env::HOME;
        file::create_dir_all(base)?;
        Ok(tempfile::Builder::new()
            .prefix(".mise-cask-appdir-")
            .tempdir_in(base)?)
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
            depends_on: CaskDependencies::default(),
            conflicts_with: CaskConflicts::default(),
            ruby_source_path: None,
            ruby_source_checksum: None,
            tap_git_head: None,
            tap: Some("homebrew/cask".to_string()),
            auto_updates: false,
            raw_base: None,
            definition_source: "https://formulae.brew.sh/api/cask/example.json".to_string(),
            loaded_from_internal_api: false,
            platform_policy: CaskPlatformPolicy::Unspecified,
            resolved_formula_dependencies: Vec::new(),
            resolved_cask_dependencies: Vec::new(),
        }
    }

    #[test]
    fn uninstall_artifacts_match_homebrew_order_and_shape() -> Result<()> {
        let mut codex = test_cask("codex", "1.2.3");
        codex.artifacts = serde_json::from_value(serde_json::json!([
            {"binary": ["codex-aarch64-apple-darwin", {"target": "codex"}]},
            {
                "bash_completion": ["$APPDIR/Codex.app/completions/codex.bash"],
                "target": "$HOMEBREW_PREFIX/etc/bash_completion.d/codex"
            },
            {"generate_completions_from_executable": ["codex", "completion"]},
            {"postflight": null},
            {"zap": [{"trash": "~/.codex"}]}
        ]))
        .unwrap();
        assert_eq!(
            cask_uninstall_artifacts(&codex)?,
            serde_json::from_value::<Vec<Value>>(serde_json::json!([
                {"binary": ["codex-aarch64-apple-darwin", {"target": "codex"}]},
                {
                    "bash_completion": [format!(
                        "{}/Codex.app/completions/codex.bash",
                        EffectiveCaskDirs::current().appdir.display()
                    )]
                },
                {"generate_completions_from_executable": [
                    "codex",
                    "completion",
                    {
                        "base_name": null,
                        "shell_parameter_format": null,
                        "shells": ["bash", "zsh", "fish"]
                    }
                ]},
                {"zap": [{"trash": "~/.codex"}]}
            ]))
            .unwrap()
        );

        let mut pkg = test_cask("example", "2.0");
        pkg.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Example.app"], "target": "/Applications/Example.app"},
            {"pkg": ["Example.pkg"]},
            {"uninstall": [{"pkgutil": "com.example.pkg"}]},
            {"postflight_steps": [{"steps": [{"type": "terminate_process", "name": "Example"}]}]},
            {"uninstall_postflight": null},
            {"zap": [{"trash": "~/Library/Application Support/Example"}]}
        ]))
        .unwrap();
        assert_eq!(
            cask_uninstall_artifacts(&pkg)?,
            serde_json::from_value::<Vec<Value>>(serde_json::json!([
                {"uninstall": [{"pkgutil": "com.example.pkg"}]},
                {"app": ["Example.app"]},
                {"postflight_steps": [{"steps": [{"type": "terminate_process", "name": "Example"}]}]},
                {"uninstall_postflight": null},
                {"zap": [{"trash": "~/Library/Application Support/Example"}]}
            ]))
            .unwrap()
        );

        let mut font = test_cask("font-example", "1.0");
        font.artifacts = (0..16)
            .map(|index| serde_json::json!({"font": [format!("font-{index}.ttf")]}))
            .collect();
        let ordered = cask_uninstall_artifacts(&font)?;
        let sources = ordered
            .iter()
            .map(|artifact| artifact["font"][0].as_str().unwrap())
            .collect::<Vec<_>>();
        #[cfg(target_os = "macos")]
        assert_eq!(
            sources,
            [
                "font-15.ttf",
                "font-1.ttf",
                "font-2.ttf",
                "font-3.ttf",
                "font-4.ttf",
                "font-5.ttf",
                "font-6.ttf",
                "font-7.ttf",
                "font-8.ttf",
                "font-9.ttf",
                "font-10.ttf",
                "font-11.ttf",
                "font-12.ttf",
                "font-13.ttf",
                "font-14.ttf",
                "font-0.ttf",
            ]
        );
        #[cfg(not(target_os = "macos"))]
        {
            let actual = sources.into_iter().collect::<BTreeSet<_>>();
            let expected = (0..16)
                .map(|index| format!("font-{index}.ttf"))
                .collect::<BTreeSet<_>>();
            assert_eq!(
                actual,
                expected.iter().map(String::as_str).collect::<BTreeSet<_>>()
            );
        }
        Ok(())
    }

    #[test]
    fn uninstall_receipt_expands_homebrew_api_placeholders() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.remove(APP_DIR_ENV);
        let value = serde_json::json!({
            "paths": [
                "/$HOME/example",
                "$HOMEBREW_PREFIX/bin/example",
                "$HOMEBREW_CELLAR/example/1.0",
                "$APPDIR/Example.app"
            ]
        });
        assert_eq!(
            expand_homebrew_cask_placeholders(value)?,
            serde_json::json!({
                "paths": [
                    crate::dirs::HOME.join("example"),
                    prefix::prefix().join("bin/example"),
                    prefix::prefix().join("Cellar/example/1.0"),
                    EffectiveCaskDirs::current().appdir.join("Example.app")
                ]
            })
        );
        Ok(())
    }

    #[test]
    fn homebrew_metadata_writer_creates_complete_set() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let caskroom = dir.path().join("Caskroom/example/1.2.3");
        file::create_dir_all(&caskroom)?;
        let mut cask = test_cask("example", "1.2.3");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Example.app"]},
            {"zap": [{"trash": "~/.example"}]}
        ]))?;

        write_homebrew_metadata(&caskroom, &cask, &serde_json::Map::new(), false)?;

        let token_dir = dir.path().join("Caskroom/example");
        let receipt = receipt::read_cask_receipt(&token_dir)?;
        assert_eq!(receipt.homebrew_version, receipt::EMULATED_BREW_VERSION);
        assert_eq!(receipt.source.version, "1.2.3");
        assert_eq!(
            receipt.uninstall_artifacts,
            cask_uninstall_artifacts(&cask)?
        );
        assert!(token_dir.join(".metadata/config.json").is_file());
        let snapshot_dir = receipt::newest_cask_metadata_dir(&token_dir, "1.2.3")?.unwrap();
        assert_eq!(
            std::fs::read_to_string(snapshot_dir.join("Casks/example.json"))?,
            "{}"
        );
        assert!(!caskroom.join(".mise-cask.toml").exists());
        Ok(())
    }

    #[test]
    fn installed_and_auto_update_casks_are_noops() {
        let mut cask = test_cask("example", "2.0");
        let installed = InstalledCaskState::Installed("1.0".to_string());
        assert_eq!(
            existing_install_noop(&installed, &cask, false),
            Some("1.0".to_string())
        );
        assert_eq!(existing_install_noop(&installed, &cask, true), None);
        cask.auto_updates = true;
        assert_eq!(
            existing_install_noop(&installed, &cask, true),
            Some("1.0".to_string())
        );
        assert_eq!(
            existing_install_noop(
                &InstalledCaskState::NeedsRepair {
                    installed: Some("1.0".to_string()),
                    reason: "corrupt receipt".to_string(),
                    replacement_safe: false,
                },
                &cask,
                false,
            ),
            None
        );
    }

    #[test]
    fn auto_update_status_is_offline_and_accepts_owned_app_drift() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("self-updating", "1.0.0");
        cask.auto_updates = true;
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Example.app"]},
            {"binary": ["bin/example"]}
        ]))?;
        let artifacts = cask_artifacts(&cask)?;
        let target = app_target_path("Example.app")?;
        file::create_dir_all(target.join("Contents"))?;
        file::write(target.join("Contents/version"), "downloaded")?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        file::make_symlink(&target, &version_dir.join("Example.app"))?;
        let executable = version_dir.join("bin/example");
        let binary_target = prefix::prefix().join("bin/example");
        file::create_dir_all(executable.parent().unwrap())?;
        file::create_dir_all(binary_target.parent().unwrap())?;
        file::write(&executable, "example")?;
        file::make_symlink(&executable, &binary_target)?;
        write_homebrew_metadata(&version_dir, &cask, &serde_json::Map::new(), false)?;
        let metadata_only_apps = BTreeSet::from([target.clone()]);
        write_auxiliary_cask_receipt_with_flight_targets(
            &cask,
            &artifacts,
            &[],
            &BTreeMap::new(),
            &[],
            &metadata_only_apps,
        )?;

        file::write(target.join("Contents/version"), "updated by app")?;
        let status = installed_cask_status(&PackageRequest {
            name: cask.token.clone(),
            version: None,
            tap_url: None,
        })?;
        assert!(matches!(
            status.state,
            PackageState::InstalledAutoUpdates { version } if version == cask.version
        ));
        let auxiliary = read_auxiliary_cask_receipt(&cask.token, &cask.version)?.unwrap();
        assert!(auxiliary.prune_safe);
        assert!(auxiliary.prune_blocker.is_none());
        assert!(!version_dir.join(".mise-cask.toml").exists());
        let app_record = auxiliary
            .targets
            .iter()
            .find(|record| record.path == target)
            .unwrap();
        assert!(!cask_target_record_matches(app_record)?);
        assert!(target.is_dir());
        assert!(version_dir.is_dir());

        file::remove_all(&target)?;
        let InstalledCaskState::NeedsRepair {
            replacement_safe, ..
        } = installed_cask_state(&cask, &artifacts)?
        else {
            panic!("missing self-updating app must need repair");
        };
        assert!(replacement_safe);

        file::remove_file(version_dir.join("Example.app"))?;
        file::create_dir_all(target.join("Contents"))?;
        file::write(target.join("Contents/version"), "foreign")?;
        file::remove_file(binary_target)?;
        let InstalledCaskState::NeedsRepair {
            replacement_safe, ..
        } = installed_cask_state(&cask, &artifacts)?
        else {
            panic!("foreign self-updating app must need repair");
        };
        assert!(!replacement_safe);

        let native_receipt = caskroom_token_dir(&cask.token)
            .join(".metadata")
            .join("INSTALL_RECEIPT.json");
        let mut native_body = file::read_to_string(&native_receipt)?;
        native_body.push('\n');
        file::write(&native_receipt, native_body)?;
        assert!(read_auxiliary_cask_receipt(&cask.token, &cask.version)?.is_none());
        Ok(())
    }

    #[test]
    fn adopted_app_requires_exact_fingerprint_but_ignores_later_content_drift() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("adopted", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Example.app"]}
        ]))?;
        let artifacts = cask_artifacts(&cask)?;
        let stage = tmp.path().join("stage");
        let source = stage.join("Example.app/Contents");
        let target = app_target_path("Example.app")?;
        file::create_dir_all(&source)?;
        file::create_dir_all(target.join("Contents"))?;
        file::write(source.join("version"), "identical")?;
        file::write(target.join("Contents/version"), "identical")?;
        let metadata_only_apps = BTreeSet::from([target.clone()]);
        validate_adoptable_apps(&stage, &artifacts.apps, &metadata_only_apps)?;

        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        file::make_symlink(&target, &version_dir.join("Example.app"))?;
        write_homebrew_metadata(&version_dir, &cask, &serde_json::Map::new(), false)?;
        write_auxiliary_cask_receipt_with_flight_targets(
            &cask,
            &artifacts,
            &[],
            &BTreeMap::new(),
            &[],
            &metadata_only_apps,
        )?;
        assert!(
            validate_installed_cask_topology_with_metadata(
                &cask,
                &artifacts,
                &version_dir,
                false,
                &metadata_only_apps,
                None,
            )
            .is_err()
        );
        let receipt = read_auxiliary_cask_receipt(&cask.token, &cask.version)?.unwrap();
        validate_installed_cask_topology_with_metadata(
            &cask,
            &artifacts,
            &version_dir,
            false,
            &metadata_only_apps,
            Some(&receipt.targets),
        )?;
        let request = PackageRequest {
            name: cask.token.clone(),
            version: None,
            tap_url: None,
        };
        assert!(matches!(
            installed_cask_status(&request)?.state,
            PackageState::Installed { version } if version == cask.version
        ));

        file::write(target.join("Contents/version"), "foreign drift")?;
        assert!(matches!(
            installed_cask_status(&request)?.state,
            PackageState::Installed { version } if version == cask.version
        ));
        // Adoption remains fingerprint-bound; only post-adoption health ignores
        // app content drift to avoid replacing the bundle and resetting TCC.
        assert!(validate_adoptable_apps(&stage, &artifacts.apps, &metadata_only_apps).is_err());
        Ok(())
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
        file::create_dir_all(&version_dir)?;
        file::make_symlink(&target, &version_dir.join(app_name))?;
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
    #[cfg(unix)]
    fn staged_app_accepts_target_symlink_and_legacy_copy() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("Applications/Example.app");
        file::create_dir_all(target.join("Contents"))?;
        file::write(target.join("Contents/app"), "content")?;
        let record = CaskTargetRecord {
            path: target.clone(),
            fingerprint: cask_target_fingerprint(&target)?,
            uninstall: None,
        };

        let staged_link = tmp.path().join("Caskroom/example/1.0.0/Example.app");
        file::create_dir_all(staged_link.parent().unwrap())?;
        file::make_symlink(&target, &staged_link)?;
        assert!(staged_app_matches_target(&record, &staged_link));

        file::remove_file(&staged_link)?;
        file::copy_dir_all_preserve_symlinks(&target, &staged_link)?;
        assert!(staged_app_matches_target(&record, &staged_link));

        file::remove_all(&staged_link)?;
        file::make_symlink(&tmp.path().join("Applications/Other.app"), &staged_link)?;
        assert!(!staged_app_matches_target(&record, &staged_link));
        Ok(())
    }

    #[test]
    fn legacy_receipt_ignores_app_bundle_content_drift() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Example.app"], "target": "$HOMEBREW_PREFIX/Applications/Example.app"}
        ]))?;
        let artifacts = cask_artifacts(&cask)?;
        let app = artifacts.apps[0].clone();
        let app_target = app_target_path(app.target_name())?;
        file::create_dir_all(app_target.join("Contents"))?;
        crate::file::write(app_target.join("Contents/app"), "original")?;
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        write_receipt_with_flight_targets(
            &caskroom,
            &cask,
            &artifacts,
            &[],
            &BTreeMap::new(),
            &[],
            &BTreeSet::new(),
        )?;
        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some(cask.version.clone())
        );
        crate::file::write(app_target.join("Contents/app"), "changed")?;
        // Content drift must not look like "missing" — that would reinstall the
        // app on the next apply and revoke macOS TCC grants.
        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some(cask.version.clone())
        );
        assert!(!cask_target_record_matches(
            read_receipt(&caskroom)?
                .expect("receipt")
                .targets
                .first()
                .expect("app target")
        )?);
        assert!(matches!(
            validate_legacy_cask(&cask, installed_cask_state(&cask, &artifacts)?)?,
            InstalledCaskState::LegacyMise(_)
        ));
        Ok(())
    }

    #[test]
    fn completed_receipt_missing_app_is_not_installed() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Example.app"], "target": "$HOMEBREW_PREFIX/Applications/Example.app"}
        ]))?;
        let artifacts = cask_artifacts(&cask)?;
        let app = artifacts.apps[0].clone();
        let app_target = app_target_path(app.target_name())?;
        file::create_dir_all(app_target.join("Contents"))?;
        crate::file::write(app_target.join("Contents/app"), "original")?;
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        write_receipt_with_flight_targets(
            &caskroom,
            &cask,
            &artifacts,
            &[],
            &BTreeMap::new(),
            &[],
            &BTreeSet::new(),
        )?;
        file::remove_all(&app_target)?;
        assert!(matches!(
            validate_legacy_cask(&cask, installed_cask_state(&cask, &artifacts)?)?,
            InstalledCaskState::NeedsRepair { reason, .. }
                if reason.contains("recorded target is missing")
        ));
        Ok(())
    }

    #[test]
    fn cask_target_present_checks_symlink_destination_and_kinds() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let dest = tmp.path().join("bin/tool");
        file::create_dir_all(dest.parent().unwrap())?;
        crate::file::write(&dest, "tool")?;
        let link = tmp.path().join("prefix/bin/tool");
        file::create_dir_all(link.parent().unwrap())?;
        file::make_symlink(&dest, &link)?;
        let record = CaskTargetRecord {
            path: link.clone(),
            fingerprint: cask_target_fingerprint(&link)?,
            uninstall: None,
        };
        assert!(cask_target_present(&record));

        file::remove_file(&dest)?;
        assert!(
            !cask_target_present(&record),
            "dangling symlink must not count as present"
        );

        crate::file::write(&dest, "tool")?;
        let other = tmp.path().join("bin/other");
        crate::file::write(&other, "other")?;
        file::remove_file(&link)?;
        file::make_symlink(&other, &link)?;
        assert!(
            !cask_target_present(&record),
            "retargeted symlink must not count as present"
        );

        file::remove_file(&link)?;
        crate::file::write(&link, "not a symlink")?;
        assert!(
            !cask_target_present(&record),
            "file replacing a symlink must not count as present"
        );

        let font = tmp.path().join("fonts/Example.ttf");
        file::create_dir_all(font.parent().unwrap())?;
        crate::file::write(&font, "font")?;
        let file_record = CaskTargetRecord {
            path: font.clone(),
            fingerprint: cask_target_fingerprint(&font)?,
            uninstall: None,
        };
        assert!(cask_target_present(&file_record));
        crate::file::write(&font, "changed font bytes")?;
        assert!(
            cask_target_present(&file_record),
            "file content drift is ignored for install health"
        );
        file::remove_file(&font)?;
        file::create_dir_all(&font)?;
        assert!(
            !cask_target_present(&file_record),
            "directory replacing a file must not count as present"
        );
        Ok(())
    }

    #[test]
    fn self_updating_receipt_accepts_app_bundle_drift() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("self-updating", "1.0.0");
        cask.auto_updates = true;
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
        crate::file::write(app_target.join("Contents/app"), "downloaded")?;
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        write_receipt_with_flight_targets(
            &caskroom,
            &cask,
            &artifacts,
            &[],
            &BTreeMap::new(),
            &[],
            &BTreeSet::new(),
        )?;

        crate::file::write(app_target.join("Contents/app"), "updated by app")?;
        cask.version = "2.0.0".to_string();
        assert_eq!(
            installed_cask_version(&cask, &artifacts)?,
            Some("1.0.0".to_string())
        );
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
                env: IndexMap::new(),
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
            env: IndexMap::from([
                ("FIREFOX_MODE".to_string(), "mise test".to_string()),
                ("FIREFOX_ROOT".to_string(), "{{staged_path}}".to_string()),
            ]),
        };

        stage_command_wrapper(&caskroom, &appdir, &cask, &wrapper)?;
        file::rename(&caskroom, &final_caskroom)?;
        link_command_wrapper(&final_caskroom, &wrapper)?;

        let staged = final_caskroom.join(".homebrew-command-wrappers/firefox");
        let contents = file::read_to_string(&staged)?;
        let executable = appdir.join("Firefox.app/Contents/MacOS/firefox");
        assert_eq!(
            contents,
            format!(
                "#!/bin/bash\nFIREFOX_MODE=\"mise test\" FIREFOX_ROOT=\"{}\" exec \"{}\" --profile two\\ words 153.0.1 \"$@\"\n",
                final_caskroom.display(),
                executable.display()
            )
        );
        assert_eq!(staged.metadata()?.permissions().mode() & 0o777, 0o555);
        assert_eq!(std::fs::read_link(wrapper.target_path()?)?, staged);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn stages_command_wrapper_without_args_like_homebrew() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let cask = test_cask("vlc", "3.0.23");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let appdir = Path::new("/Applications");
        let wrapper = CommandWrapperArtifact {
            name: "vlc".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/vlc".to_string()),
            content: None,
            executable: Some("$APPDIR/VLC.app/Contents/MacOS/VLC".to_string()),
            args: Vec::new(),
            env: IndexMap::new(),
        };

        stage_command_wrapper(&caskroom, appdir, &cask, &wrapper)?;

        let staged = wrapper.caskroom_path(&caskroom);
        assert_eq!(
            file::read_to_string(&staged)?,
            "#!/bin/bash\nexec \"/Applications/VLC.app/Contents/MacOS/VLC\"  \"$@\"\n"
        );
        assert_eq!(staged.metadata()?.permissions().mode() & 0o777, 0o555);
        Ok(())
    }

    #[test]
    fn command_wrapper_rendering_fails_closed_before_staging() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let cask = test_cask("example", "1.0.0");
        let caskroom = tmp.path().join("Caskroom/example/1.0.0");
        let wrapper = CommandWrapperArtifact {
            name: "example".to_string(),
            target: None,
            content: None,
            executable: Some("/Applications/$UNTRUSTED/example".to_string()),
            args: Vec::new(),
            env: IndexMap::new(),
        };

        let err = stage_command_wrapper(&caskroom, Path::new("/Applications"), &cask, &wrapper)
            .unwrap_err()
            .to_string();
        assert!(err.contains("executable cannot be represented safely"));
        assert!(!caskroom.exists());
        assert_eq!(
            homebrew_shell_escape("two words; true")?,
            "two\\ words\\;\\ true"
        );
        assert_eq!(
            homebrew_shell_escape("channel+nightly")?,
            "channel\\+nightly"
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn stages_command_wrapper_with_literal_content() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
            env: IndexMap::new(),
        };

        stage_command_wrapper(&caskroom, Path::new("/Applications"), &cask, &wrapper)?;

        assert_eq!(
            file::read_to_string(caskroom.join(".homebrew-command-wrappers/example"))?,
            format!(
                "#!/bin/sh\nHOME=$HOME\nSTAGE={{{{staged_path}}}}\nexec '{}/bin/example' \"$@\"\n",
                prefix.display()
            )
        );
        assert_eq!(
            caskroom
                .join(".homebrew-command-wrappers/example")
                .metadata()?
                .permissions()
                .mode()
                & 0o777,
            0o755
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
                network_access: false,
                guards: Vec::new(),
            }]
        );
        Ok(())
    }

    #[test]
    fn parses_structured_symlink_steps() -> Result<()> {
        let mut cask = test_cask("docker-desktop", "4.86.0,236216");
        cask.artifacts = vec![
            serde_json::json!({"app": "Docker.app"}),
            serde_json::json!({
                "postflight_steps": [{
                    "steps": [{
                        "type": "symlink",
                        "source": {"path": "{{appdir}}/Docker.app/Contents/Resources/bin/kubectl"},
                        "target": {"path": "/usr/local/bin/kubectl"},
                        "force": true,
                        "uninstall": true,
                        "sudo": "if_needed",
                        "guards": [{
                            "condition": "unless_exists",
                            "path": "/usr/local/bin/kubectl",
                            "id": "1"
                        }]
                    }]
                }]
            }),
        ];

        let artifacts = cask_artifacts(&cask)?;
        assert!(matches!(
            artifacts.postflight_steps.as_slice(),
            [FlightStep::Symlink {
                force: true,
                uninstall: true,
                sudo: FlightSudo::IfNeeded,
                guards,
                ..
            }] if guards.len() == 1
        ));
        Ok(())
    }

    #[test]
    fn parses_gcloud_copy_installer_and_run_metadata() -> Result<()> {
        let mut cask = test_cask("gcloud-cli", "580.0.0");
        cask.artifacts = vec![
            serde_json::json!({
                "preflight_steps": [{"steps": [{
                    "type": "copy",
                    "source": {"base": "staged_path", "path": "google-cloud-sdk/."},
                    "target": {"base": "homebrew_prefix", "path": "share/google-cloud-sdk"},
                    "recursive": true
                }]}]
            }),
            serde_json::json!({
                "installer": [{"script": {
                    "executable": "google-cloud-sdk/install.sh",
                    "args": ["--quiet", "--install-python", "false"]
                }}]
            }),
            serde_json::json!({"binary": "google-cloud-sdk/bin/gcloud"}),
            serde_json::json!({
                "postflight_steps": [{"steps": [{
                    "type": "run",
                    "command": {"base": "homebrew_prefix", "path": "share/google-cloud-sdk/bin/gcloud"},
                    "args": ["version"],
                    "network_access": true
                }]}]
            }),
        ];

        let artifacts = cask_artifacts(&cask)?;
        assert!(matches!(
            artifacts.preflight_steps.as_slice(),
            [FlightStep::Copy {
                recursive: true,
                overwrite: true,
                ..
            }]
        ));
        assert_eq!(
            artifacts.installers,
            [InstallerArtifact {
                executable: "google-cloud-sdk/install.sh".to_string(),
                args: vec![
                    "--quiet".to_string(),
                    "--install-python".to_string(),
                    "false".to_string()
                ],
            }]
        );
        assert!(matches!(
            artifacts.postflight_steps.as_slice(),
            [FlightStep::Run { .. }]
        ));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn installer_script_is_made_executable_before_running() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let _guard = BrewPrefixGuard::set(&prefix);
        file::create_dir_all(prefix.join("bin"))?;
        file::create_dir_all(prefix.join("sbin"))?;
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        let script = stage.join("install.sh");
        let marker = tmp.path().join("installed");
        file::write(&script, "#!/bin/sh\nprintf '%s' \"$PATH\" > \"$1\"\n")?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644))?;
        let installer = InstallerArtifact {
            executable: "install.sh".to_string(),
            args: vec![marker.display().to_string()],
        };

        run_installer_artifact(&stage, &installer, &BTreeSet::new())?;

        let installed_path = file::read_to_string(marker)?;
        let installed_paths = std::env::split_paths(std::ffi::OsStr::new(&installed_path))
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(installed_paths, [prefix.join("bin"), prefix.join("sbin")]);
        assert_ne!(script.metadata()?.permissions().mode() & 0o111, 0);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn installer_script_rejects_paths_outside_stage() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        let outside = tmp.path().join("outside.sh");
        file::write(&outside, "#!/bin/sh\nexit 0\n")?;
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644))?;
        file::make_symlink(&outside, &stage.join("linked.sh"))?;

        for executable in [
            outside.display().to_string(),
            "../outside.sh".to_string(),
            "linked.sh".to_string(),
        ] {
            let err = run_installer_artifact(
                &stage,
                &InstallerArtifact {
                    executable,
                    args: Vec::new(),
                },
                &BTreeSet::new(),
            )
            .unwrap_err()
            .to_string();

            assert!(err.contains("outside trusted installer roots"));
            assert_eq!(outside.metadata()?.permissions().mode() & 0o111, 0);
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn installer_script_accepts_preflight_copied_root() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let _guard = BrewPrefixGuard::set(&prefix);
        file::create_dir_all(prefix.join("bin"))?;
        file::create_dir_all(prefix.join("sbin"))?;
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        let copied = prefix.join("share/example");
        file::create_dir_all(&copied)?;
        let marker = tmp.path().join("installed");
        file::write(
            copied.join("install.sh"),
            "#!/bin/sh\nprintf installed > \"$1\"\n",
        )?;
        file::make_symlink(&copied, &stage.join("payload"))?;

        let copied_files = BTreeSet::from([file::desymlink_path(&copied.join("install.sh"))]);
        run_installer_artifact(
            &stage,
            &InstallerArtifact {
                executable: "payload/install.sh".to_string(),
                args: vec![marker.display().to_string()],
            },
            &copied_files,
        )?;

        assert_eq!(file::read_to_string(marker)?, "installed");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn installer_script_rejects_unrecorded_file_beneath_copied_target() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        let broad_target = tmp.path().join("prefix");
        file::create_dir_all(broad_target.join("bin"))?;
        let outside = broad_target.join("bin/existing.sh");
        file::write(&outside, "#!/bin/sh\nexit 0\n")?;
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644))?;
        file::make_symlink(&broad_target, &stage.join("payload"))?;
        let copied_files = BTreeSet::from([broad_target.join("copied.txt")]);

        let err = run_installer_artifact(
            &stage,
            &InstallerArtifact {
                executable: "payload/bin/existing.sh".to_string(),
                args: Vec::new(),
            },
            &copied_files,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("outside trusted installer roots"));
        assert_eq!(outside.metadata()?.permissions().mode() & 0o111, 0);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn installer_mutations_are_included_in_durable_symlink_sources() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let _guard = BrewPrefixGuard::set(&prefix);
        file::create_dir_all(prefix.join("bin"))?;
        file::create_dir_all(prefix.join("sbin"))?;
        let stage = tmp.path().join("stage");
        let source = stage.join("payload");
        file::create_dir_all(&source)?;
        let script = stage.join("install.sh");
        file::write(&script, "#!/bin/sh\nprintf mutated > \"$1\"\n")?;
        let installer = InstallerArtifact {
            executable: "install.sh".to_string(),
            args: vec![source.join("generated").display().to_string()],
        };
        let target = tmp.path().join("share/example");
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&source, &target)?;
        let mut targets = FlightTargetTransaction::default();
        targets.record_installed(target);
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");

        run_installers_before_durabilizing(
            &stage,
            &temporary_caskroom,
            &[installer],
            &mut targets,
            |_| Ok(()),
        )?;

        assert_eq!(
            file::read_to_string(temporary_caskroom.join(".homebrew-staged/payload/generated"))?,
            "mutated"
        );
        Ok(())
    }

    #[test]
    fn structured_copy_restores_external_target_and_tracks_unmodified_state() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let _guard = BrewPrefixGuard::set(&prefix);
        let stage = tmp.path().join("stage");
        let source = stage.join("google-cloud-sdk");
        file::create_dir_all(&source)?;
        file::write(source.join("gcloud"), "sdk")?;
        let target = prefix.join("share/google-cloud-sdk");
        file::create_dir_all(&target)?;
        file::write(target.join("old"), "old")?;
        let step = FlightStep::Copy {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "google-cloud-sdk/.".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::HomebrewPrefix,
                path: "share/google-cloud-sdk".to_string(),
            },
            recursive: true,
            overwrite: true,
            source_glob: false,
            guards: Vec::new(),
        };
        let mut targets = FlightTargetTransaction::default();
        let cask = test_cask("gcloud-cli", "580.0.0");
        execute_flight_steps_with_completion(
            &cask,
            std::slice::from_ref(&step),
            &stage,
            Path::new("/Applications"),
            "preflight_steps",
            &mut targets,
            |_, _| Ok(()),
        )?;
        assert!(target.join("gcloud").is_file());
        assert!(!target.join("old").exists());
        assert!(targets.installed_targets().is_empty());
        assert_eq!(
            targets.copied_files(),
            &BTreeSet::from([file::desymlink_path(&target.join("gcloud"))])
        );
        let artifacts = CaskArtifacts {
            preflight_steps: vec![step],
            ..Default::default()
        };
        let records = structured_copy_target_records(&cask, &artifacts, &stage, false)?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, target);
        assert_eq!(records[0].uninstall, Some(false));
        file::write(target.join("gcloud"), "user change")?;
        assert!(structured_copy_target_records(&cask, &artifacts, &stage, false).is_err());
        targets.rollback()?;
        assert_eq!(file::read_to_string(target.join("old"))?, "old");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_replacement_targets_reject_metadata_errors() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let _guard = BrewPrefixGuard::set(&prefix);
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        file::write(stage.join("payload"), "payload")?;
        file::create_dir_all(&prefix)?;
        file::write(prefix.join("blocked"), "not a directory")?;
        let source = FlightPath {
            base: FlightPathBase::StagedPath,
            path: "payload".to_string(),
        };
        let target = FlightPath {
            base: FlightPathBase::HomebrewPrefix,
            path: "blocked/target".to_string(),
        };
        let cask = test_cask("structured-errors", "1.0.0");
        let copy_artifacts = CaskArtifacts {
            preflight_steps: vec![FlightStep::Copy {
                source: source.clone(),
                target: target.clone(),
                recursive: false,
                overwrite: true,
                source_glob: false,
                guards: vec![],
            }],
            ..Default::default()
        };
        assert!(structured_copy_target_records(&cask, &copy_artifacts, &stage, true).is_err());

        let symlink_artifacts = CaskArtifacts {
            postflight_steps: vec![FlightStep::Symlink {
                source,
                target,
                force: false,
                uninstall: false,
                source_glob: false,
                sudo: FlightSudo::Never,
                guards: vec![],
            }],
            ..Default::default()
        };
        assert!(
            structured_symlink_target_records(&cask, &symlink_artifacts, &stage, true).is_err()
        );
        Ok(())
    }

    #[test]
    fn structured_copy_rollback_removes_target_with_created_parent() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let _guard = BrewPrefixGuard::set(&prefix);
        let stage = tmp.path().join("stage");
        let source = stage.join("payload");
        file::create_dir_all(&source)?;
        file::write(source.join("installed"), "content")?;
        let target = prefix.join("share/new-parent/payload");
        let copy = FlightStep::Copy {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "payload/.".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::HomebrewPrefix,
                path: "share/new-parent/payload".to_string(),
            },
            recursive: true,
            overwrite: true,
            source_glob: false,
            guards: Vec::new(),
        };
        let fail = FlightStep::Copy {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "missing".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::HomebrewPrefix,
                path: "share/unused".to_string(),
            },
            recursive: false,
            overwrite: true,
            source_glob: false,
            guards: Vec::new(),
        };

        let err = execute_flight_steps(
            &test_cask("example", "1.0.0"),
            &[copy, fail],
            &stage,
            Path::new("/Applications"),
            "preflight_steps",
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("was not found"));
        assert!(target.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn cask_metadata_accepts_null_dependencies_and_conflicts() -> Result<()> {
        let cask: Cask = serde_json::from_value(serde_json::json!({
            "token": "example",
            "version": "1.0.0",
            "url": "https://example.com/example.zip",
            "auto_updates": null,
            "depends_on": null,
            "conflicts_with": null
        }))?;
        assert!(cask.depends_on.formula.is_empty());
        assert!(cask.conflicts_with.cask.is_empty());
        assert!(!cask.auto_updates);
        Ok(())
    }

    #[test]
    fn cask_runtime_dependencies_use_resolved_installed_formula_facts() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let formula = |name: &str, dependencies: Vec<&str>, rebuild: u64| -> Result<_> {
            Ok(super::super::resolve::ResolvedFormula {
                tap_name: "homebrew/core".to_string(),
                formula: serde_json::from_value::<super::super::api::Formula>(serde_json::json!({
                    "name": name,
                    "versions": {"stable": "4.0.0"},
                    "dependencies": dependencies,
                    "bottle": {"stable": {"rebuild": rebuild, "files": {}}}
                }))?,
                tap_raw_base: None,
                on_request: name == "root",
            })
        };
        let dependency = formula("dependency", Vec::new(), 1)?;
        let root = formula("root", vec!["dependency"], 2)?;
        for name in ["dependency", "root"] {
            let keg = super::super::pour::keg_path(name, "4.0.0");
            file::create_dir_all(&keg)?;
            file::write(
                keg.join("INSTALL_RECEIPT.json"),
                include_bytes!("testdata/ada-url-INSTALL_RECEIPT.json"),
            )?;
        }
        let mut cask = test_cask("formula-dependent", "1.0.0");
        cask.depends_on.formula = vec!["root".to_string()];
        cask.resolved_formula_dependencies = vec![dependency, root];

        let runtime = cask_runtime_dependencies(&cask)?;
        let formulae = runtime["formula"].as_array().unwrap();

        assert_eq!(formulae.len(), 2);
        assert_eq!(formulae[0]["full_name"], "dependency");
        assert_eq!(formulae[0]["declared_directly"], false);
        assert_eq!(formulae[0]["bottle_rebuild"], 1);
        assert_eq!(formulae[1]["full_name"], "root");
        assert_eq!(formulae[1]["declared_directly"], true);
        assert_eq!(formulae[1]["bottle_rebuild"], 2);
        Ok(())
    }

    #[test]
    fn cask_metadata_treats_null_auto_updates_as_false() -> Result<()> {
        let cask: Cask = serde_json::from_value(serde_json::json!({
            "token": "example",
            "version": "1.0.0",
            "url": "https://example.com/example.zip",
            "auto_updates": null
        }))?;
        assert!(!cask.auto_updates);
        Ok(())
    }

    #[test]
    fn structured_symlink_preserves_relative_source() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let appdir = tmp.path().join("Applications");
        let target = appdir.join("MeshLab2025.07.app/Contents/MacOS/MeshLab");
        file::create_dir_all(
            target
                .parent()
                .ok_or_else(|| eyre!("missing target parent"))?,
        )?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::Literal,
                path: "meshlab".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::AppDir,
                path: "MeshLab{{version}}.app/Contents/MacOS/MeshLab".to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: vec![FlightGuard::UnlessExists(FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            })],
        };

        execute_flight_steps(
            &test_cask("meshlab", "2025.07"),
            &[step],
            tmp.path(),
            &appdir,
            "postflight_steps",
        )?;

        assert_eq!(std::fs::read_link(target)?, PathBuf::from("meshlab"));
        Ok(())
    }

    #[test]
    fn based_flight_paths_reject_root_escapes() {
        let cask = test_cask("example", "1.0.0");
        let staged = Path::new("/tmp/staged");
        let appdir = Path::new("/Applications");
        for base in [
            FlightPathBase::StagedPath,
            FlightPathBase::AppDir,
            FlightPathBase::HomebrewPrefix,
        ] {
            for path in ["../outside", "/absolute/outside"] {
                let err = resolve_flight_path_with_context(
                    &cask,
                    &FlightPath {
                        base,
                        path: path.to_string(),
                    },
                    staged,
                    appdir,
                )
                .unwrap_err()
                .to_string();
                assert!(err.contains("invalid structured flight path"));
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_expands_versioned_glob() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source_dir = tmp.path().join("tool-2/bin");
        file::create_dir_all(&source_dir)?;
        file::write(source_dir.join("tool"), "binary")?;
        let target_dir = tmp.path().join("links");
        file::create_dir_all(&target_dir)?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "tool-{{version.major}}/bin/*".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target_dir.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: true,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };

        execute_flight_steps(
            &test_cask("tool", "2.3.4"),
            &[step],
            tmp.path(),
            Path::new("/Applications"),
            "postflight_steps",
        )?;

        let link = target_dir.join("tool");
        assert_eq!(
            lexically_normalized_path(&resolve_symlink_target(&link, std::fs::read_link(&link)?)),
            source_dir.join("tool")
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_glob_rollback_removes_created_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let staged = tmp.path().join("stage");
        file::create_dir_all(&staged)?;
        file::write(staged.join("one"), "one")?;
        file::write(staged.join("two"), "two")?;
        let target = tmp.path().join("links");
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "*".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: true,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };
        let mut targets = FlightTargetTransaction::default();

        execute_flight_step(
            &test_cask("example", "1.0.0"),
            &step,
            &staged,
            Path::new("/Applications"),
            &mut targets,
        )?;
        assert!(target.is_dir());
        assert_eq!(
            targets.installed_directories(),
            std::slice::from_ref(&target)
        );
        assert!(!targets.installed_targets().contains(&target));
        assert_eq!(
            targets.uninstall_targets(),
            &BTreeMap::from([(target.join("one"), false), (target.join("two"), false),])
        );

        targets.rollback()?;

        assert!(target.symlink_metadata().is_err());

        file::create_dir_all(&target)?;
        let mut upgrade_targets = FlightTargetTransaction::default();
        upgrade_targets.previous_directories.insert(target.clone());
        execute_flight_step(
            &test_cask("example", "2.0.0"),
            &step,
            &staged,
            Path::new("/Applications"),
            &mut upgrade_targets,
        )?;
        assert_eq!(upgrade_targets.installed_directories(), [target]);
        upgrade_targets.commit()?;
        Ok(())
    }

    #[test]
    fn obsolete_flight_directories_remove_only_empty_unclaimed_directories() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let empty = tmp.path().join("empty");
        let occupied = tmp.path().join("occupied");
        let current = tmp.path().join("current");
        for directory in [&empty, &occupied, &current] {
            file::create_dir_all(directory)?;
        }
        file::write(occupied.join("user-file"), "keep")?;
        let previous = BTreeSet::from([empty.clone(), occupied.clone(), current.clone()]);

        remove_obsolete_flight_directories(&previous, std::slice::from_ref(&current))?;

        assert!(empty.symlink_metadata().is_err());
        assert!(occupied.join("user-file").is_file());
        assert!(current.is_dir());
        Ok(())
    }

    #[test]
    fn structured_symlink_rejects_empty_glob() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("links");
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "missing/*".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: true,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };

        let err = execute_flight_steps(
            &test_cask("tool", "1.0.0"),
            &[step],
            tmp.path(),
            Path::new("/Applications"),
            "postflight_steps",
        )
        .unwrap_err();
        let err = format!("{err:#}");

        assert!(err.contains("did not match any paths"));
        assert!(target.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_glob_replaces_dangling_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source_dir = tmp.path().join("bin");
        file::create_dir_all(&source_dir)?;
        file::write(source_dir.join("one"), "one")?;
        file::write(source_dir.join("two"), "two")?;
        let target = tmp.path().join("links");
        file::make_symlink(Path::new("missing"), &target)?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "bin/*".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: true,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };

        execute_flight_steps(
            &test_cask("tool", "1.0.0"),
            &[step],
            tmp.path(),
            Path::new("/Applications"),
            "postflight_steps",
        )?;

        assert!(target.is_dir());
        for name in ["one", "two"] {
            let link = target.join(name);
            assert_eq!(
                lexically_normalized_path(&resolve_symlink_target(
                    &link,
                    std::fs::read_link(&link)?
                )),
                source_dir.join(name)
            );
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_replaces_directory_symlink_without_following_it() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let unrelated = tmp.path().join("unrelated");
        let target = tmp.path().join("target");
        file::write(&source, "source")?;
        file::create_dir_all(&unrelated)?;
        file::make_symlink(&unrelated, &target)?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::Literal,
                path: source.to_string_lossy().to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: true,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };

        execute_flight_steps(
            &test_cask("tool", "1.0.0"),
            &[step],
            tmp.path(),
            Path::new("/Applications"),
            "postflight_steps",
        )?;

        assert_eq!(
            lexically_normalized_path(&resolve_symlink_target(
                &target,
                std::fs::read_link(&target)?
            )),
            source
        );
        assert!(std::fs::read_dir(unrelated)?.next().is_none());
        Ok(())
    }

    #[test]
    fn forced_symlink_command_uses_replacement_flags() {
        let no_dereference = if cfg!(target_os = "macos") {
            "-h"
        } else {
            "-n"
        };
        assert_eq!(
            symlink_command_args(Path::new("source"), Path::new("target")),
            ["-s", "-f", no_dereference, "--", "source", "target"]
        );
    }

    #[test]
    #[cfg(unix)]
    fn flight_guards_match_homebrew_for_dangling_symlinks() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let dangling = tmp.path().join("dangling");
        std::os::unix::fs::symlink("missing", &dangling)?;
        let cask = test_cask("example", "1.0.0");
        let path = FlightPath {
            base: FlightPathBase::Literal,
            path: dangling.to_string_lossy().to_string(),
        };

        assert!(!flight_guard_matches(
            &cask,
            &FlightGuard::IfExists(path.clone()),
            tmp.path(),
            Path::new("/Applications"),
        )?);
        assert!(flight_guard_matches(
            &cask,
            &FlightGuard::UnlessExists(path),
            tmp.path(),
            Path::new("/Applications"),
        )?);

        let blocking_file = tmp.path().join("not-a-directory");
        file::write(&blocking_file, "blocked")?;
        let blocked = FlightPath {
            base: FlightPathBase::Literal,
            path: blocking_file.join("target").to_string_lossy().to_string(),
        };
        assert!(
            flight_guard_matches(
                &cask,
                &FlightGuard::IfExists(blocked.clone()),
                tmp.path(),
                Path::new("/Applications"),
            )
            .is_err()
        );
        assert!(
            flight_guard_matches(
                &cask,
                &FlightGuard::UnlessExists(blocked),
                tmp.path(),
                Path::new("/Applications"),
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_replaces_dangling_target_without_force() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        file::write(&source, "source")?;
        file::make_symlink(Path::new("missing"), &target)?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::Literal,
                path: source.to_string_lossy().to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: vec![FlightGuard::UnlessExists(FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            })],
        };

        execute_flight_steps(
            &test_cask("example", "1.0.0"),
            &[step],
            tmp.path(),
            Path::new("/Applications"),
            "postflight_steps",
        )?;

        assert_eq!(
            lexically_normalized_path(&resolve_symlink_target(
                &target,
                std::fs::read_link(&target)?
            )),
            source
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_replaces_previous_owned_target_without_force() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let old_source = tmp.path().join("old");
        let new_source = stage.join("new");
        let target = tmp.path().join("target");
        file::create_dir_all(&stage)?;
        file::write(&old_source, "old")?;
        file::write(&new_source, "new")?;
        file::make_symlink(&old_source, &target)?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "new".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };
        let mut targets = FlightTargetTransaction::default();
        targets.previous_symlinks.insert(target.clone());

        execute_flight_step(
            &test_cask("example", "2.0.0"),
            &step,
            &stage,
            Path::new("/Applications"),
            &mut targets,
        )?;
        assert_eq!(std::fs::read_link(&target)?, new_source);

        targets.rollback()?;
        assert_eq!(std::fs::read_link(target)?, old_source);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_rollback_removes_link_with_created_parent() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let source = stage.join("source");
        let target = tmp.path().join("external/nested/target");
        file::create_dir_all(&stage)?;
        file::create_dir_all(tmp.path().join("external"))?;
        file::write(&source, "source")?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "source".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };
        let mut targets = FlightTargetTransaction::default();

        execute_flight_step(
            &test_cask("example", "1.0.0"),
            &step,
            &stage,
            Path::new("/Applications"),
            &mut targets,
        )?;
        assert!(target.symlink_metadata()?.file_type().is_symlink());

        targets.rollback()?;
        assert!(target.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_force_refuses_to_replace_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        let link = target.join("source");
        file::write(&source, "source")?;
        file::create_dir_all(&link)?;
        file::write(link.join("keep"), "keep")?;
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::Literal,
                path: source.to_string_lossy().to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::Literal,
                path: target.to_string_lossy().to_string(),
            },
            force: true,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };

        let err = execute_flight_steps(
            &test_cask("example", "1.0.0"),
            &[step],
            tmp.path(),
            Path::new("/Applications"),
            "postflight_steps",
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("refusing to replace structured symlink directory"));
        assert_eq!(file::read_to_string(link.join("keep"))?, "keep");
        Ok(())
    }

    #[test]
    fn flight_target_transaction_restores_replaced_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        file::write(&target, "original")?;
        {
            let mut transaction = FlightTargetTransaction::default();
            transaction.protect(&target)?;
            let backup = transaction.backups[0].backup.as_ref().unwrap();
            let recovery = flight_backup_recovery_path(backup);
            assert!(recovery.is_file());
            assert_ne!(recovery.parent(), backup.parent());
            file::write(&target, "replacement")?;
        }
        assert_eq!(file::read_to_string(target)?, "original");
        Ok(())
    }

    #[test]
    fn flight_target_transaction_retries_failed_restore() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("missing/target");
        let backup = tmp.path().join("backup");
        file::write(&backup, "original")?;
        let recovery = flight_backup_recovery_path(&backup);
        let target_parent = resolved_parent(&target)?;
        let backup_parent = Some(resolved_parent(&backup)?);
        let record = FlightRecoveryRecord {
            target: target.clone(),
            backup: Some(backup.clone()),
            target_parent: target_parent.clone(),
            backup_parent: backup_parent.clone(),
            receipt_caskroom: None,
            elevate: true,
        };
        write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;
        let mut transaction = FlightTargetTransaction {
            backups: vec![ArtifactLinkBackup {
                target: target.clone(),
                backup: Some(backup.clone()),
                target_parent,
                backup_parent,
                elevate: true,
            }],
            allowed_targets: None,
            receipt_caskroom: None,
            installed: Vec::new(),
            uninstall: BTreeMap::new(),
            previous_symlinks: BTreeSet::new(),
            copied_files: BTreeSet::new(),
            previous_directories: BTreeSet::new(),
            installed_directories: Vec::new(),
            committed: false,
        };

        assert!(transaction.rollback().is_err());
        assert_eq!(transaction.backups.len(), 1);
        assert!(backup.is_file());
        assert!(recovery.is_file());

        file::create_dir_all(target.parent().unwrap())?;
        transaction.rollback()?;
        assert!(transaction.backups.is_empty());
        assert_eq!(file::read_to_string(target)?, "original");
        assert!(recovery.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn recovers_interrupted_flight_target_transaction() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        file::write(&target, "original")?;
        let mut transaction = FlightTargetTransaction::default();
        transaction.protect(&target)?;
        let backup = transaction.backups[0].backup.as_ref().unwrap();
        let recovery = flight_backup_recovery_path(backup);
        std::mem::forget(transaction);

        recover_flight_backup(&recovery)?;

        assert_eq!(file::read_to_string(&target)?, "original");
        assert!(recovery.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn recovers_interrupted_new_flight_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        let mut transaction = FlightTargetTransaction::default();
        transaction.protect(&target)?;
        let recovery = flight_absent_recovery_path(&target);
        assert!(recovery.is_file());
        file::make_symlink(Path::new("source"), &target)?;
        std::mem::forget(transaction);

        recover_flight_backup(&recovery)?;

        assert!(target.symlink_metadata().is_err());
        assert!(recovery.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn interrupted_new_flight_target_preserves_completed_receipt() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        let caskroom = tmp.path().join("Caskroom/example/1.0.0");
        file::create_dir_all(&caskroom)?;
        let mut transaction = FlightTargetTransaction::default();
        transaction.receipt_caskroom = Some(caskroom.clone());
        transaction.protect(&target)?;
        let recovery = flight_absent_recovery_path(&target);
        file::make_symlink(Path::new("source"), &target)?;
        let receipt = CaskReceipt {
            schema_version: 3,
            version: "1.0.0".to_string(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: Vec::new(),
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: Vec::new(),
            targets: vec![CaskTargetRecord {
                path: target.clone(),
                fingerprint: cask_target_fingerprint(&target)?,
                uninstall: Some(true),
            }],
            prune_safe: false,
            prune_blocker: None,
        };
        file::write(
            caskroom.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;
        std::mem::forget(transaction);

        recover_flight_backup(&recovery)?;

        assert!(target.is_symlink());
        assert!(recovery.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn interrupted_flight_recovery_preserves_recreated_target_and_backup() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        file::write(&target, "original")?;
        let mut transaction = FlightTargetTransaction::default();
        transaction.protect(&target)?;
        let backup = transaction.backups[0].backup.as_ref().unwrap().clone();
        let recovery = flight_backup_recovery_path(&backup);
        file::write(&target, "recreated")?;
        std::mem::forget(transaction);

        recover_flight_backup(&recovery)?;

        assert_eq!(file::read_to_string(&target)?, "recreated");
        assert_eq!(file::read_to_string(&backup)?, "original");
        assert!(recovery.is_file());

        let mut retry = FlightTargetTransaction::default();
        let err = retry.protect(&target).unwrap_err().to_string();
        assert!(err.contains("unresolved recovery"));
        assert_eq!(file::read_to_string(&backup)?, "original");

        file::remove_all(&target)?;
        recover_flight_backup(&recovery)?;
        assert_eq!(file::read_to_string(&target)?, "original");
        assert!(backup.symlink_metadata().is_err());
        assert!(recovery.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn invalid_flight_recovery_does_not_block_later_installs() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("missing/target");
        let backup = tmp.path().join("backup");
        file::write(&backup, "original")?;
        let recovery = flight_backup_recovery_path(&backup);
        let record = FlightRecoveryRecord {
            target,
            backup: Some(backup.clone()),
            target_parent: tmp.path().join("unexpected-target-parent"),
            backup_parent: Some(resolved_parent(&backup)?),
            receipt_caskroom: None,
            elevate: true,
        };
        write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;

        recover_flight_backup_or_warn(&recovery);

        assert!(recovery.is_file());
        assert_eq!(file::read_to_string(backup)?, "original");
        file::remove_all(recovery)?;
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn stale_flight_recovery_temp_file_does_not_block_recovery() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let recovery_root = tmp.path().join("recovery");
        let stale = recovery_root.join("stale.tmp");
        file::create_dir_all(&stale)?;
        file::write(stale.join("locked"), "content")?;
        std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o000))?;

        let target = tmp.path().join("target");
        let backup = tmp.path().join("backup");
        file::write(&backup, "original")?;
        let recovery = recovery_root.join("valid.recovery");
        let record = FlightRecoveryRecord {
            target: target.clone(),
            backup: Some(backup.clone()),
            target_parent: resolved_parent(&target)?,
            backup_parent: Some(resolved_parent(&backup)?),
            receipt_caskroom: None,
            elevate: true,
        };
        write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;

        recover_flight_backups_in(&recovery_root)?;

        assert_eq!(file::read_to_string(target)?, "original");
        assert!(recovery.symlink_metadata().is_err());
        assert!(stale.symlink_metadata().is_ok());
        std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn flight_target_rollback_rejects_swapped_parent() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let target = prefix.join("bin/example");
        file::create_dir_all(target.parent().unwrap())?;
        file::write(&target, "original")?;
        let mut transaction = FlightTargetTransaction::default();
        transaction.protect(&target)?;

        let saved_prefix = tmp.path().join("saved-prefix");
        file::rename(&prefix, &saved_prefix)?;
        let external = tmp.path().join("external");
        file::create_dir_all(external.join("bin"))?;
        let external_target = external.join("bin/example");
        file::write(&external_target, "external")?;
        file::make_symlink(&external, &prefix)?;

        assert!(transaction.rollback().is_err());
        assert_eq!(file::read_to_string(&external_target)?, "external");
        assert_eq!(transaction.backups.len(), 1);

        file::remove_file(&prefix)?;
        file::rename(&saved_prefix, &prefix)?;
        transaction.rollback()?;
        assert_eq!(file::read_to_string(&target)?, "original");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn flight_target_backup_survives_app_replacement() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let app = tmp.path().join("Example.app");
        let target = app.join("Contents/MacOS/example-link");
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(Path::new("original"), &target)?;
        let mut transaction = FlightTargetTransaction::default();

        transaction.protect(&target)?;
        let backup = transaction.backups[0].backup.as_ref().unwrap().clone();
        assert_eq!(backup.parent(), app.parent());
        file::remove_all(&app)?;
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(Path::new("replacement"), &target)?;

        transaction.rollback()?;
        assert_eq!(std::fs::read_link(target)?, PathBuf::from("original"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn receipt_flight_symlinks_exclude_standard_and_drifted_targets() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let binary = tmp.path().join("bin/example");
        let flight = tmp.path().join("share/example");
        let retained = tmp.path().join("share/retained");
        let drifted = tmp.path().join("share/drifted");
        file::create_dir_all(binary.parent().unwrap())?;
        file::create_dir_all(flight.parent().unwrap())?;
        file::make_symlink(Path::new("binary-source"), &binary)?;
        file::make_symlink(Path::new("flight-source"), &flight)?;
        file::make_symlink(Path::new("retained-source"), &retained)?;
        file::make_symlink(Path::new("original-source"), &drifted)?;
        let records = [&binary, &flight, &retained, &drifted]
            .into_iter()
            .map(|path| {
                Ok(CaskTargetRecord {
                    path: path.clone(),
                    fingerprint: cask_target_fingerprint(path)?,
                    uninstall: if path == &retained {
                        Some(false)
                    } else if path == &binary {
                        None
                    } else {
                        Some(true)
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        file::remove_file(&drifted)?;
        file::make_symlink(Path::new("changed-source"), &drifted)?;
        let receipt = CaskReceipt {
            schema_version: 3,
            version: "1.0.0".to_string(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: vec![binary],
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: Vec::new(),
            targets: records,
            prune_safe: false,
            prune_blocker: None,
        };

        assert_eq!(receipt_flight_symlink_targets(&receipt)?, vec![flight]);
        Ok(())
    }

    #[test]
    fn staged_symlink_sources_become_caskroom_owned() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let source = stage.join("AndroidNDK.app/Contents/NDK");
        file::create_dir_all(&source)?;
        file::write(source.join("ndk-build"), "binary")?;
        let temporary_caskroom = tmp.path().join("Caskroom/android-ndk/.mise-tmp");
        let target = tmp.path().join("share/android-ndk");
        let mut targets = FlightTargetTransaction::default();
        targets.protect(&target)?;
        file::create_dir_all(
            target
                .parent()
                .ok_or_else(|| eyre!("missing target parent"))?,
        )?;
        file::make_symlink(&source, &target)?;
        targets.record_installed(target.clone());

        durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

        assert_eq!(
            std::fs::read_link(&target)?,
            temporary_caskroom.join(".homebrew-staged/AndroidNDK.app/Contents/NDK")
        );
        assert!(
            temporary_caskroom
                .join(".homebrew-staged/AndroidNDK.app/Contents/NDK/ndk-build")
                .is_file()
        );
        targets.commit()?;
        Ok(())
    }

    #[test]
    fn staged_symlink_source_copies_reachable_internal_links() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let source = stage.join("pkg/bin");
        let shared = stage.join("shared/data");
        file::create_dir_all(&source)?;
        file::create_dir_all(&shared)?;
        file::write(shared.join("value"), "content")?;
        file::make_symlink(&shared, &source.join("absolute"))?;
        file::make_symlink(Path::new("../../shared/data"), &source.join("relative"))?;
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let target = tmp.path().join("share/example");
        let mut targets = FlightTargetTransaction::default();
        targets.protect(&target)?;
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&source, &target)?;
        targets.record_installed(target.clone());

        durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

        let owned_stage = temporary_caskroom.join(".homebrew-staged");
        assert_eq!(
            std::fs::read_link(owned_stage.join("pkg/bin/absolute"))?,
            owned_stage.join("shared/data")
        );
        assert_eq!(
            std::fs::read_link(owned_stage.join("pkg/bin/relative"))?,
            PathBuf::from("../../shared/data")
        );
        assert_eq!(
            file::read_to_string(owned_stage.join("pkg/bin/absolute/value"))?,
            "content"
        );
        assert_eq!(
            file::read_to_string(owned_stage.join("pkg/bin/relative/value"))?,
            "content"
        );
        targets.commit()?;
        Ok(())
    }

    #[test]
    fn staged_symlink_source_preserves_link_to_external_referent() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let external = tmp.path().join("external");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&external)?;
        file::write(external.join("value"), "content")?;
        let staged_link = stage.join("external-link");
        file::make_symlink(&external, &staged_link)?;
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let target = tmp.path().join("share/example");
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&staged_link, &target)?;
        let mut targets = FlightTargetTransaction::default();
        targets.protect(&target)?;
        file::make_symlink(&staged_link, &target)?;
        targets.record_installed(target.clone());

        durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

        let durable = temporary_caskroom.join(".homebrew-staged/external-link");
        assert_eq!(std::fs::read_link(&target)?, durable);
        assert_eq!(std::fs::read_link(&durable)?, external);
        file::remove_all(&stage)?;
        assert_eq!(file::read_to_string(target.join("value"))?, "content");
        targets.commit()?;
        Ok(())
    }

    #[test]
    fn staged_symlink_source_accepts_canonical_stage_spelling() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let real_parent = tmp.path().join("real");
        let real_stage = real_parent.join("stage");
        file::create_dir_all(&real_stage)?;
        file::write(real_stage.join("value"), "content")?;
        let alias_parent = tmp.path().join("alias");
        file::make_symlink(&real_parent, &alias_parent)?;
        let stage = alias_parent.join("stage");
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let target = tmp.path().join("share/example");
        file::create_dir_all(target.parent().unwrap())?;
        let mut targets = FlightTargetTransaction::default();
        targets.protect(&target)?;
        file::make_symlink(&real_stage, &target)?;
        targets.record_installed(target.clone());

        durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

        assert_eq!(file::read_to_string(target.join("value"))?, "content");
        targets.commit()?;
        Ok(())
    }

    #[test]
    fn staged_artifact_closure_merges_a_parent_after_its_child() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let parent = stage.join("parent");
        file::create_dir_all(&parent)?;
        file::write(parent.join("first"), "first")?;
        file::write(parent.join("second"), "second")?;
        let owned = tmp.path().join("owned");

        copy_staged_artifact_closure(&stage, &owned, &parent.join("first"))?;
        copy_staged_artifact_closure(&stage, &owned, &parent)?;

        assert_eq!(file::read_to_string(owned.join("parent/first"))?, "first");
        assert_eq!(file::read_to_string(owned.join("parent/second"))?, "second");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn staged_artifact_closure_rejects_intermediate_symlink_escape() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let outside = tmp.path().join("outside");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&outside)?;
        file::write(outside.join("secret"), "secret")?;
        file::make_symlink(&outside, &stage.join("link"))?;
        let owned = tmp.path().join("owned");

        let err = copy_staged_artifact_closure(&stage, &owned, &stage.join("link/secret"))
            .unwrap_err()
            .to_string();

        assert!(err.contains("escaped extraction root"));
        assert!(owned.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn structured_symlink_inside_stage_uses_relative_source() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        file::create_dir_all(stage.join("source"))?;
        let link = stage.join("nested/link");
        let step = FlightStep::Symlink {
            source: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "source".to_string(),
            },
            target: FlightPath {
                base: FlightPathBase::StagedPath,
                path: "nested/link".to_string(),
            },
            force: false,
            uninstall: false,
            source_glob: false,
            sudo: FlightSudo::Never,
            guards: Vec::new(),
        };

        execute_flight_steps(
            &test_cask("example", "1.0.0"),
            &[step],
            &stage,
            Path::new("/Applications"),
            "preflight_steps",
        )?;

        assert_eq!(std::fs::read_link(link)?, PathBuf::from("../source"));
        Ok(())
    }

    #[test]
    fn internal_staged_symlinks_remain_relative_and_untracked() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let source = stage.join("Example.app/Contents/Resources/data");
        let link = stage.join("Example.app/Contents/data");
        file::create_dir_all(&source)?;
        file::create_dir_all(link.parent().unwrap())?;
        file::make_symlink(Path::new("Resources/data"), &link)?;
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let mut targets = FlightTargetTransaction::default();
        targets.record_installed(link.clone());

        durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

        assert_eq!(std::fs::read_link(&link)?, PathBuf::from("Resources/data"));
        assert!(temporary_caskroom.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn temporary_caskroom_symlink_sources_follow_activation() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let final_caskroom = tmp.path().join("Caskroom/example/1.0.0");
        let source = temporary_caskroom.join("bin/example");
        file::create_dir_all(source.parent().unwrap())?;
        file::write(&source, "binary")?;
        let target = tmp.path().join("bin/example");
        file::create_dir_all(target.parent().unwrap())?;
        let mut targets = FlightTargetTransaction::default();
        targets.protect(&target)?;
        file::make_symlink(&source, &target)?;
        targets.record_installed(target.clone());
        file::rename(&temporary_caskroom, &final_caskroom)?;

        retarget_transient_symlinks(
            &temporary_caskroom,
            &final_caskroom,
            &final_caskroom,
            &targets,
        )?;

        assert_eq!(
            std::fs::read_link(target)?,
            final_caskroom.join("bin/example")
        );
        targets.commit()?;
        Ok(())
    }

    #[test]
    fn internal_temporary_caskroom_symlinks_follow_activation() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let final_caskroom = tmp.path().join("Caskroom/example/1.0.0");
        let source = temporary_caskroom.join("share/example/source");
        let target = temporary_caskroom.join("share/example/target");
        file::create_dir_all(source.parent().unwrap())?;
        file::write(&source, "content")?;
        file::make_symlink(&source, &target)?;
        file::rename(&temporary_caskroom, &final_caskroom)?;
        let installed_target = final_caskroom.join("share/example/target");

        retarget_transient_symlinks(
            &temporary_caskroom,
            &final_caskroom,
            &final_caskroom,
            &FlightTargetTransaction::default(),
        )?;

        assert_eq!(
            std::fs::read_link(installed_target)?,
            final_caskroom.join("share/example/source")
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
        let mut targets = FlightTargetTransaction::default();

        execute_flight_steps_with_completion(
            &cask,
            &steps,
            stage.path(),
            Path::new("/Applications"),
            "postflight_steps",
            &mut targets,
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

    #[cfg(target_os = "macos")]
    #[test]
    fn structured_run_expands_paths_args_and_env() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
                    base: FlightPathBase::Literal,
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
                network_access: false,
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
    fn structured_runs_share_transaction_home() {
        let first = test_cask("example", "1.2.3");
        let same_transaction = test_cask("example", "1.2.3");
        let next_version = test_cask("example", "1.2.4");

        assert_eq!(cask_step_home(&first), cask_step_home(&same_transaction));
        assert_ne!(cask_step_home(&first), cask_step_home(&next_version));
        assert!(cask_step_home(&first).starts_with(caskroom_tmp_dir(&first)));
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
                "preflight_steps": [{"steps": [{
                    "type": "set_permissions",
                    "paths": [{"base": "staged_path", "path": "Battle.net-Setup.app"}],
                    "permissions": "a+x"
                }]}]
            }),
            serde_json::json!({"app": "Battle.net.app"}),
        ];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("unsupported preflight_steps step type set_permissions"));
    }

    #[test]
    fn rejects_service_and_opaque_artifacts() {
        for artifact in [
            serde_json::json!({"service": {"run": ["example"]}}),
            serde_json::json!({"suite": ["Example Suite"]}),
        ] {
            let mut cask = test_cask("example", "1.0.0");
            cask.artifacts = vec![artifact, serde_json::json!({"app": "Example.app"})];

            let err = cask_artifacts(&cask).unwrap_err().to_string();
            assert!(err.contains("unsupported artifact type"));
        }
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
                    base: FlightPathBase::Literal,
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
    fn raw_payload_mode_follows_declared_artifact_type() {
        let mut binary = test_cask("claude", "1.0.0");
        binary.artifacts = vec![serde_json::json!({"binary": ["claude"]})];
        assert!(raw_payload_is_executable(&binary, "claude"));

        let mut pkg = test_cask("zoom", "1.0.0");
        pkg.artifacts = vec![serde_json::json!({"pkg": ["zoomusInstallerFull.pkg"]})];
        assert!(!raw_payload_is_executable(&pkg, "zoomusInstallerFull.pkg"));
    }

    #[test]
    fn suffixless_xar_uses_sole_declared_pkg_basename() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("2026.6.880.0");
        std::fs::write(&archive, b"xar!package")?;
        let mut cask = test_cask("cloudflare-warp", "2026.6.880.0");
        cask.artifacts = vec![serde_json::json!({
            "pkg": ["Cloudflare_WARP_2026.6.880.0.pkg"]
        })];

        assert_eq!(
            raw_payload_filename(&cask, &archive, "2026.6.880.0")?,
            "Cloudflare_WARP_2026.6.880.0.pkg"
        );
        Ok(())
    }

    #[test]
    fn suffixless_xar_does_not_guess_ambiguous_or_nested_pkg_names() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let archive = tmp.path().join("download");
        std::fs::write(&archive, b"xar!package")?;
        let mut ambiguous = test_cask("ambiguous", "1");
        ambiguous.artifacts = vec![
            serde_json::json!({"pkg": ["first.pkg"]}),
            serde_json::json!({"pkg": ["second.pkg"]}),
        ];
        assert_eq!(
            raw_payload_filename(&ambiguous, &archive, "download")?,
            "download"
        );

        let mut nested = test_cask("nested", "1");
        nested.artifacts = vec![serde_json::json!({"pkg": ["nested/installer.pkg"]})];
        assert_eq!(
            raw_payload_filename(&nested, &archive, "download")?,
            "download"
        );
        Ok(())
    }

    #[test]
    fn cached_effective_filename_record_round_trips_safe_basename() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let record = tmp.path().join("archive.effective-filename");
        file::write(&record, "Cloudflare_WARP.pkg")?;
        assert_eq!(
            read_effective_filename_record(&record)?.as_deref(),
            Some("Cloudflare_WARP.pkg")
        );
        Ok(())
    }

    #[test]
    fn cached_effective_filename_record_rejects_paths() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let record = tmp.path().join("archive.effective-filename");
        for invalid in ["../installer.pkg", "nested/installer.pkg", "line\nbreak"] {
            file::write(&record, invalid)?;
            assert!(read_effective_filename_record(&record).is_err());
        }
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

    #[cfg(unix)]
    #[test]
    fn artifact_lookup_resolves_through_a_flight_created_symlink() -> Result<()> {
        // gcloud-cli's last preflight step symlinks
        // `staged_path/google-cloud-sdk` at the SDK copied into the prefix, so
        // every `binary` source resolves only by traversing that link. The walk
        // cannot enter it.
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let installed = tmp.path().join("share/google-cloud-sdk");
        file::create_dir_all(&stage)?;
        file::create_dir_all(installed.join("bin"))?;
        crate::file::write(
            installed.join("bin/git-credential-gcloud.sh"),
            "credential helper",
        )?;
        std::os::unix::fs::symlink(&installed, stage.join("google-cloud-sdk"))?;

        // The artifact's real location, not the path through the link: callers
        // decide copy-vs-symlink from it, and the stage does not outlive the
        // install.
        assert_eq!(
            find_file_artifact(&stage, "google-cloud-sdk/bin/git-credential-gcloud.sh"),
            Some(file::desymlink_path(
                &installed.join("bin/git-credential-gcloud.sh")
            ))
        );
        Ok(())
    }

    #[test]
    fn artifact_lookup_rejects_sources_that_escape_the_root() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        file::create_dir_all(&stage)?;
        crate::file::write(tmp.path().join("outside"), "not ours")?;

        assert_eq!(find_file_artifact(&stage, "../outside"), None);
        assert_eq!(
            find_file_artifact(&stage, &tmp.path().join("outside").to_string_lossy()),
            None
        );
        Ok(())
    }

    #[test]
    fn relative_artifact_path_refuses_names_it_cannot_contain() {
        let root = Path::new("/stage");

        assert_eq!(
            relative_artifact_path(root, Path::new("bin/op")),
            Some(PathBuf::from("/stage/bin/op"))
        );
        assert_eq!(
            relative_artifact_path(root, Path::new("./bin/op")),
            Some(PathBuf::from("/stage/./bin/op"))
        );
        // Names that would resolve to `root` itself, which `find_app`'s
        // directory predicate would accept as the bundle.
        assert_eq!(relative_artifact_path(root, Path::new("")), None);
        assert_eq!(relative_artifact_path(root, Path::new(".")), None);
        assert_eq!(relative_artifact_path(root, Path::new("./")), None);
        // Escapes.
        assert_eq!(relative_artifact_path(root, Path::new("../op")), None);
        assert_eq!(
            relative_artifact_path(root, Path::new("bin/../../op")),
            None
        );
        assert_eq!(relative_artifact_path(root, Path::new("/etc/passwd")), None);
        // Resource-fork copies the walk skips.
        assert_eq!(
            relative_artifact_path(root, Path::new("__MACOSX/Yaak.app")),
            None
        );
        assert_eq!(
            relative_artifact_path(root, Path::new("payload/__MACOSX/op")),
            None
        );
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
    fn nested_app_source_defaults_to_bundle_basename() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let app = AppArtifact {
            source: "Kimi Installer.app/Contents/Helpers/Kimi.app".to_string(),
            target: None,
        };

        assert_eq!(app.target_name(), "Kimi.app");
        assert_eq!(
            app_target_path(app.target_name())?,
            EffectiveCaskDirs::current().appdir.join("Kimi.app")
        );
        Ok(())
    }

    #[test]
    fn internal_no_check_symbol_is_normalized() {
        assert_eq!(strip_internal_symbol(":no_check"), "no_check");
        assert_eq!(strip_internal_symbol("abc123"), "abc123");
    }

    #[test]
    fn unsigned_cask_payload_forms_fail_before_mutation() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let sentinel = tmp.path().join("unchanged");
        file::write(&sentinel, "before")?;
        let mut cask = test_cask("unsafe", "1");

        for checksum in [None, Some("no_check"), Some("abc123")] {
            cask.url = "https://example.com/unsafe.zip".to_string();
            cask.sha256 = checksum.map(str::to_string);
            assert!(validate_cask_payload_identity(&mut cask).is_err());
            assert_eq!(file::read_to_string(&sentinel)?, "before");
        }

        cask.url = "https://example.com/unsafe.git".to_string();
        cask.sha256 = Some("a".repeat(64));
        cask.url_specs.branch = Some("mutable-branch".to_string());
        let error = validate_cask_payload_identity(&mut cask)
            .unwrap_err()
            .to_string();
        assert!(error.contains("immutable repository revision"));
        assert_eq!(file::read_to_string(&sentinel)?, "before");
        Ok(())
    }

    #[test]
    fn signed_archive_digest_is_accepted() {
        let mut cask = test_cask("safe", "1");
        cask.sha256 = Some("0123456789ABCDEF".repeat(4));
        validate_cask_payload_identity(&mut cask).unwrap();
        assert_eq!(
            cask.sha256.as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
    }

    #[test]
    fn payload_source_kind_recognizes_mutable_vcs_urls() {
        for url in [
            "git://example.com/tool",
            "ssh://git@example.com/tool",
            "git+https://example.com/tool",
            "https://example.com/tool.GIT/",
            "https://example.com/tool.git?ref=v1#archive",
        ] {
            assert_eq!(cask_payload_source_kind(url), CaskPayloadSourceKind::Vcs);
        }
        assert_eq!(
            cask_payload_source_kind("https://example.com/tool.zip?source=.git"),
            CaskPayloadSourceKind::Archive
        );
    }

    #[test]
    fn pending_recovery_runs_before_unsafe_payload_rejection() {
        let mut cask = test_cask("unsafe", "1");
        cask.sha256 = Some("no_check".to_string());
        let recovered = std::cell::Cell::new(false);

        assert!(
            recover_before_payload_validation(&mut cask, |_| {
                recovered.set(true);
                Ok(())
            })
            .is_err()
        );
        assert!(recovered.get());
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

        stage_primary_container(&stage, &caskroom)?;
        stage_completion(&stage, &caskroom, &cask, &[], &completion)?;
        link_completion(&cask, &artifacts, &caskroom, &stage, &target)?;

        assert_eq!(
            std::fs::read_link(&target)?,
            caskroom.join("completions/ghostty.bash")
        );
        assert!(
            caskroom
                .join("etc/bash_completion.d/ghostty")
                .symlink_metadata()
                .is_err()
        );
        assert_eq!(crate::file::read_to_string(target)?, "complete");
        Ok(())
    }

    #[test]
    fn declared_completion_source_maps_caskroom_path_to_temp_caskroom() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

        stage_primary_container(&stage, &caskroom)?;
        stage_completion(&stage, &caskroom, &cask, &[], &completion)?;

        assert_eq!(
            crate::file::read_to_string(caskroom.join("share/completions/foo.bash"))?,
            "complete"
        );
        assert!(
            caskroom
                .join("etc/bash_completion.d/foo")
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_declared_completion_source() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(stage.join("one"))?;
        file::create_dir_all(stage.join("two"))?;
        file::create_dir_all(&caskroom)?;
        crate::file::write(stage.join("one/foo.bash"), "one")?;
        crate::file::write(stage.join("two/foo.bash"), "two")?;
        let cask = test_cask("foo", "1.0.0");
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "foo.bash".to_string(),
            target: None,
        };

        let err = stage_completion(&stage, &caskroom, &cask, &[], &completion)
            .unwrap_err()
            .to_string();

        assert!(err.contains("completion artifact 'foo.bash' is ambiguous"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn link_completion_preserves_homebrew_app_symlink() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("docker-desktop", "2.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let stage = tmp.path().join("stage");
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
        let target = tmp.path().join("etc/bash_completion.d/docker");
        let app_completion = app_target_path(app.target_name())?
            .join("Contents/Resources/etc/docker.bash-completion");
        file::create_dir_all(app_completion.parent().unwrap())?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&app_completion, "homebrew")?;
        file::make_symlink(&app_completion, &target)?;

        link_completion(&cask, &artifacts, &caskroom, &stage, &target)?;

        assert_eq!(std::fs::read_link(&target)?, app_completion);
        assert_eq!(crate::file::read_to_string(target)?, "homebrew");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn appdir_completion_staging_creates_no_duplicate_caskroom_file() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("Caskroom/ghostty/1.0.0");
        let app = AppArtifact {
            source: "Ghostty.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Ghostty.app".to_string()),
        };
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: "$APPDIR/Ghostty.app/Contents/Resources/ghostty.bash".to_string(),
            target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/ghostty".to_string()),
        };
        file::create_dir_all(&stage)?;
        let staged_source = caskroom.join("Ghostty.app/Contents/Resources/ghostty.bash");
        file::create_dir_all(staged_source.parent().unwrap())?;
        crate::file::write(staged_source, "complete")?;

        stage_completion(
            &stage,
            &caskroom,
            &test_cask("ghostty", "1.0.0"),
            &[app],
            &completion,
        )?;

        assert!(!caskroom.join("etc/bash_completion.d/ghostty").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn link_completion_rejects_other_file_in_declared_app() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("foo", "2.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        let other_caskroom = caskroom_version_dir("other", "1.0.0");
        let stage = tmp.path().join("stage");
        let relative = Path::new("etc/bash_completion.d/foo");
        let target = tmp.path().join(relative);
        file::create_dir_all(caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(other_caskroom.join("etc/bash_completion.d"))?;
        file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(caskroom.join(relative), "new")?;
        crate::file::write(other_caskroom.join(relative), "other")?;
        file::make_symlink(&other_caskroom.join(relative), &target)?;
        let completion = CompletionArtifact {
            shell: CompletionShell::Bash,
            source: relative.to_string_lossy().to_string(),
            target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
        };
        let artifacts = CaskArtifacts {
            completions: vec![completion],
            ..Default::default()
        };

        let err = link_completion(&cask, &artifacts, &caskroom, &stage, &target)
            .unwrap_err()
            .to_string();

        assert!(err.contains("is not owned by cask 'foo'"));
        assert_eq!(std::fs::read_link(&target)?, other_caskroom.join(relative));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_generated_completion_output() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

        let target = generated_completion_target_path(CompletionShell::Bash, "op")?;
        assert_eq!(
            crate::file::read_to_string(generated_completion_staging_path(&stage, &target)?)?,
            "completion|bash|bash"
        );
        assert!(
            caskroom
                .join("etc/bash_completion.d/op")
                .symlink_metadata()
                .is_err()
        );
        link_completion(
            &cask,
            &CaskArtifacts {
                generated_completions: vec![completion],
                ..Default::default()
            },
            &caskroom,
            &stage,
            &target,
        )?;
        assert_eq!(
            crate::file::read_to_string(&target)?,
            "completion|bash|bash"
        );
        assert!(!target.symlink_metadata()?.file_type().is_symlink());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn generated_completion_recovery_requires_retained_matching_output() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let target = tmp.path().join("etc/bash_completion.d/op");
        let staged = generated_completion_staging_path(&stage, &target)?;
        file::create_dir_all(target.parent().unwrap())?;
        file::create_dir_all(staged.parent().unwrap())?;
        crate::file::write(&target, "owned")?;

        assert!(!generated_completion_matches_staging(&stage, &target));
        crate::file::write(&staged, "owned")?;
        assert!(generated_completion_matches_staging(&stage, &target));
        crate::file::write(&staged, "other")?;
        assert!(!generated_completion_matches_staging(&stage, &target));
        file::remove_file(&staged)?;
        file::make_symlink(&target, &staged)?;
        assert!(!generated_completion_matches_staging(&stage, &target));
        Ok(())
    }

    #[test]
    fn generated_completion_executable_expands_appdir() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
    fn generated_completion_resolves_declared_prefix_binary_to_activated_app() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&caskroom)?;
        let app_executable = tmp
            .path()
            .join("Applications/Zed Preview.app/Contents/MacOS/cli");
        file::create_dir_all(app_executable.parent().unwrap())?;
        crate::file::write(&app_executable, "zed cli")?;
        let mut cask = test_cask("zed@preview", "1.16.0");
        cask.artifacts = vec![serde_json::json!({
            "binary": [
                "$APPDIR/Zed Preview.app/Contents/MacOS/cli",
                {"target": "zed-preview"}
            ],
            "target": "$HOMEBREW_PREFIX/bin/zed-preview"
        })];
        let completion = GeneratedCompletionArtifact {
            executable: "$HOMEBREW_PREFIX/bin/zed-preview".to_string(),
            args: vec!["--completions".to_string()],
            base_name: None,
            shell_parameter_format: None,
            shells: vec![CompletionShell::Bash],
        };
        let apps = [AppArtifact {
            source: "Zed Preview.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Zed Preview.app".to_string()),
        }];

        assert_eq!(
            find_generated_completion_executable(&stage, &caskroom, &cask, &apps, &completion,)?,
            app_executable
        );
        Ok(())
    }

    #[test]
    fn appdir_artifact_source_matches_app_case_insensitively() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

    #[cfg(unix)]
    #[test]
    fn binary_appdir_source_accepts_owned_directory_and_rejects_escape() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let app = tmp.path().join("Applications/Surge.app");
        let nested = app.join("Contents/Applications/Surge Dashboard.app");
        file::create_dir_all(&nested)?;
        let apps = [AppArtifact {
            source: "Surge.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Surge.app".to_string()),
        }];
        let source = "$APPDIR/Surge.app/Contents/Applications/Surge Dashboard.app";

        assert_eq!(
            binary_appdir_artifact_source(source, &apps)?,
            Some(nested.clone())
        );
        file::remove_all(&nested)?;
        let foreign = tmp.path().join("foreign-dashboard");
        file::create_dir_all(&foreign)?;
        file::make_symlink(&foreign, &nested)?;
        assert_eq!(binary_appdir_artifact_source(source, &apps)?, None);
        Ok(())
    }

    #[test]
    fn generated_completion_executable_prefers_staged_prefix_binary() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
    fn parses_pkg_receipt_root_and_rejects_foreign_volume() -> Result<()> {
        let root = pkg_root_from_info(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>volume</key><string>/</string>
<key>install-location</key><string>/Applications</string>
</dict></plist>"#,
        )?;
        assert_eq!(root, Path::new("/Applications"));

        let err = pkg_root_from_info(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>volume</key><string>/Volumes/Foreign</string>
<key>install-location</key><string>/</string>
</dict></plist>"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("volume is unsupported"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn pkg_bom_plan_classifies_files_links_and_deepest_directories() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("root");
        file::create_dir_all(root.join("App/Contents"))?;
        file::write(root.join("App/Contents/file"), "owned")?;
        file::make_symlink(Path::new("missing"), &root.join("App/Contents/link"))?;

        let plan = pkg_removal_plan_from_bom(
            "com.example.pkg",
            root.clone(),
            "App\nApp/Contents\nApp/Contents/file\nApp/Contents/link\n",
        )?;

        assert_eq!(plan.files, [root.join("App/Contents/file")]);
        assert_eq!(plan.specials, [root.join("App/Contents/link")]);
        assert_eq!(
            plan.directories,
            [root.join("App/Contents"), root.join("App")]
        );
        assert_eq!(plan.all_paths.len(), 4);
        assert!(
            pkg_removal_plan_from_bom("com.example.pkg", root, "../escape\n")
                .unwrap_err()
                .to_string()
                .contains("not relative and normalized")
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn pkg_bom_execution_tolerates_owned_script_removal_but_rejects_type_changes() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let file_path = tmp.path().join("file");
        let link_path = tmp.path().join("link");
        file::write(&file_path, "owned")?;
        file::make_symlink(Path::new("missing"), &link_path)?;

        std::fs::remove_file(&file_path)?;
        std::fs::remove_file(&link_path)?;
        assert!(
            live_pkg_removal_paths(
                "example",
                std::slice::from_ref(&file_path),
                PkgRemovalPathKind::File,
            )?
            .is_empty()
        );
        assert!(
            live_pkg_removal_paths(
                "example",
                std::slice::from_ref(&link_path),
                PkgRemovalPathKind::Special,
            )?
            .is_empty()
        );

        file::create_dir_all(&file_path)?;
        assert!(
            live_pkg_removal_paths("example", &[file_path], PkgRemovalPathKind::File,)
                .unwrap_err()
                .to_string()
                .contains("changed type after preflight")
        );
        Ok(())
    }

    #[test]
    fn protected_system_descendant_delete_is_validated_but_root_is_rejected() -> Result<()> {
        validate_cask_delete_pattern(
            "bartender",
            Path::new("/System/Library/ScriptingAdditions/BartenderSystemHelper.osax"),
        )?;
        assert!(validate_cask_delete_pattern("example", Path::new("/System")).is_err());
        Ok(())
    }

    #[test]
    fn parses_and_installs_generic_artifact() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let source = stage.join("libcblite-4.1.0/include/cbl");
        file::create_dir_all(&source)?;
        file::write(source.join("CouchbaseLite.h"), "header")?;
        let value = serde_json::json!({
            "artifact": [
                "libcblite-4.1.0/include/cbl",
                {"target": "$HOMEBREW_PREFIX/include/cbl"}
            ],
            "target": "$HOMEBREW_PREFIX/include/cbl"
        });
        let artifact = parse_generic_artifact(&value)?.ok_or_else(|| eyre!("missing artifact"))?;
        assert_eq!(
            artifact,
            GenericArtifact {
                source: "libcblite-4.1.0/include/cbl".to_string(),
                target: "$HOMEBREW_PREFIX/include/cbl".to_string(),
            }
        );

        let mut targets = FlightTargetTransaction::default();
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        install_generic_artifact(&stage, &temporary_caskroom, &artifact, &mut targets)?;
        assert_eq!(
            file::read_to_string(tmp.path().join("include/cbl/CouchbaseLite.h"))?,
            "header"
        );
        assert_eq!(
            file::read_to_string(
                temporary_caskroom.join("libcblite-4.1.0/include/cbl/CouchbaseLite.h")
            )?,
            "header"
        );
        assert_eq!(
            std::fs::read_link(temporary_caskroom.join("libcblite-4.1.0/include/cbl"))?,
            tmp.path().join("include/cbl")
        );
        assert_eq!(
            targets.installed_targets(),
            [tmp.path().join("include/cbl")]
        );
        assert_eq!(targets.backups.len(), 1);
        assert!(!targets.backups[0].elevate);
        targets.commit()?;
        Ok(())
    }

    #[test]
    fn native_generic_artifact_round_trip_uses_homebrew_metadata() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("generic-native", "1.0.0");
        cask.artifacts = vec![serde_json::json!({
            "artifact": [
                "payload/include/example",
                {"target": "$HOMEBREW_PREFIX/include/example"}
            ],
            "target": "$HOMEBREW_PREFIX/include/example"
        })];
        let artifacts = cask_artifacts(&cask)?;
        let target = tmp.path().join("include/example");
        file::create_dir_all(&target)?;
        file::write(target.join("example.h"), "header")?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        let backlink = version_dir.join("payload/include/example");
        file::create_dir_all(backlink.parent().unwrap())?;
        file::make_symlink(&target, &backlink)?;

        validate_installed_cask_topology(&cask, &artifacts, &version_dir)?;
        write_homebrew_metadata(&version_dir, &cask, &serde_json::Map::new(), false)?;
        let native = receipt::read_cask_receipt(&caskroom_token_dir(&cask.token))?;
        let targets = homebrew_receipt_targets(&cask.token, &native)?;

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, target);
        assert!(!version_dir.join(".mise-cask.toml").exists());
        assert_eq!(
            installed_cask_state(&cask, &artifacts)?,
            InstalledCaskState::Installed(cask.version)
        );
        Ok(())
    }

    #[test]
    fn flight_transaction_rejects_unpreflighted_target_before_mutation() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let allowed = tmp.path().join("allowed");
        let foreign = tmp.path().join("foreign");
        file::write(&foreign, "operator data")?;
        let mut transaction = FlightTargetTransaction::default();
        transaction.allowed_targets = Some(BTreeSet::from([allowed]));

        let err = transaction.protect_unprivileged(&foreign).unwrap_err();

        assert!(err.to_string().contains("unpreflighted lifecycle target"));
        assert_eq!(file::read_to_string(&foreign)?, "operator data");
        assert!(transaction.backups.is_empty());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn generic_artifact_rejects_extraction_source_symlink_escape() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let outside = tmp.path().join("outside");
        file::create_dir_all(&stage)?;
        file::create_dir_all(&outside)?;
        file::write(outside.join("secret"), "external")?;
        file::make_symlink(&outside, &stage.join("payload"))?;
        let artifact = GenericArtifact {
            source: "payload".to_string(),
            target: "$HOMEBREW_PREFIX/share/example".to_string(),
        };
        let mut targets = FlightTargetTransaction::default();

        let err = install_generic_artifact(
            &stage,
            &tmp.path().join("Caskroom/example/.mise-tmp"),
            &artifact,
            &mut targets,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("outside the extraction root"));
        assert!(tmp.path().join("share/example").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn generic_artifact_rejects_caskroom_source_symlink_escape() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let source = stage.join("payload/include/example");
        file::create_dir_all(&source)?;
        file::write(source.join("example.h"), "header")?;
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        let outside = tmp.path().join("outside");
        file::create_dir_all(&temporary_caskroom)?;
        file::create_dir_all(&outside)?;
        file::make_symlink(&outside, &temporary_caskroom.join("payload"))?;
        let artifact = GenericArtifact {
            source: "payload/include/example".to_string(),
            target: "$HOMEBREW_PREFIX/include/example".to_string(),
        };
        let mut targets = FlightTargetTransaction::default();

        let err = install_generic_artifact(&stage, &temporary_caskroom, &artifact, &mut targets)
            .unwrap_err()
            .to_string();

        assert!(err.contains("outside the caskroom"));
        assert!(
            tmp.path()
                .join("include/example")
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn rejects_generic_artifact_target_through_external_symlink() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let external = tmp.path().join("external");
        file::create_dir_all(&prefix)?;
        file::create_dir_all(&external)?;
        std::os::unix::fs::symlink(&external, prefix.join("lib"))?;
        let _guard = BrewPrefixGuard::set(&prefix);

        let err = generic_artifact_target_path("$HOMEBREW_PREFIX/lib/example")
            .unwrap_err()
            .to_string();
        assert!(err.contains("must stay below"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn generic_copy_revalidates_target_after_symlink_swap() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let external = tmp.path().join("external");
        let library = prefix.join("lib");
        file::create_dir_all(&library)?;
        file::create_dir_all(&external)?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let target = generic_artifact_target_path("$HOMEBREW_PREFIX/lib/example")?;

        std::fs::remove_dir(&library)?;
        std::os::unix::fs::symlink(&external, &library)?;

        let err = validate_generic_copy_target(&target)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing generic artifact copy outside Homebrew prefix"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn trusted_operation_parent_stays_bound_after_ancestor_swap() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let library = prefix.join("lib");
        file::create_dir_all(&library)?;
        let external = tmp.path().join("external");
        file::create_dir_all(external.join("lib"))?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let target = library.join("example");
        let parent = open_trusted_operation_parent(&target, true, false)?;
        let source = tmp.path().join("source");
        file::create_dir_all(&source)?;
        file::write(source.join("payload"), "installed")?;

        let saved_prefix = tmp.path().join("saved-homebrew");
        file::rename(&prefix, &saved_prefix)?;
        file::make_symlink(&external, &prefix)?;
        copy_cask_artifact_at(&source, &parent.fd, std::ffi::OsStr::new("example"))?;

        assert_eq!(
            file::read_to_string(saved_prefix.join("lib/example/payload"))?,
            "installed"
        );
        assert!(external.join("lib/example").symlink_metadata().is_err());
        remove_all_at(&parent.fd, std::ffi::OsStr::new("example"))?;
        assert!(saved_prefix.join("lib/example").symlink_metadata().is_err());

        file::remove_file(&prefix)?;
        file::rename(&saved_prefix, &prefix)?;
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn trusted_operation_parent_accepts_symlinked_prefix() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let real_prefix = tmp.path().join("real-homebrew");
        file::create_dir_all(real_prefix.join("lib"))?;
        let configured_prefix = tmp.path().join("homebrew");
        file::make_symlink(&real_prefix, &configured_prefix)?;
        let _guard = BrewPrefixGuard::set(&configured_prefix);
        let target = configured_prefix.join("lib/example");

        let parent = open_trusted_operation_parent(&target, true, false)?;
        assert_eq!(
            parent.stable_path()?,
            std::fs::canonicalize(real_prefix.join("lib"))?
        );
        file::write(parent.path()?.join("example"), "installed")?;

        assert_eq!(
            file::read_to_string(real_prefix.join("lib/example"))?,
            "installed"
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn trusted_operation_parent_creates_missing_directories() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        file::create_dir_all(&prefix)?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let target = prefix.join("share/example/include/example.h");

        let parent = open_trusted_operation_parent(&target, true, true)?;

        file::write(parent.path()?.join("example.h"), "header")?;
        assert_eq!(file::read_to_string(&target)?, "header");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn sudo_invoking_ids_are_trusted_only_for_effective_root() {
        assert_eq!(sudo_invoking_id_from(0, Some("501")), Some(501));
        assert_eq!(sudo_invoking_id_from(1000, Some("501")), None);
        assert_eq!(sudo_invoking_id_from(0, Some("0")), None);
        assert_eq!(sudo_invoking_id_from(0, Some("invalid")), None);
    }

    #[test]
    fn permission_detection_follows_wrapped_error_sources() {
        let err = eyre::Report::from(nix::errno::Errno::EACCES)
            .wrap_err("cannot create operation directory");

        assert!(is_permission_denied(&err));
    }

    #[test]
    #[cfg(unix)]
    fn elevated_generic_target_allows_only_group_writable_prefix() {
        let prefix = Path::new("/usr/local");

        assert!(strict_elevated_directory_is_trusted(
            prefix, prefix, 0, 0o775
        ));
        assert!(!strict_elevated_directory_is_trusted(
            Path::new("/usr/local/include"),
            prefix,
            0,
            0o775,
        ));
        assert!(!strict_elevated_directory_is_trusted(
            prefix, prefix, 0, 0o777
        ));
        assert!(!strict_elevated_directory_is_trusted(
            prefix, prefix, 501, 0o755
        ));
    }

    #[test]
    #[cfg(unix)]
    fn unprivileged_generic_rollback_restores_backup_when_target_is_absent() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let target = prefix.join("include/example.h");
        file::create_dir_all(target.parent().unwrap())?;
        file::write(&target, "original")?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let mut transaction = FlightTargetTransaction::default();

        transaction.protect_generic(&target)?;
        assert!(target.symlink_metadata().is_err());
        transaction.rollback()?;

        assert_eq!(file::read_to_string(&target)?, "original");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn trusted_generic_rename_rejects_swapped_prefix() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let library = prefix.join("lib");
        file::create_dir_all(&library)?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let target = library.join("example");
        let backup = library.join("example.backup");
        let expected_parent = resolved_parent(&target)?;
        file::write(&backup, "original")?;

        let saved_prefix = tmp.path().join("saved-homebrew");
        file::rename(&prefix, &saved_prefix)?;
        let external = tmp.path().join("external");
        file::create_dir_all(external.join("lib"))?;
        file::write(external.join("lib/example"), "external")?;
        file::write(external.join("lib/example.backup"), "attacker")?;
        file::make_symlink(&external, &prefix)?;

        let err = rename_trusted_generic_target(&backup, &target, &expected_parent)
            .unwrap_err()
            .to_string();

        assert!(err.contains("changed generic artifact parent"));
        assert_eq!(
            file::read_to_string(external.join("lib/example"))?,
            "external"
        );
        assert_eq!(
            file::read_to_string(external.join("lib/example.backup"))?,
            "attacker"
        );
        file::remove_file(&prefix)?;
        file::rename(&saved_prefix, &prefix)?;
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn private_staging_cleanup_rejects_replaced_directory() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let library = prefix.join("lib");
        file::create_dir_all(&library)?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let parent = open_trusted_operation_parent(&library.join("target"), true, false)?;
        let staging_name = std::ffi::OsStr::new(".mise-copy-test");
        let staging_path = library.join(staging_name);
        file::create_dir_all(&staging_path)?;
        std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o700))?;
        let staging = TrustedOperationParent {
            fd: nix::fcntl::openat(
                &parent.fd,
                staging_name,
                nix::fcntl::OFlag::O_RDONLY
                    | nix::fcntl::OFlag::O_DIRECTORY
                    | nix::fcntl::OFlag::O_NOFOLLOW,
                nix::sys::stat::Mode::empty(),
            )?,
        };
        let saved = library.join("saved-staging");
        file::rename(&staging_path, &saved)?;
        file::create_dir_all(&staging_path)?;

        let err = remove_private_staging_dir(&parent, &staging, staging_name)
            .unwrap_err()
            .to_string();

        assert!(err.contains("was replaced"));
        assert!(staging_path.is_dir());
        assert!(saved.is_dir());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn obsolete_generic_cleanup_skips_mutable_parent_directories() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let obsolete = tmp.path().join("lib/obsolete");
        let modified = tmp.path().join("lib/modified");
        file::create_dir_all(obsolete.parent().unwrap())?;
        std::fs::set_permissions(
            obsolete.parent().unwrap(),
            std::fs::Permissions::from_mode(0o777),
        )?;
        file::write(&obsolete, "owned")?;
        file::write(&modified, "owned")?;
        let records = vec![
            CaskTargetRecord {
                path: obsolete.clone(),
                fingerprint: cask_target_fingerprint(&obsolete)?,
                uninstall: None,
            },
            CaskTargetRecord {
                path: modified.clone(),
                fingerprint: cask_target_fingerprint(&modified)?,
                uninstall: None,
            },
        ];
        file::write(&modified, "user change")?;

        remove_obsolete_generic_artifacts(&records, &[])?;

        assert_eq!(file::read_to_string(obsolete)?, "owned");
        assert_eq!(file::read_to_string(modified)?, "user change");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn obsolete_generic_cleanup_allows_owner_group_writable_prefix() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let library = prefix.join("lib");
        file::create_dir_all(&library)?;
        std::fs::set_permissions(&prefix, std::fs::Permissions::from_mode(0o775))?;
        std::fs::set_permissions(&library, std::fs::Permissions::from_mode(0o775))?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let obsolete = library.join("obsolete");
        file::write(&obsolete, "owned")?;
        let records = vec![CaskTargetRecord {
            path: obsolete.clone(),
            fingerprint: cask_target_fingerprint(&obsolete)?,
            uninstall: None,
        }];

        remove_obsolete_generic_artifacts(&records, &[])?;

        assert!(obsolete.symlink_metadata().is_err());
        Ok(())
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
    fn pkg_cask_availability_matches_host_support() -> Result<()> {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({"pkg": ["Example.pkg"]}),
            serde_json::json!({"uninstall": [{"pkgutil": "com.example.pkg"}]}),
        ];
        let artifacts = cask_artifacts(&cask)?;

        #[cfg(target_os = "macos")]
        {
            validate_platform_support(&cask, &artifacts)?;
            assert!(unsupported_package_state(&cask, &artifacts).is_none());
        }
        #[cfg(not(target_os = "macos"))]
        {
            let error = validate_platform_support(&cask, &artifacts)
                .unwrap_err()
                .to_string();
            assert!(error.contains("only available on macOS"));
            assert!(matches!(
                unsupported_package_state(&cask, &artifacts),
                Some(PackageState::Unsupported { reason })
                    if reason.contains("only available on macOS")
            ));
        }
        Ok(())
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
    fn parses_ghostty_as_app_manpage_and_completion_mechanisms() -> Result<()> {
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
        assert_eq!(
            artifacts.manpages,
            vec![ManpageArtifact {
                source: "ghostty.1".to_string(),
                section: "1".to_string(),
            }]
        );
        assert_eq!(artifacts.completions.len(), 3);
        assert_eq!(artifacts.fonts.len(), 0);
        Ok(())
    }

    #[test]
    fn parses_homebrew_expanded_ghostty_receipt_sources() -> Result<()> {
        let appdir = EffectiveCaskDirs::current().appdir;
        let mut receipt: receipt::CaskReceipt =
            serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        receipt.source.version = "1.2.0".to_string();
        receipt.uninstall_artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Ghostty.app"]},
            {"manpage": [appdir.join("Ghostty.app/Contents/Resources/man/man1/ghostty.1")]},
            {"bash_completion": [appdir.join("Ghostty.app/Contents/Resources/bash-completion/completions/ghostty.bash")]},
            {"fish_completion": [appdir.join("Ghostty.app/Contents/Resources/fish/vendor_completions.d/ghostty.fish")]},
            {"zsh_completion": [appdir.join("Ghostty.app/Contents/Resources/zsh/site-functions/_ghostty")]}
        ]))?;

        let cask = cask_from_homebrew_receipt("ghostty", &receipt);
        let artifacts = parse_cask_artifacts(&cask, false)?;

        assert_eq!(artifacts.apps.len(), 1);
        assert_eq!(artifacts.manpages.len(), 1);
        assert_eq!(artifacts.completions.len(), 3);
        assert!(artifacts.manpages[0].source.starts_with("$APPDIR/"));
        assert!(
            artifacts
                .completions
                .iter()
                .all(|completion| completion.source.starts_with("$APPDIR/"))
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_or_escaping_manpage_sources() {
        for source in ["ghostty.txt", "../ghostty.1", "/tmp/ghostty.1", "ghostty.9"] {
            let mut cask = test_cask("ghostty", "1.2.0");
            cask.artifacts = vec![serde_json::json!({"manpage": [source]})];
            assert!(cask_artifacts(&cask).is_err(), "accepted {source}");
        }
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
        let expected = std::env::var_os("HOMEBREW_XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::dirs::HOME.join(".local/share"))
            .join("fonts");
        assert_eq!(font_dir(), expected);
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
    fn linux_uses_homebrew_appdir_for_direct_app_casks() -> Result<()> {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![serde_json::json!({"app": "Example.app"})];
        let artifacts = cask_artifacts(&cask)?;

        validate_platform_support(&cask, &artifacts)?;
        assert_eq!(
            app_target_path("Example.app")?,
            crate::dirs::HOME.join(".config/apps/Example.app")
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_native_config_contains_only_homebrew_linux_defaults() -> Result<()> {
        let config = native_cask_config()?;
        let defaults = config.default.as_object().unwrap();
        assert_eq!(
            defaults.get("appdir"),
            Some(&serde_json::json!(crate::dirs::HOME.join(".config/apps")))
        );
        assert_eq!(
            defaults.get("vst_plugindir"),
            Some(&serde_json::json!(crate::dirs::HOME.join(".vst")))
        );
        assert!(!defaults.contains_key("keyboard_layoutdir"));
        assert!(
            !config
                .to_json_bytes()?
                .windows(8)
                .any(|bytes| bytes == b"/Library")
        );
        Ok(())
    }

    #[test]
    fn stages_complete_container_and_preserves_nested_moved_sources() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path().join("stage");
        let caskroom = tmp.path().join("caskroom");
        let font = FontArtifact {
            source: "fonts/ttf/Example.ttf".to_string(),
            target: None,
        };
        file::create_dir_all(stage.join("fonts/ttf"))?;
        file::create_dir_all(stage.join("fonts/webfonts"))?;
        file::create_dir_all(stage.join("__MACOSX/fonts/ttf"))?;
        crate::file::write(stage.join("fonts/ttf/Example.ttf"), "font")?;
        crate::file::write(stage.join("fonts/webfonts/Example.woff2"), "webfont")?;
        crate::file::write(stage.join("__MACOSX/fonts/ttf/._Example.ttf"), "metadata")?;
        crate::file::write(stage.join("LICENSE"), "license")?;
        file::create_dir_all(&caskroom)?;

        stage_primary_container(&stage, &caskroom)?;
        stage_font(&stage, &caskroom, &font)?;

        assert_eq!(
            crate::file::read_to_string(caskroom_font_path(&caskroom, &font)?)?,
            "font"
        );
        assert_eq!(
            crate::file::read_to_string(caskroom.join("fonts/webfonts/Example.woff2"))?,
            "webfont"
        );
        assert_eq!(
            crate::file::read_to_string(caskroom.join("LICENSE"))?,
            "license"
        );
        assert!(!caskroom.join("__MACOSX").exists());
        assert!(!caskroom.join("Example.ttf").exists());
        Ok(())
    }

    #[test]
    fn discards_dmg_presentation_entries_before_staging() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let stage = tmp.path();
        file::create_dir_all(stage.join(".background"))?;
        crate::file::write(stage.join(".background/image.png"), "background")?;
        crate::file::write(stage.join(".DS_Store"), "metadata")?;
        file::create_dir_all(stage.join("Example.app"))?;
        file::make_symlink(Path::new("/Applications"), &stage.join("Applications"))?;
        file::make_symlink(
            Path::new("Versions/Current"),
            &stage.join("framework-current"),
        )?;

        discard_dmg_presentation_entries(stage)?;

        assert!(!stage.join(".background").exists());
        assert!(!stage.join(".DS_Store").exists());
        assert!(stage.join("Applications").symlink_metadata().is_err());
        assert!(stage.join("Example.app").is_dir());
        assert_eq!(
            std::fs::read_link(stage.join("framework-current"))?,
            Path::new("Versions/Current")
        );
        Ok(())
    }

    #[test]
    fn caskroom_artifact_sources_cannot_escape_staging() {
        let caskroom = Path::new("/tmp/caskroom");
        assert!(caskroom_artifact_path(caskroom, "../escape", "font").is_err());
        #[cfg(unix)]
        assert!(caskroom_artifact_path(caskroom, "/escape", "app").is_err());
    }

    #[test]
    fn rejects_opaque_uninstall_hooks_before_activation() -> Result<()> {
        for kind in ["uninstall_preflight", "uninstall_postflight"] {
            let mut cask = test_cask("unsupported-hook", "1.0.0");
            cask.artifacts = vec![
                serde_json::json!({"app": "Example.app"}),
                serde_json::json!({(kind): []}),
            ];
            let artifacts = cask_artifacts(&cask)?;
            assert!(
                validate_platform_support(&cask, &artifacts)
                    .unwrap_err()
                    .to_string()
                    .contains("cannot be replayed")
            );
        }
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let appdir = EffectiveCaskDirs::current().appdir;
        assert_eq!(
            binary_target_path("$APPDIR/Surge Dashboard.app", &appdir)?,
            appdir.join("Surge Dashboard.app")
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
                binary_target_path(target, &appdir).is_err(),
                "accepted {target}"
            );
        }
        Ok(())
    }

    #[test]
    fn installed_cask_version_uses_only_recorded_legacy_targets() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: vec![app_target_path(app.target_name())?],
            binaries: vec![],
            fonts: vec![],
            manpages: vec![],
            completions: vec![],
            flight_directories: vec![],
            generic: vec![],
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
            None
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_rejects_unknown_receipt_schema() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("future", "1.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            schema_version: 4,
            version: cask.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: Vec::new(),
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        file::create_dir_all(&version_dir)?;
        file::make_symlink(&target, &version_dir.join("Example.app"))?;
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");

        let configured = test_cask("configured", "1.0.0");
        let configured_dir = caskroom_version_dir(&configured.token, &configured.version);
        let configured_target = tmp.path().join("Applications/Configured.app");
        file::create_dir_all(&configured_target)?;
        file::create_dir_all(configured_dir.join("Configured.app"))?;
        write_receipt_with_flight_targets(
            &configured_dir,
            &configured,
            &CaskArtifacts {
                apps: vec![AppArtifact {
                    source: "Configured.app".to_string(),
                    target: Some("$HOMEBREW_PREFIX/Applications/Configured.app".to_string()),
                }],
                ..Default::default()
            },
            &[],
            &BTreeMap::new(),
            &[],
            &BTreeSet::new(),
        )?;

        let drifted = test_cask("drifted", "1.0.0");
        let drifted_dir = caskroom_version_dir(&drifted.token, &drifted.version);
        let drifted_target = tmp.path().join("Applications/Drifted.app");
        file::create_dir_all(&drifted_target)?;
        file::create_dir_all(drifted_dir.join("Drifted.app"))?;
        write_receipt_with_flight_targets(
            &drifted_dir,
            &drifted,
            &CaskArtifacts {
                apps: vec![AppArtifact {
                    source: "Drifted.app".to_string(),
                    target: Some("$HOMEBREW_PREFIX/Applications/Drifted.app".to_string()),
                }],
                ..Default::default()
            },
            &[],
            &BTreeMap::new(),
            &[],
            &BTreeSet::new(),
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
                auto_updates: false,
                metadata_only_apps: Vec::new(),
                apps: Vec::new(),
                binaries: Vec::new(),
                fonts: Vec::new(),
                manpages: Vec::new(),
                completions: Vec::new(),
                flight_directories: Vec::new(),
                generic: Vec::new(),
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
    fn cask_prune_indexes_configured_homebrew_target_claims() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let shared_app = EffectiveCaskDirs::current().appdir.join("Shared.app");
        file::create_dir_all(&shared_app)?;
        for token in ["remove", "keep"] {
            write_homebrew_cask_receipt(token, "1.2.3", |receipt| {
                receipt["uninstall_artifacts"] = if token == "keep" {
                    serde_json::json!([
                        {"app": ["Shared.app"]},
                        {"uninstall": [{"trash": "~/Library/Keep"}]}
                    ])
                } else {
                    serde_json::json!([{"app": ["Shared.app"]}])
                };
            });
        }

        let plan = cask_prune_plan_from_tokens(&BTreeSet::from(["keep".to_string()]), &state_dir)?;

        assert!(plan.remove.is_empty());
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "remove" && skip.reason.contains("also claimed by another cask")
        }));
        assert!(shared_app.exists());
        Ok(())
    }

    #[test]
    fn cask_prune_rechecks_shared_targets_before_removal() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let target = write_test_app_receipt(&test_cask("planned", "1.0.0"), "Shared.app")?;
        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);

        write_test_app_receipt(&test_cask("late-claim", "1.0.0"), "Shared.app")?;

        assert!(apply_cask_prune_plan_in(&plan, false, &state_dir).is_err());
        assert!(target.exists());
        assert!(caskroom_token_dir("planned").exists());
        Ok(())
    }

    #[test]
    fn cask_prune_rechecks_homebrew_ownership_before_removal() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let target = write_test_app_receipt(&test_cask("claimed", "1.0.0"), "Claimed.app")?;
        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);

        file::create_dir_all(caskroom_token_dir("claimed").join(".metadata"))?;

        assert!(apply_cask_prune_plan_in(&plan, false, &state_dir).is_err());
        assert!(target.exists());
        assert!(caskroom_token_dir("claimed").exists());
        Ok(())
    }

    #[test]
    fn cask_prune_removes_homebrew_binary_and_metadata() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        write_homebrew_cask_receipt("codex", "1.2.3", |_| {});
        let target = tmp.path().join("bin/codex");
        let source = caskroom_version_dir("codex", "1.2.3").join("bin/codex");
        file::create_dir_all(source.parent().unwrap())?;
        file::write(&source, "codex")?;
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&source, &target)?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);
        assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 1);
        assert!(!target.exists());
        assert!(!caskroom_token_dir("codex").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cask_prune_removes_homebrew_appdir_binary_and_metadata() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let app = EffectiveCaskDirs::current().appdir.join("CodexBar.app");
        let executable = app.join("Contents/Helpers/CodexBarCLI");
        let nested_app = app.join("Contents/Applications/Codex Dashboard.app");
        let nested_target = EffectiveCaskDirs::current()
            .appdir
            .join("Codex Dashboard.app");
        write_homebrew_cask_receipt("codexbar", "1.2.3", |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([
                {"app": ["CodexBar.app"]},
                {"binary": [executable.to_string_lossy(), {"target": "codexbar"}]},
                {"binary": [nested_app.to_string_lossy(), {"target": nested_target.to_string_lossy()}]}
            ]);
        });
        let version_dir = caskroom_version_dir("codexbar", "1.2.3");
        file::create_dir_all(executable.parent().unwrap())?;
        file::write(&executable, "codexbar")?;
        file::create_dir_all(&nested_app)?;
        file::write(nested_app.join("payload"), "dashboard")?;
        file::make_symlink(&app, &version_dir.join("CodexBar.app"))?;
        let target = prefix::prefix().join("bin/codexbar");
        file::create_dir_all(target.parent().unwrap())?;
        file::make_symlink(&executable, &target)?;
        file::make_symlink(&nested_app, &nested_target)?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);
        let foreign = tmp.path().join("foreign-codexbar");
        file::write(&foreign, "foreign")?;
        file::remove_file(&target)?;
        file::make_symlink(&foreign, &target)?;
        assert!(apply_cask_prune_plan_in(&plan, false, &state_dir).is_err());
        assert!(app.exists());
        assert!(caskroom_token_dir("codexbar").exists());
        file::remove_file(&target)?;
        file::make_symlink(&executable, &target)?;

        assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 1);
        assert!(!target.exists());
        assert!(!nested_target.exists());
        assert!(!app.exists());
        assert!(!caskroom_token_dir("codexbar").exists());
        Ok(())
    }

    #[test]
    fn cask_prune_skips_homebrew_metadata_with_pending_transaction() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        write_homebrew_cask_receipt("codex", "1.2.3", |_| {});
        let journal_dir = state_dir.join("brew-cask/codex");
        file::create_dir_all(&journal_dir)?;
        file::write(journal_dir.join("1.2.3.json"), "{}")?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;

        assert!(plan.remove.is_empty());
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "codex"
                && skip
                    .reason
                    .contains("incomplete cask transaction is pending")
        }));
        Ok(())
    }

    #[test]
    fn configured_homebrew_cask_with_unsupported_teardown_does_not_block_prune_index() -> Result<()>
    {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        write_test_app_receipt(&test_cask("remove-me", "1.0.0"), "Remove Me.app")?;
        write_homebrew_cask_receipt("gcloud-cli", "580.0.0", |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([
                {"uninstall": [{"trash": "$HOMEBREW_PREFIX/Caskroom/gcloud-cli/latest"}]}
            ]);
        });

        let keep = BTreeSet::from(["gcloud-cli".to_string()]);
        let plan = cask_prune_plan_from_tokens(&keep, &state_dir)?;

        assert_eq!(
            plan.remove
                .iter()
                .map(|candidate| candidate.token.as_str())
                .collect::<Vec<_>>(),
            ["remove-me"]
        );
        assert!(plan.skipped.is_empty());
        Ok(())
    }

    #[test]
    fn cask_prune_revalidates_late_homebrew_receipt_claims() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let target =
            write_test_app_receipt(&test_cask("remove-me", "1.0.0"), "Shared Application.app")?;
        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);

        write_homebrew_cask_receipt("late-owner", "2.0.0", |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([
                {
                    "app": ["Shared Application.app"],
                    "target": target
                }
            ]);
        });

        let err = apply_cask_prune_plan_in(&plan, false, &state_dir)
            .unwrap_err()
            .to_string();
        assert!(err.contains("claimed by another cask"));
        assert!(target.exists());
        assert!(caskroom_token_dir("remove-me").exists());
        Ok(())
    }

    #[test]
    fn homebrew_uninstall_rejects_malformed_signal_before_removal_and_ignores_zap() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        write_homebrew_cask_receipt("codex", "1.2.3", |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([
                {"binary": ["bin/codex"]},
                {"uninstall": [{"signal": "codex"}]},
                {"zap": [{"delete": "~/.codex"}]}
            ]);
        });
        let target = tmp.path().join("bin/codex");
        file::create_dir_all(target.parent().unwrap())?;
        file::write(&target, "codex")?;
        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert!(plan.remove.is_empty());
        assert!(
            plan.skipped[0]
                .reason
                .contains("recorded uninstall signal must be an array")
        );
        assert!(target.exists());

        write_homebrew_cask_receipt("codex", "1.2.3", |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([
                {"binary": ["bin/codex"]},
                {"zap": [{"delete": "~/.codex"}]}
            ]);
        });
        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
        assert_eq!(plan.remove.len(), 1);
        Ok(())
    }

    #[test]
    fn homebrew_uninstall_actions_cover_recorded_native_directives() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        value["uninstall_artifacts"] = serde_json::json!([
            {"uninstall": [{
                "pkgutil": ["com.example.one", "com.example.two"],
                "delete": "/Library/Example",
                "quit": "com.example.app",
                "launchctl": "com.example.agent"
            }]},
            {"zap": [{"delete": "~/.example"}]}
        ]);
        let receipt: receipt::CaskReceipt = serde_json::from_value(value)?;
        assert_eq!(
            homebrew_uninstall_actions("example", &receipt)?,
            vec![
                HomebrewUninstallAction::Launchctl("com.example.agent".to_string()),
                HomebrewUninstallAction::Quit("com.example.app".to_string()),
                HomebrewUninstallAction::Pkgutil("com.example.one".to_string()),
                HomebrewUninstallAction::Pkgutil("com.example.two".to_string()),
                HomebrewUninstallAction::Delete(PathBuf::from("/Library/Example")),
            ]
        );
        Ok(())
    }

    #[test]
    fn gcloud_uninstall_trash_is_typed_and_confined() -> Result<()> {
        let artifacts = serde_json::json!([{
            "uninstall": [{
                "trash": "$HOMEBREW_PREFIX/Caskroom/gcloud-cli/latest"
            }]
        }]);
        let actions =
            homebrew_uninstall_actions_from_artifacts("gcloud-cli", artifacts.as_array().unwrap())?;
        assert_eq!(
            actions,
            vec![HomebrewUninstallAction::Trash(PathBuf::from(
                "$HOMEBREW_PREFIX/Caskroom/gcloud-cli/latest"
            ))]
        );
        assert!(validate_homebrew_uninstall_actions("gcloud-cli", "581.0.0", &actions).is_ok());
        assert!(
            validate_homebrew_uninstall_actions(
                "gcloud-cli",
                "581.0.0",
                &[HomebrewUninstallAction::Trash(PathBuf::from("/"))],
            )
            .unwrap_err()
            .to_string()
            .contains("protected path")
        );
        Ok(())
    }

    #[test]
    fn tunnelblick_uninstall_ownership_preflight_is_typed_and_app_bound() -> Result<()> {
        let mut cask = test_cask("tunnelblick", "8.0.3,6303");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"uninstall_preflight_steps": [{"steps": [{
                "type": "set_ownership",
                "paths": [{"base": "appdir", "path": "Tunnelblick.app"}]
            }]}]},
            {"uninstall": [{"quit": "net.tunnelblick.tunnelblick"}]},
            {"app": ["Tunnelblick.app"]}
        ]))?;

        let plan = cask_uninstall_flight_plan(&cask)?;

        assert_eq!(plan.preflight.len(), 1);
        assert_eq!(plan.preflight[0].user, None);
        assert_eq!(plan.preflight[0].group, None);
        assert!(plan.preflight[0].recursive);
        assert_eq!(
            homebrew_uninstall_actions_from_artifacts(&cask.token, &cask.artifacts)?,
            vec![HomebrewUninstallAction::Quit(
                "net.tunnelblick.tunnelblick".to_string()
            )]
        );
        Ok(())
    }

    #[test]
    fn uninstall_ownership_preflight_rejects_non_app_targets() -> Result<()> {
        let mut cask = test_cask("unsafe", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"uninstall_preflight_steps": [{"steps": [{
                "type": "set_ownership",
                "paths": [{"base": "homebrew_prefix", "path": "bin"}]
            }]}]},
            {"app": ["Unsafe.app"]}
        ]))?;

        let error = cask_uninstall_flight_plan(&cask).unwrap_err().to_string();

        assert!(error.contains("restricted to appdir"));
        Ok(())
    }

    #[test]
    fn uninstall_postflight_steps_remain_fail_closed() -> Result<()> {
        let mut cask = test_cask("unsafe", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"uninstall_postflight_steps": [{"steps": [{
                "type": "set_ownership",
                "paths": [{"base": "appdir", "path": "Unsafe.app"}]
            }]}]},
            {"app": ["Unsafe.app"]}
        ]))?;

        let error = cask_uninstall_flight_plan(&cask).unwrap_err().to_string();

        assert!(error.contains("uninstall_postflight_steps are unsupported"));
        Ok(())
    }

    #[test]
    fn predecessor_receipt_action_executes_and_is_journaled_before_successor() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let delete_target = tmp.path().join("predecessor-owned-state");
        crate::file::write(&delete_target, "old")?;
        let mut value: Value =
            serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        value["source"]["version"] = Value::String("1.0.0".to_string());
        value["uninstall_artifacts"] = serde_json::json!([
            {"uninstall": [{"delete": delete_target}]}
        ]);
        let homebrew: receipt::CaskReceipt = serde_json::from_value(value)?;
        let token_dir = caskroom_token_dir("example");
        file::create_dir_all(token_dir.join(".metadata"))?;
        file::write(
            token_dir.join(".metadata/config.json"),
            native_cask_config()?.to_json_bytes()?,
        )?;
        let candidate = CaskPruneCandidate {
            token: "example".to_string(),
            version: "1.0.0".to_string(),
            version_dir: caskroom_version_dir("example", "1.0.0"),
            receipt: synthetic_homebrew_prune_receipt("example", &homebrew)?,
            homebrew_receipt: Some(homebrew.clone()),
        };
        let mut journal = CaskTransactionJournal {
            schema_version: 2,
            token: "example".to_string(),
            version: "2.0.0".to_string(),
            phase: CaskTransactionPhase::Prepared,
            recovery: CaskRecoveryMode::DiscardStaging,
            receipt_inventory_targets: Vec::new(),
            activation_targets: Vec::new(),
            predecessor_targets: Vec::new(),
            had_predecessor_metadata: true,
            reopen_bundle_ids: Vec::new(),
            completed: Vec::new(),
        };
        write_cask_journal_in(&state_dir, &journal)?;

        execute_uninstall_recording_in(
            &state_dir,
            &candidate,
            &homebrew,
            &mut journal,
            "predecessor_uninstall",
            CaskTransactionPhase::Staging,
            true,
        )?;

        assert!(!delete_target.exists());
        assert_eq!(
            journal.completed,
            vec!["predecessor_uninstall[0]".to_string()]
        );
        assert_eq!(journal.phase, CaskTransactionPhase::Staging);
        assert_eq!(journal.recovery, CaskRecoveryMode::Manual);
        Ok(())
    }

    #[test]
    fn predecessor_reopen_intent_is_durable_and_deduplicated() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let state_dir = tmp.path().join("state");
        let mut journal = CaskTransactionJournal {
            schema_version: 2,
            token: "example".to_string(),
            version: "2.0.0".to_string(),
            phase: CaskTransactionPhase::Prepared,
            recovery: CaskRecoveryMode::DiscardStaging,
            receipt_inventory_targets: Vec::new(),
            activation_targets: Vec::new(),
            predecessor_targets: Vec::new(),
            had_predecessor_metadata: true,
            reopen_bundle_ids: Vec::new(),
            completed: Vec::new(),
        };

        record_reopen_bundle_in(&state_dir, &mut journal, "com.example.app")?;
        record_reopen_bundle_in(&state_dir, &mut journal, "com.example.app")?;

        assert_eq!(journal.reopen_bundle_ids, ["com.example.app"]);
        let persisted = read_pending_cask_journal_in(&state_dir, "example")?.unwrap();
        assert_eq!(persisted.reopen_bundle_ids, ["com.example.app"]);
        assert!(record_reopen_bundle_in(&state_dir, &mut journal, "../invalid").is_err());
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn parses_and_validates_script_and_signal_teardown() -> Result<()> {
        let mut cask = test_cask("supported-teardown", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({"app": "Example.app"}),
            serde_json::json!({"uninstall": [{
                "signal": ["TERM", "com.example.app"],
                "script": {
                    "executable": "$APPDIR/Example.app/uninstall.sh",
                    "args": ["--all"],
                    "sudo": true
                }
            }]}),
        ];
        let artifacts = cask_artifacts(&cask)?;
        validate_platform_support(&cask, &artifacts)?;
        assert_eq!(
            homebrew_uninstall_actions_from_artifacts(&cask.token, &cask.artifacts)?,
            vec![
                HomebrewUninstallAction::Signal {
                    signal: "TERM".to_string(),
                    bundle_id: "com.example.app".to_string(),
                },
                HomebrewUninstallAction::Script {
                    executable: "$APPDIR/Example.app/uninstall.sh".to_string(),
                    args: vec!["--all".to_string()],
                    sudo: true,
                    must_succeed: true,
                },
            ]
        );

        cask.artifacts[1] = serde_json::json!({"uninstall": [{
            "signal": ["INVALID", "com.example.app"]
        }]});
        let artifacts = cask_artifacts(&cask)?;
        assert!(
            validate_platform_support(&cask, &artifacts)
                .unwrap_err()
                .to_string()
                .contains("signal is unsupported")
        );
        Ok(())
    }

    #[test]
    fn confined_structured_run_passes_fresh_preflight() -> Result<()> {
        let mut cask = test_cask("confined-run", "1.0.0");
        cask.artifacts = serde_json::json!([
            {"app": "Example.app"},
            {"postflight_steps": [{"steps": [{
                "type": "run",
                "command": {"path": "bin/configure", "base": "staged_path"},
                "args": []
            }]}]}
        ])
        .as_array()
        .unwrap()
        .clone();

        let artifacts = cask_artifacts(&cask)?;
        validate_platform_support(&cask, &artifacts)?;

        cask.artifacts = serde_json::json!([
            {"app": "Example.app"},
            {"postflight_steps": [{"steps": [{
                "type": "run",
                "command": {"path": "bin/configure", "base": "staged_path"},
                "sudo": true
            }]}]}
        ])
        .as_array()
        .unwrap()
        .clone();
        let artifacts = cask_artifacts(&cask)?;
        let err = validate_platform_support(&cask, &artifacts)
            .unwrap_err()
            .to_string();
        assert!(err.contains("elevation would escape process confinement"));
        Ok(())
    }

    #[test]
    fn structured_run_cannot_write_outside_allowlist() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("homebrew");
        let _guard = BrewPrefixGuard::set(&prefix);
        let staged = tmp.path().join("stage");
        let appdir = tmp.path().join("Applications");
        let outside = tmp.path().join("outside");
        file::create_dir_all(&staged)?;
        file::create_dir_all(&appdir)?;

        let result = execute_flight_steps(
            &test_cask("confined-run", "1.0.0"),
            &[FlightStep::Run {
                command: FlightPath {
                    base: FlightPathBase::Literal,
                    path: "/bin/sh".to_string(),
                },
                args: vec![
                    "-c".to_string(),
                    "printf escaped > \"$1\"".to_string(),
                    "_".to_string(),
                    outside.display().to_string(),
                ],
                env: BTreeMap::new(),
                sudo: false,
                network_access: false,
                guards: Vec::new(),
            }],
            &staged,
            &appdir,
            "postflight_steps",
        );

        assert!(result.is_err());
        assert!(outside.symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn guarded_external_symlink_requires_receipt_only_when_executed() -> Result<()> {
        let mut cask = test_cask("guarded-symlink", "1.0.0");
        cask.artifacts = serde_json::json!([
            {"app": "Example.app"},
            {"postflight_steps": [{"steps": [{
                "type": "symlink",
                "source": {"base": "staged_path", "path": "config"},
                "target": {"base": "homebrew_prefix", "path": "share/example/config"},
                "guards": [{
                    "condition": "unless_exists",
                    "base": "homebrew_prefix",
                    "path": "share/example/config"
                }]
            }]}]}
        ])
        .as_array()
        .unwrap()
        .clone();

        let artifacts = cask_artifacts(&cask)?;
        validate_platform_support(&cask, &artifacts)?;

        let executed = PathBuf::from("/opt/homebrew/share/example/config");
        assert!(requires_auxiliary_cask_receipt(
            false,
            &BTreeSet::new(),
            std::slice::from_ref(&executed),
            &[],
        ));
        assert!(!requires_auxiliary_cask_receipt(
            false,
            &BTreeSet::new(),
            &[],
            &[],
        ));
        Ok(())
    }

    #[test]
    fn homebrew_uninstall_rejects_flight_blocks_and_unsafe_delete_paths() -> Result<()> {
        let mut value: Value =
            serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        value["uninstall_flight_blocks"] = Value::Bool(true);
        let receipt: receipt::CaskReceipt = serde_json::from_value(value.clone())?;
        assert!(
            validate_homebrew_uninstall_artifacts("example", &receipt)
                .unwrap_err()
                .to_string()
                .contains("flight blocks")
        );

        value["uninstall_flight_blocks"] = Value::Bool(false);
        value["uninstall_artifacts"] = serde_json::json!([
            {"uninstall": [{"delete": "relative/path"}]}
        ]);
        let receipt: receipt::CaskReceipt = serde_json::from_value(value)?;
        assert!(
            validate_homebrew_uninstall_artifacts("example", &receipt)
                .unwrap_err()
                .to_string()
                .contains("absolute normalized path")
        );

        value = serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        value["uninstall_artifacts"] = serde_json::json!([
            {"uninstall": [{"delete": "/"}]}
        ]);
        let receipt: receipt::CaskReceipt = serde_json::from_value(value)?;
        assert!(
            validate_homebrew_uninstall_artifacts("example", &receipt)
                .unwrap_err()
                .to_string()
                .contains("protected path")
        );

        value = serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        value["uninstall_artifacts"] = serde_json::json!([
            {"uninstall": [{"quit": "com.example\"; do shell script \"id"}]}
        ]);
        let receipt: receipt::CaskReceipt = serde_json::from_value(value)?;
        assert!(
            validate_homebrew_uninstall_artifacts("example", &receipt)
                .unwrap_err()
                .to_string()
                .contains("bundle identifier is invalid")
        );

        for kind in ["uninstall_preflight", "uninstall_postflight"] {
            value = serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
            value["uninstall_artifacts"] = serde_json::json!([{(kind): []}]);
            let receipt: receipt::CaskReceipt = serde_json::from_value(value)?;
            assert!(
                validate_homebrew_uninstall_artifacts("example", &receipt)
                    .unwrap_err()
                    .to_string()
                    .contains(kind)
            );
        }

        value = serde_json::from_str(include_str!("testdata/codex-INSTALL_RECEIPT.json"))?;
        value["uninstall_artifacts"] = serde_json::json!([{
            "postflight_steps": [{"steps": [{
                "type": "symlink",
                "source": {"base": "staged_path", "path": "source"},
                "target": {"base": "staged_path", "path": "target"},
                "uninstall": true
            }]}]
        }]);
        let receipt: receipt::CaskReceipt = serde_json::from_value(value)?;
        assert!(
            validate_homebrew_uninstall_artifacts("example", &receipt)
                .unwrap_err()
                .to_string()
                .contains("symlink uninstall step is unsupported")
        );
        Ok(())
    }

    #[test]
    fn cask_prune_fails_closed_when_a_receipt_is_corrupt() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

    #[cfg(unix)]
    #[test]
    fn cask_prune_fails_closed_when_a_token_directory_is_unreadable() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        write_test_app_receipt(&test_cask("clean", "1.0.0"), "Clean.app")?;
        let unreadable = caskroom_token_dir("unreadable");
        file::create_dir_all(&unreadable)?;
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000))?;

        let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir);
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755))?;
        let plan = plan?;

        assert!(plan.remove.is_empty());
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == "unreadable" && skip.reason.contains("directory could not be read")
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
                env: IndexMap::new(),
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
            env: IndexMap::new(),
        };
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let app_target = app_target_path(app.target_name())?;
        file::create_dir_all(&app_target)?;
        let receipt = CaskReceipt {
            schema_version: 0,
            version: cask.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: vec![app_target],
            binaries: Vec::new(),
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
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

        assert_eq!(installed_cask_version(&cask, &artifacts)?, None);

        let target = wrapper.target_path()?;
        file::create_dir_all(target.parent().unwrap())?;
        file::write(target, "wrapper")?;
        assert_eq!(installed_cask_version(&cask, &artifacts)?, None);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_and_links_binary_artifact() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

        stage_primary_container(&stage, &caskroom)?;
        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
        link_binary(&caskroom, &cask, &[], Path::new("/Applications"), &binary)?;

        let target = binary.target_path(Path::new("/Applications"))?;
        assert_eq!(std::fs::read_link(&target)?, caskroom.join("op"));
        assert!(caskroom.join("bin/op").symlink_metadata().is_err());
        assert_eq!(crate::file::read_to_string(&target)?, "binary");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn appdir_binary_links_directly_to_moved_app() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = caskroom_version_dir("codexbar", "1.0.0");
        let staged_binary = caskroom.join("CodexBar.app/Contents/Helpers/CodexBarCLI");
        file::create_dir_all(staged_binary.parent().unwrap())?;
        crate::file::write(&staged_binary, "binary")?;
        let mut cask = test_cask("codexbar", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["CodexBar.app", {"target": "$HOMEBREW_PREFIX/Applications/CodexBar.app"}]},
            {"binary": ["$APPDIR/CodexBar.app/Contents/Helpers/CodexBarCLI", {"target": "$HOMEBREW_PREFIX/bin/codexbar"}]}
        ]))?;
        let artifacts = cask_artifacts(&cask)?;
        let app = &artifacts.apps[0];
        let binary = &artifacts.binaries[0];

        stage_binary(
            tmp.path().join("stage").as_path(),
            &caskroom,
            &cask,
            std::slice::from_ref(app),
            binary,
        )?;
        activate_app(&caskroom, app, true)?;
        let appdir = cask_appdir(std::slice::from_ref(app))?;
        link_binary(&caskroom, &cask, std::slice::from_ref(app), &appdir, binary)?;

        let app_binary = appdir.join("CodexBar.app/Contents/Helpers/CodexBarCLI");
        let target = binary.target_path(&appdir)?;
        assert_eq!(std::fs::read_link(&target)?, app_binary);
        assert!(caskroom.join("bin/codexbar").symlink_metadata().is_err());
        validate_installed_cask_topology(&cask, &artifacts, &caskroom)?;
        assert!(successor_owns_public_target(&cask, &target));
        let foreign = tmp.path().join("foreign-codexbar");
        crate::file::write(&foreign, "foreign")?;
        file::remove_file(&target)?;
        file::make_symlink(&foreign, &target)?;
        assert!(!successor_owns_public_target(&cask, &target));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_same_basename_binaries_without_collision() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

        stage_primary_container(&stage, &caskroom)?;
        stage_binary(&stage, &caskroom, &cask, &[], &bin)?;
        stage_binary(&stage, &caskroom, &cask, &[], &sbin)?;
        link_binary(&caskroom, &cask, &[], Path::new("/Applications"), &bin)?;
        link_binary(&caskroom, &cask, &[], Path::new("/Applications"), &sbin)?;

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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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

        assert_eq!(crate::file::read_to_string(caskroom.join("op"))?, "hook");
        assert!(caskroom.join("bin/op").symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn links_rather_than_copies_a_binary_behind_a_flight_symlink() -> Result<()> {
        // gcloud-cli's preflight installs the SDK under the prefix and leaves
        // `staged_path/google-cloud-sdk` as a link to it. The launcher derives
        // CLOUDSDK_ROOT_DIR from the resolved path of `$0`, so copying it into
        // the caskroom — out of the tree holding `lib/` — would stage a broken
        // binary. It has to be linked, like Homebrew does.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let installed = tmp.path().join("share/google-cloud-sdk");
        file::create_dir_all(&stage)?;
        file::create_dir_all(installed.join("bin"))?;
        file::create_dir_all(installed.join("lib"))?;
        crate::file::write(installed.join("bin/gcloud"), "launcher")?;
        std::os::unix::fs::symlink(&installed, stage.join("google-cloud-sdk"))?;
        let caskroom = caskroom_version_dir("gcloud-cli", "531.0.0");
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("gcloud-cli", "531.0.0");
        let binary = BinaryArtifact {
            source: "google-cloud-sdk/bin/gcloud".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/gcloud".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

        let staged = caskroom.join("bin/gcloud");
        assert_eq!(
            std::fs::read_link(&staged)?,
            file::desymlink_path(&installed.join("bin/gcloud")),
            "must link into the SDK tree, not copy the launcher out of it"
        );
        // Still resolves, and `lib/` is a sibling of the link target.
        assert_eq!(crate::file::read_to_string(&staged)?, "launcher");
        assert!(
            std::fs::read_link(&staged)?
                .parent()
                .and_then(Path::parent)
                .is_some_and(|root| root.join("lib").is_dir())
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn links_a_stage_symlink_at_its_target_so_it_survives_teardown() -> Result<()> {
        // The walk matches symlink entries by name and `is_file` follows them,
        // so a stage-local link to a durable binary comes back as a stage path
        // that resolves outside the stage. Linking the caskroom entry at that
        // literal path would dangle the moment staging tears the stage down.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let stage = tmp.path().join("stage");
        let durable = tmp.path().join("opt/vendor/tool");
        file::create_dir_all(&stage)?;
        file::create_dir_all(durable.parent().unwrap())?;
        crate::file::write(&durable, "durable")?;
        std::os::unix::fs::symlink(&durable, stage.join("tool"))?;
        let caskroom = caskroom_version_dir("linked-binary", "1.0.0");
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("linked-binary", "1.0.0");
        let binary = BinaryArtifact {
            source: "tool".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

        let staged = caskroom.join("bin/tool");
        assert_eq!(
            std::fs::read_link(&staged)?,
            file::desymlink_path(&durable),
            "must link the real location, not the path through the stage"
        );
        // The decisive check: staging is over, so the stage is gone.
        file::remove_all(&stage)?;
        assert_eq!(crate::file::read_to_string(&staged)?, "durable");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_generic_artifact_through_a_symlinked_stage() -> Result<()> {
        // The lookup resolves links it traverses, so the source can be
        // contained by the stage without sharing its literal prefix. A lexical
        // strip would fail here even though the containment check passes.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let real_stage = tmp.path().join("real-stage");
        let payload = real_stage.join("libcblite-4.1.0/include/cbl");
        file::create_dir_all(&payload)?;
        file::write(payload.join("CouchbaseLite.h"), "header")?;
        std::os::unix::fs::symlink(
            real_stage.join("libcblite-4.1.0"),
            real_stage.join("current"),
        )?;
        let stage = tmp.path().join("stage");
        std::os::unix::fs::symlink(&real_stage, &stage)?;
        let artifact = GenericArtifact {
            source: "current/include/cbl".to_string(),
            target: "$HOMEBREW_PREFIX/include/cbl".to_string(),
        };

        let mut targets = FlightTargetTransaction::default();
        let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
        install_generic_artifact(&stage, &temporary_caskroom, &artifact, &mut targets)?;

        assert_eq!(
            file::read_to_string(tmp.path().join("include/cbl/CouchbaseLite.h"))?,
            "header"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn copies_a_binary_whose_link_stays_inside_a_symlinked_stage() -> Result<()> {
        // Mirror of the case above with the link pointing back into the stage,
        // reached through a stage path that is itself a symlink (a symlinked
        // `~/Library/Caches`). The resolved source then differs lexically from
        // `stage`, and treating that as a durable location would leave a
        // dangling binary once the stage is torn down.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let real_stage = tmp.path().join("real-stage");
        file::create_dir_all(real_stage.join("payload/bin"))?;
        crate::file::write(real_stage.join("payload/bin/tool"), "tool")?;
        std::os::unix::fs::symlink(real_stage.join("payload"), real_stage.join("link"))?;
        let stage = tmp.path().join("stage");
        std::os::unix::fs::symlink(&real_stage, &stage)?;
        let caskroom = caskroom_version_dir("linked-stage", "1.0.0");
        file::create_dir_all(&caskroom)?;
        let cask = test_cask("linked-stage", "1.0.0");
        let binary = BinaryArtifact {
            source: "link/bin/tool".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
        };

        stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

        let staged = caskroom.join("bin/tool");
        assert!(
            !staged.is_symlink(),
            "stage content must be copied, not linked"
        );
        assert_eq!(crate::file::read_to_string(&staged)?, "tool");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn stages_absolute_binary_source_from_pkg_install() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        link_binary(&caskroom, &cask, &[], Path::new("/Applications"), &binary)?;

        let target = binary.target_path(Path::new("/Applications"))?;
        assert_eq!(std::fs::read_link(&target)?, pkg_binary);
        assert_eq!(crate::file::read_to_string(&target)?, "pkg binary");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn reports_missing_target_for_dangling_staged_binary_symlink() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let err = link_binary(&caskroom, &cask, &[], Path::new("/Applications"), &binary)
            .unwrap_err()
            .to_string();

        assert!(err.contains("was not found"));
        Ok(())
    }

    #[test]
    fn cask_appdir_uses_prefix_for_prefix_targeted_apps() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };

        assert_eq!(cask_appdir(&[app])?, tmp.path().join("Applications"));
        Ok(())
    }

    #[test]
    fn app_target_path_defaults_to_platform_appdir() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.remove(APP_DIR_ENV);
        assert_eq!(
            app_target_path("Firefox.app")?,
            EffectiveCaskDirs::current().appdir.join("Firefox.app")
        );
        Ok(())
    }

    #[test]
    fn parse_app_artifact_target_without_slash_is_preserved() {
        // The Homebrew API commonly renders `app` targets as a bare bundle
        // name. Parsing must keep it verbatim and must not consult the override.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, "/tmp/should-not-be-used");
        let value: Value =
            serde_json::json!({"app": ["Firefox.app", {"target": "Firefox Nightly.app"}]});
        assert_eq!(
            parse_app_artifact(&value),
            Some(AppArtifact {
                source: "Firefox.app".to_string(),
                target: Some("Firefox Nightly.app".to_string()),
            })
        );
    }

    #[test]
    fn parse_app_artifact_preserves_prefix_target() {
        // A `$HOMEBREW_PREFIX`-anchored target must survive parsing so that
        // `cask_appdir`/`app_target_path` can route it into the prefix.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, "/tmp/should-not-be-used");
        let value: Value = serde_json::json!({
            "app": ["Example.app", {"target": "$HOMEBREW_PREFIX/Applications/Example.app"}]
        });
        assert_eq!(
            parse_app_artifact(&value),
            Some(AppArtifact {
                source: "Example.app".to_string(),
                target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
            })
        );
    }

    #[test]
    fn app_target_path_honours_appdir_override() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        // target_app_dir canonicalizes the override, so compare against the
        // resolved base (macOS tempdirs live under the `/var` symlink).
        let base = tmp.path().canonicalize()?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &base);
        assert_eq!(app_target_path("Firefox.app")?, base.join("Firefox.app"));
        Ok(())
    }

    #[test]
    fn app_target_path_accepts_absolute_target_under_override() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &base);
        let target = base.join("Firefox.app");
        assert_eq!(app_target_path(&target.to_string_lossy())?, target);
        Ok(())
    }

    #[test]
    fn app_target_path_relocates_default_applications_target() -> Result<()> {
        // The Homebrew API frequently hardcodes an absolute
        // `/Applications/Foo.app` target (e.g. the firefox cask). With an
        // override configured this must be relocated into it.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &base);
        assert_eq!(
            app_target_path("/Applications/Firefox.app")?,
            base.join("Firefox.app")
        );
        Ok(())
    }

    #[test]
    fn app_target_path_relocation_preserves_subdirectories() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &base);
        assert_eq!(
            app_target_path("/Applications/JetBrains/IDEA.app")?,
            base.join("JetBrains/IDEA.app")
        );
        Ok(())
    }

    #[test]
    fn app_target_path_defaults_follow_platform_appdir() -> Result<()> {
        // Without an override, macOS keeps `/Applications` while Linux
        // relocates that cask DSL default into Homebrew's platform appdir.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.remove(APP_DIR_ENV);
        assert_eq!(
            app_target_path("/Applications/Firefox.app")?,
            EffectiveCaskDirs::current().appdir.join("Firefox.app")
        );
        Ok(())
    }

    #[test]
    fn app_target_path_rejects_target_outside_override() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &base);
        let err = app_target_path("/Users/someone/Evil.app")
            .unwrap_err()
            .to_string();
        assert!(err.contains(&base.to_string_lossy().to_string()), "{err}");
        assert!(!err.contains("/Applications"), "{err}");
        Ok(())
    }

    #[test]
    fn cask_appdir_uses_override() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let _prefix = BrewPrefixGuard::set(&base.join("prefix"));
        let appdir = base.join("appdir");
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &appdir);
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: None,
        };
        assert_eq!(cask_appdir(&[app])?, appdir);
        Ok(())
    }

    #[test]
    fn native_cask_config_records_appdir_override() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("appdir");
        file::create_dir_all(&appdir)?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &appdir);

        let config = native_cask_config()?;
        assert_eq!(config.default["appdir"], serde_json::json!(appdir));
        Ok(())
    }

    #[test]
    fn command_wrapper_target_path_uses_override() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("appdir");
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &appdir);
        // Only `$APPDIR`-anchored targets resolve into the appdir; a bare name
        // lands under the prefix's bin, so anchor the target to exercise the
        // override path.
        let wrapper = CommandWrapperArtifact {
            name: "gimp".to_string(),
            target: Some("$APPDIR/GIMP.app/Contents/MacOS/gimp".to_string()),
            content: None,
            executable: None,
            args: Vec::new(),
            env: IndexMap::new(),
        };
        assert_eq!(
            wrapper.target_path()?,
            appdir.join("GIMP.app/Contents/MacOS/gimp"),
        );
        Ok(())
    }

    #[test]
    fn allowed_appdir_roots_has_no_duplicates() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let _prefix = BrewPrefixGuard::set(&base.join("prefix"));

        let mut _guard = EnvVarGuard::new();
        _guard.remove(APP_DIR_ENV);
        let roots = allowed_appdir_roots()?;
        let unique: BTreeSet<_> = roots.iter().collect();
        assert_eq!(unique.len(), roots.len(), "{roots:?}");
        assert!(roots.contains(&EffectiveCaskDirs::current().appdir));

        let appdir = base.join("appdir");
        _guard.set(APP_DIR_ENV, &appdir);
        let roots = allowed_appdir_roots()?;
        let unique: BTreeSet<_> = roots.iter().collect();
        assert_eq!(unique.len(), roots.len(), "{roots:?}");
        assert!(roots.contains(&appdir));
        Ok(())
    }

    #[test]
    fn binary_target_path_accepts_override_appdir() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("appdir");
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &appdir);
        assert_eq!(
            binary_target_path("$APPDIR/Foo.app/Contents/MacOS/foo", &appdir)?,
            appdir.join("Foo.app/Contents/MacOS/foo"),
        );
        Ok(())
    }

    #[test]
    fn ensure_trusted_appdir_refuses_world_writable_ancestor() -> Result<()> {
        // Regression guard for the CI failure: a world-writable ancestor (as
        // `/tmp` is, mode 1777) must be refused, because any local user could
        // substitute components beneath it. Real application directories are
        // never world-writable.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let shared = base.join("shared");
        file::create_dir_all(&shared)?;
        let mode = std::fs::Permissions::from_mode(0o1777);
        std::fs::set_permissions(&shared, mode)?;
        let err = match ensure_trusted_appdir(&shared.join("Applications")) {
            Ok(_) => panic!("expected world-writable ancestor to be refused"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("untrusted directory"), "{err}");
        Ok(())
    }

    #[test]
    fn ensure_trusted_appdir_creates_missing_tail() -> Result<()> {
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("Applications");
        ensure_trusted_appdir(&appdir)?;
        assert!(appdir.symlink_metadata()?.file_type().is_dir());
        // Idempotent when the directory already exists.
        ensure_trusted_appdir(&appdir)?;
        Ok(())
    }

    #[test]
    fn ensure_trusted_appdir_rejects_symlinked_tail() -> Result<()> {
        // Simulate a symlink planted on the not-yet-existing appdir tail
        // between validation and mutation: it must be rejected, not followed.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let elsewhere = base.join("elsewhere");
        file::create_dir_all(&elsewhere)?;
        let appdir = base.join("Applications");
        std::os::unix::fs::symlink(&elsewhere, &appdir)?;
        let err = match ensure_trusted_appdir(&appdir) {
            Ok(_) => panic!("expected symlinked appdir tail to be rejected"),
            Err(err) => err.to_string(),
        };
        // Must fail because the tail is a symlink, not because an ancestor was
        // untrusted (which is a different guard).
        assert!(err.contains("cannot open operation directory"), "{err}");
        assert!(!err.contains("untrusted directory"), "{err}");
        Ok(())
    }

    #[test]
    fn ensure_trusted_appdir_stays_bound_after_same_uid_replacement() -> Result<()> {
        // The reviewer's scenario: after validation, a same-uid process swaps
        // the accepted appdir for a different directory (or symlink). Because
        // the descriptor is retained and mutations are addressed through it,
        // writes still land in the originally validated directory.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("Applications");
        let parent = ensure_trusted_appdir(&appdir)?;

        // Swap the validated directory aside and put an attacker-controlled
        // path in its place.
        let stashed = base.join("stashed");
        std::fs::rename(&appdir, &stashed)?;
        let attacker = base.join("attacker");
        file::create_dir_all(&attacker)?;
        std::os::unix::fs::symlink(&attacker, &appdir)?;

        // Writing through the bound descriptor path must reach the original
        // directory (now at `stashed`), never the attacker's directory.
        let bound = parent.path()?;
        crate::file::write(bound.join("canary"), "bound")?;
        assert!(stashed.join("canary").is_file());
        assert!(!attacker.join("canary").exists());
        Ok(())
    }

    #[test]
    fn app_copy_into_stays_bound_after_directory_replacement() -> Result<()> {
        // Bind the appdir, then have a same-uid replacement swap the directory
        // pathname for an attacker-controlled one. The fd-bound copy must still
        // land in the originally validated directory.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("Applications");
        let parent = ensure_trusted_appdir(&appdir)?;

        let source = base.join("payload");
        file::create_dir_all(&source)?;
        crate::file::write(source.join("marker"), "payload")?;

        let stashed = base.join("stashed");
        std::fs::rename(&appdir, &stashed)?;
        let attacker = base.join("attacker");
        file::create_dir_all(&attacker)?;
        std::os::unix::fs::symlink(&attacker, &appdir)?;

        copy_app_bundle_into(&source, &parent.fd, std::ffi::OsStr::new("Copied.app"))?;
        assert!(stashed.join("Copied.app/marker").is_file());
        assert!(!attacker.join("Copied.app").exists());
        Ok(())
    }

    #[test]
    fn ensure_trusted_appdir_walks_from_unreplaceable_root() -> Result<()> {
        // The appdir is never re-opened via a scanned ancestor pathname: the
        // walk starts at `/` and descends only through verified descriptors.
        // Swapping an intermediate component for another same-uid-owned real
        // directory before the call therefore cannot be reached through a
        // previously-resolved root, and a symlink swap is rejected outright.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let middle = base.join("middle");
        file::create_dir_all(&middle)?;
        let appdir = middle.join("Applications");
        let parent = ensure_trusted_appdir(&appdir)?;
        assert!(appdir.symlink_metadata()?.file_type().is_dir());

        // Replace the intermediate component with a same-uid symlink: the next
        // walk must refuse it rather than following it.
        let attacker = base.join("attacker");
        file::create_dir_all(&attacker)?;
        std::fs::remove_dir_all(&middle)?;
        std::os::unix::fs::symlink(&attacker, &middle)?;
        assert!(ensure_trusted_appdir(&appdir).is_err());
        // Nothing was created inside the attacker's directory.
        assert!(!attacker.join("Applications").exists());
        drop(parent);
        Ok(())
    }

    #[test]
    fn app_copy_into_rejects_preplanted_symlink_destination() -> Result<()> {
        // A same-uid process creates the predictable temporary name as a
        // symlink before the copy. The copy must fail closed rather than follow
        // it, so nothing is written outside the verified directory.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("Applications");
        let parent = ensure_trusted_appdir(&appdir)?;

        let source = base.join("payload");
        file::create_dir_all(&source)?;
        crate::file::write(source.join("marker"), "payload")?;

        let attacker = base.join("attacker");
        file::create_dir_all(&attacker)?;
        let tmp_name = std::ffi::OsStr::new("Foo.mise-tmp-abc");
        std::os::unix::fs::symlink(&attacker, appdir.join(tmp_name))?;

        // Fails at `mkdirat` (EEXIST) before any payload is copied.
        let err = match copy_app_bundle_into(&source, &parent.fd, tmp_name) {
            Ok(()) => panic!("expected pre-planted symlink destination to be refused"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("cannot stage app bundle"), "{err}");
        assert!(!attacker.join("marker").exists());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn repair_app_permissions_does_not_traverse_bundle_symlinks() -> Result<()> {
        // A cask bundle may contain a symlink pointing outside the application
        // directory. The recursive flag/permission repair must not follow it and
        // change the referent.
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let appdir = base.join("Applications");
        let parent = ensure_trusted_appdir(&appdir)?;

        let outside = base.join("outside.txt");
        crate::file::write(&outside, "keep")?;
        let status = std::process::Command::new("/bin/chmod")
            .args(["644"])
            .arg(&outside)
            .status()?;
        assert!(status.success());

        let bundle = appdir.join("Victim.app");
        file::create_dir_all(&bundle)?;
        std::os::unix::fs::symlink(&outside, bundle.join("link"))?;

        repair_app_permissions_at(&parent, std::ffi::OsStr::new("Victim.app"));

        // The referent keeps its mode and gains no flags.
        let mode = std::process::Command::new("/usr/bin/stat")
            .args(["-f", "%Sp"])
            .arg(&outside)
            .output()?;
        let mode = String::from_utf8_lossy(&mode.stdout).trim().to_string();
        assert_eq!(mode, "-rw-r--r--", "referent mode changed: {mode}");
        let flags = std::process::Command::new("/usr/bin/stat")
            .args(["-f", "%Sf"])
            .arg(&outside)
            .output()?;
        let flags = String::from_utf8_lossy(&flags.stdout).trim().to_string();
        assert!(
            flags.is_empty() || flags == "-",
            "referent flags set: {flags}"
        );
        assert_eq!(crate::file::read_to_string(&outside)?, "keep");
        Ok(())
    }

    #[test]
    fn empty_appdir_override_falls_back_to_platform_appdir() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, "");
        assert_eq!(
            app_target_path("Firefox.app")?,
            EffectiveCaskDirs::current().appdir.join("Firefox.app")
        );
        assert!(app_target_path("/etc/passwd").is_err());
        Ok(())
    }

    #[test]
    fn relative_appdir_override_is_rejected() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, "relative/apps");
        assert!(app_target_path("Firefox.app").is_err());
        Ok(())
    }

    #[test]
    fn appdir_override_with_parent_dir_is_rejected() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, "/Applications/../etc");
        assert!(app_target_path("Firefox.app").is_err());
        Ok(())
    }

    #[test]
    fn appdir_override_root_alias_is_rejected() -> Result<()> {
        // Alternate spellings of the filesystem root must not become the
        // containment boundary.
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        for alias in ["/.", "//", "/./."] {
            let mut _guard = EnvVarGuard::new();
            _guard.set(APP_DIR_ENV, alias);
            assert!(
                app_target_path("Firefox.app").is_err(),
                "expected {alias} to be rejected"
            );
        }
        Ok(())
    }

    #[test]
    fn appdir_override_with_symlink_to_root_is_rejected() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        // An override that resolves to the filesystem root would make `/` the
        // containment boundary for privileged mutations, so it must be
        // rejected — including when reached through a symlink.
        let link = tmp.path().join("link-to-root");
        std::os::unix::fs::symlink("/", &link)?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &link);
        let err = app_target_path("Firefox.app").unwrap_err().to_string();
        assert!(
            err.contains("must not resolve to the filesystem root"),
            "{err}"
        );
        Ok(())
    }

    #[test]
    fn appdir_override_with_benign_symlink_is_resolved() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        // A symlink whose target is an ordinary directory (not root) is
        // accepted, but the boundary is the resolved real path so privileged
        // mutations cannot be redirected through the link.
        let real = tmp.path().join("real");
        file::create_dir_all(&real)?;
        let real = real.canonicalize()?;
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link)?;
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, &link);
        assert_eq!(app_target_path("Firefox.app")?, real.join("Firefox.app"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn failed_app_activation_preserves_caskroom_copy() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let _guard = BrewPrefixGuard::set(&base);
        let target = base.join("Applications/Example.app");
        file::create_dir_all(&target)?;
        file::write(target.join("version"), "old")?;
        let predecessor = CaskTargetRecord {
            path: target.clone(),
            fingerprint: cask_target_fingerprint(&target)?,
            uninstall: None,
        };

        let caskroom_app = base.join("Caskroom/example/2.0.0/Example.app");
        file::create_dir_all(&caskroom_app)?;
        file::write(caskroom_app.join("version"), "staged")?;
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };

        let mut transaction = ArtifactLinkTransaction::begin(vec![target.clone()], &[predecessor])?;
        file::create_dir_all(&target)?;
        file::write(target.join("version"), "appeared")?;
        let result = activate_app(caskroom_app.parent().unwrap(), &app, true);

        assert!(result.is_err());
        assert!(!caskroom_app.symlink_metadata()?.file_type().is_symlink());
        assert_eq!(
            file::read_to_string(caskroom_app.join("version"))?,
            "staged"
        );
        transaction.rollback()?;
        assert_eq!(file::read_to_string(target.join("version"))?, "old");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn upgrades_app_with_protected_existing_contents() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let base = tmp.path().canonicalize()?;
        let _guard = BrewPrefixGuard::set(&base);
        let mut _env = EnvVarGuard::new();
        _env.remove(APP_DIR_ENV);
        let target = base.join("Applications/Docker.app");
        let protected_dir = target.join("Contents/Resources");
        file::create_dir_all(&protected_dir)?;
        crate::file::write(protected_dir.join("docker"), "old")?;
        let status = std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone deny delete_child"])
            .arg(&protected_dir)
            .status()?;
        assert!(status.success());

        let predecessor = CaskTargetRecord {
            path: target.clone(),
            fingerprint: cask_target_fingerprint(&target)?,
            uninstall: None,
        };
        let caskroom = base.join("Caskroom/docker/2.0.0");
        let staged_app = caskroom.join("Docker.app");
        file::create_dir_all(&staged_app)?;
        crate::file::write(staged_app.join("version"), "new")?;
        let app = AppArtifact {
            source: "Docker.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Docker.app".to_string()),
        };
        let mut transaction = ArtifactLinkTransaction::begin(vec![target.clone()], &[predecessor])?;
        activate_app(&caskroom, &app, true)?;

        // Remove the ACL so tempfile can clean up even when the repro fails.
        let old_target = target.with_file_name(format!(
            ".mise-link-backup-{}",
            crate::hash::hash_to_str(&target.display().to_string())
        ));
        if old_target.exists() {
            let status = std::process::Command::new("/bin/chmod")
                .arg("-RN")
                .arg(&old_target)
                .status()?;
            assert!(status.success());
        }

        transaction.commit()?;
        assert_eq!(crate::file::read_to_string(target.join("version"))?, "new");
        assert!(caskroom.join("Docker.app").is_symlink());
        assert!(file::same_file(&caskroom.join("Docker.app"), &target));
        assert!(!old_target.exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn successful_app_activation_keeps_public_app_and_caskroom_backlink() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/example/1.0.0");
        let staged_app = caskroom.join("Example.app");
        file::create_dir_all(&staged_app)?;
        crate::file::write(staged_app.join("payload"), "installed")?;
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let target = app_target_path(app.target_name())?;

        let mut transaction = ArtifactLinkTransaction::begin(vec![target.clone()], &[])?;
        activate_app(&caskroom, &app, true)?;
        transaction.commit()?;

        assert_eq!(
            crate::file::read_to_string(target.join("payload"))?,
            "installed"
        );
        assert!(caskroom.join("Example.app").is_symlink());
        assert!(file::same_file(&caskroom.join("Example.app"), &target));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn tracked_app_activation_retains_native_caskroom_backlink() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/example/1.0.0");
        let staged_app = caskroom.join("Example.app");
        file::create_dir_all(&staged_app)?;
        file::write(staged_app.join("payload"), "installed")?;
        let app = AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        };
        let target = app_target_path(app.target_name())?;

        let mut transaction = ArtifactLinkTransaction::begin(vec![target.clone()], &[])?;
        activate_app(&caskroom, &app, true)?;
        transaction.commit()?;

        assert_eq!(file::read_to_string(target.join("payload"))?, "installed");
        assert!(file::same_file(&caskroom.join("Example.app"), &target));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn app_and_manpage_activation_commit_distinct_owned_targets() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = trusted_tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let caskroom = tmp.path().join("Caskroom/ghostty/1.2.0");
        let staged_app = caskroom.join("Ghostty.app");
        let manpage_source = staged_app.join("Contents/Resources/man/ghostty.1");
        file::create_dir_all(manpage_source.parent().unwrap())?;
        crate::file::write(&manpage_source, "ghostty manual")?;
        let app = AppArtifact {
            source: "Ghostty.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Ghostty.app".to_string()),
        };
        let manpage = ManpageArtifact {
            source: "$APPDIR/Ghostty.app/Contents/Resources/man/ghostty.1".to_string(),
            section: "1".to_string(),
        };
        let app_target = app_target_path(app.target_name())?;
        let manpage_target = manpage_target_path(&manpage)?;
        let mut transaction =
            ArtifactLinkTransaction::begin(vec![app_target.clone(), manpage_target.clone()], &[])?;

        activate_app(&caskroom, &app, true)?;
        link_manpage(&caskroom, std::slice::from_ref(&app), &manpage)?;
        transaction.commit()?;

        assert!(app_target.is_dir());
        assert_eq!(
            crate::file::read_to_string(&manpage_target)?,
            "ghostty manual"
        );
        assert!(manpage_target.is_symlink());
        assert!(file::same_file(&manpage_target, &manpage_source));
        assert_eq!(
            std::fs::read_link(&manpage_target)?,
            app_target.join("Contents/Resources/man/ghostty.1")
        );
        assert!(manpage_target_is_owned(
            &manpage,
            std::slice::from_ref(&app),
            &manpage_target,
            &caskroom,
        )?);
        Ok(())
    }

    #[test]
    fn activation_rejects_unowned_existing_target_without_mutation() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let directory = tmp.path().join("manual-app");
        file::create_dir_all(&directory)?;
        crate::file::write(directory.join("payload"), "operator-owned")?;
        let regular_file = tmp.path().join("manual-font.ttf");
        crate::file::write(&regular_file, "operator-font")?;
        let foreign_source = tmp.path().join("foreign-binary");
        crate::file::write(&foreign_source, "operator-binary")?;
        let foreign_link = tmp.path().join("bin-link");
        file::make_symlink(&foreign_source, &foreign_link)?;

        for target in [&directory, &regular_file, &foreign_link] {
            let err = ArtifactLinkTransaction::begin(vec![target.clone()], &[])
                .unwrap_err()
                .to_string();
            assert!(err.contains("ambiguous ownership"));
            assert!(!artifact_backup_path(target)?.exists());
        }
        assert_eq!(
            crate::file::read_to_string(directory.join("payload"))?,
            "operator-owned"
        );
        assert_eq!(crate::file::read_to_string(&regular_file)?, "operator-font");
        assert!(foreign_link.is_symlink());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn remove_obsolete_binary_links_removes_only_caskroom_symlinks() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("pkg-only", "1.0.0");
        let caskroom = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&caskroom)?;
        let receipt = CaskReceipt {
            schema_version: 0,
            version: cask.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: vec![],
            binaries: vec![],
            fonts: vec![],
            manpages: vec![],
            completions: vec![],
            flight_directories: vec![],
            generic: vec![],
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
            None
        );
        Ok(())
    }

    #[test]
    fn installed_cask_version_rejects_app_state_without_receipt() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: vec![app_target_path(
                "$HOMEBREW_PREFIX/Applications/Example.app",
            )?],
            binaries: Vec::new(),
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
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
            None
        );
        Ok(())
    }

    #[test]
    fn homebrew_receipt_reports_opaque_installed_version() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("homebrew-owned", "current");
        write_homebrew_cask_receipt(&cask.token, "0.147.0@preview,1", |receipt| {
            receipt["uninstall_artifacts"] = Value::Array(Vec::new());
        });

        assert_eq!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::Installed("0.147.0@preview,1".to_string())
        );
        Ok(())
    }

    #[test]
    fn homebrew_receipt_older_than_catalog_still_reports_installed() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("homebrew-owned", "9.0.0");
        write_homebrew_cask_receipt(&cask.token, "1.0.0", |receipt| {
            receipt["uninstall_artifacts"] = Value::Array(Vec::new());
        });

        assert_eq!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::Installed("1.0.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn homebrew_receipt_with_custom_relevant_directory_needs_repair() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("custom-appdir", "1.0.0");
        write_homebrew_cask_receipt(&cask.token, &cask.version, |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([{"app": ["Example.app"]}]);
        });
        let token_dir = caskroom_token_dir(&cask.token);
        let custom_appdir = tmp.path().join("custom-apps");
        let mut config = serde_json::to_value(native_cask_config()?)?;
        config["explicit"]["appdir"] = serde_json::json!(custom_appdir);
        file::write(
            token_dir.join(".metadata/config.json"),
            serde_json::to_vec(&config)?,
        )?;

        let InstalledCaskState::NeedsRepair {
            reason,
            replacement_safe,
            ..
        } = installed_cask_state(&cask, &CaskArtifacts::default())?
        else {
            panic!("custom relevant Homebrew config must not use default target paths");
        };
        assert!(reason.contains("unsupported custom appdir"));
        assert!(!replacement_safe);
        assert!(!custom_appdir.exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn homebrew_receipt_topology_uses_installed_artifacts_not_current_catalog() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut current = test_cask("homebrew-owned", "9.0.0");
        current.artifacts = serde_json::from_value(serde_json::json!([{
            "binary": ["bin/new", {"target": "new"}]
        }]))?;
        let current_artifacts = cask_artifacts(&current)?;
        write_homebrew_cask_receipt(&current.token, "1.0.0", |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([{
                "binary": ["bin/old", {"target": "old"}]
            }]);
        });
        let version_dir = caskroom_version_dir(&current.token, "1.0.0");
        let source = version_dir.join("bin/old");
        let target = prefix::prefix().join("bin/old");
        file::create_dir_all(source.parent().unwrap())?;
        file::create_dir_all(target.parent().unwrap())?;
        file::write(&source, "old")?;
        file::make_symlink(&source, &target)?;

        assert_eq!(
            installed_cask_state(&current, &current_artifacts)?,
            InstalledCaskState::Installed("1.0.0".to_string())
        );

        file::remove_file(&target)?;
        let InstalledCaskState::NeedsRepair {
            reason,
            replacement_safe,
            ..
        } = installed_cask_state(&current, &current_artifacts)?
        else {
            panic!("missing installed receipt artifact must need repair");
        };
        assert!(reason.contains("bin/old"));
        assert!(replacement_safe);

        file::write(&target, "foreign")?;
        let InstalledCaskState::NeedsRepair {
            replacement_safe, ..
        } = installed_cask_state(&current, &current_artifacts)?
        else {
            panic!("foreign replacement artifact must need repair");
        };
        assert!(!replacement_safe);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn missing_app_with_owned_dangling_appdir_links_is_replacement_safe() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("appdir-repair", "1.0.0");
        write_homebrew_cask_receipt(&cask.token, &cask.version, |receipt| {
            receipt["uninstall_artifacts"] = serde_json::json!([
                {"app": ["Example.app"]},
                {"binary": ["$APPDIR/Example.app/Contents/MacOS/example", {"target": "example"}]},
                {"binary": ["$APPDIR/Example.app/Contents/Applications/Dashboard.app", {"target": "$APPDIR/Dashboard.app"}]}
            ]);
        });
        let app = EffectiveCaskDirs::current().appdir.join("Example.app");
        let executable = app.join("Contents/MacOS/example");
        let dashboard = app.join("Contents/Applications/Dashboard.app");
        file::create_dir_all(&dashboard)?;
        file::create_dir_all(executable.parent().unwrap())?;
        file::write(&executable, "example")?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::make_symlink(&app, &version_dir.join("Example.app"))?;
        let binary = prefix::prefix().join("bin/example");
        file::create_dir_all(binary.parent().unwrap())?;
        file::make_symlink(&executable, &binary)?;
        let dashboard_target = EffectiveCaskDirs::current().appdir.join("Dashboard.app");
        file::make_symlink(&dashboard, &dashboard_target)?;

        assert!(matches!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::Installed(_)
        ));
        file::remove_all(&app)?;
        let receipt = receipt::read_cask_receipt(&caskroom_token_dir(&cask.token))?;
        let installed = cask_from_homebrew_receipt(&cask.token, &receipt);
        let artifacts = parse_cask_artifacts(&installed, false)?;
        assert_eq!(artifacts.apps.len(), 1);
        assert_eq!(artifacts.binaries.len(), 2);
        assert!(symlink_declares_target(
            &version_dir.join("Example.app"),
            &app
        ));
        assert!(symlink_declares_target(&binary, &executable));
        assert!(symlink_declares_target(&dashboard_target, &dashboard));
        assert!(surviving_cask_artifacts_are_owned(
            &installed,
            &artifacts,
            &version_dir
        )?);
        let InstalledCaskState::NeedsRepair {
            replacement_safe, ..
        } = installed_cask_state(&cask, &CaskArtifacts::default())?
        else {
            panic!("missing app must need repair");
        };
        assert!(replacement_safe);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replacement_metadata_errors_do_not_count_as_absence() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let blocking_file = tmp.path().join("not-a-directory");
        file::write(&blocking_file, "blocked")?;

        let error = replacement_target_metadata(&blocking_file.join("target"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("failed to prove whether replacement target exists"));
        assert!(replacement_target_metadata(&tmp.path().join("missing"))?.is_none());
        Ok(())
    }

    #[test]
    fn malformed_homebrew_receipt_needs_repair() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("broken-receipt", "1.0.0");
        let token_dir = caskroom_token_dir(&cask.token);
        file::create_dir_all(token_dir.join(".metadata"))?;
        file::write(token_dir.join(".metadata/INSTALL_RECEIPT.json"), "{")?;

        let InstalledCaskState::NeedsRepair {
            reason,
            replacement_safe,
            ..
        } = installed_cask_state(&cask, &CaskArtifacts::default())?
        else {
            panic!("malformed Homebrew receipt must need repair");
        };
        assert!(reason.contains("broken-receipt"));
        assert!(reason.contains("INSTALL_RECEIPT.json"));
        assert!(!replacement_safe);
        Ok(())
    }

    #[test]
    fn newer_homebrew_receipt_with_extra_key_reports_installed() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("future-receipt", "1.0.0");
        write_homebrew_cask_receipt(&cask.token, "1.0.0", |receipt| {
            receipt["homebrew_version"] = Value::String("7.0.1-3-gdeadbee".to_string());
            receipt["future_key"] = Value::Bool(true);
            receipt["uninstall_artifacts"] = Value::Array(Vec::new());
        });

        assert_eq!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::Installed("1.0.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn metadata_without_homebrew_receipt_needs_repair() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("missing-receipt", "1.0.0");
        file::create_dir_all(caskroom_token_dir(&cask.token).join(".metadata"))?;

        assert!(matches!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        Ok(())
    }

    #[test]
    fn no_metadata_or_legacy_receipt_is_absent() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("absent", "1.0.0");

        assert_eq!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::Absent
        );
        Ok(())
    }

    #[test]
    fn legacy_mise_receipt_without_fingerprints_needs_repair() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("legacy-mise", "2.0.0");
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        file::write(
            version_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&CaskReceipt {
                schema_version: 3,
                version: cask.version.clone(),
                auto_updates: false,
                metadata_only_apps: Vec::new(),
                apps: Vec::new(),
                binaries: Vec::new(),
                fonts: Vec::new(),
                manpages: Vec::new(),
                completions: Vec::new(),
                flight_directories: Vec::new(),
                generic: Vec::new(),
                pkg_ids: Vec::new(),
                targets: Vec::new(),
                prune_safe: true,
                prune_blocker: None,
            })?,
        )?;

        assert!(matches!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        Ok(())
    }

    #[test]
    fn legacy_mise_pkg_receipt_without_targets_reaches_catalog_validation() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("legacy-pkg", "2.0.0");
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        file::write(
            version_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&CaskReceipt {
                schema_version: 3,
                version: cask.version.clone(),
                auto_updates: false,
                metadata_only_apps: Vec::new(),
                apps: Vec::new(),
                binaries: Vec::new(),
                fonts: Vec::new(),
                manpages: Vec::new(),
                completions: Vec::new(),
                flight_directories: Vec::new(),
                generic: Vec::new(),
                pkg_ids: vec!["com.example.pkg".to_string()],
                targets: Vec::new(),
                prune_safe: false,
                prune_blocker: Some("pkg artifacts require uninstall support".to_string()),
            })?,
        )?;

        let state = installed_cask_state(&cask, &CaskArtifacts::default())?;
        let InstalledCaskState::LegacyMise(receipt) = state else {
            panic!("package receipts are authoritative ownership evidence");
        };
        assert!(receipt.targets.is_empty());
        assert_eq!(receipt.pkg_ids, ["com.example.pkg"]);
        Ok(())
    }

    #[test]
    fn legacy_mise_receipt_backfills_then_is_idempotent() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("legacy-backfill", "2.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Legacy.app", {"target": "$HOMEBREW_PREFIX/Applications/Legacy.app"}]}
        ]))?;
        let artifacts = cask_artifacts(&cask)?;
        let target = app_target_path(artifacts.apps[0].target_name())?;
        file::create_dir_all(target.join("Contents"))?;
        file::write(target.join("Contents/payload"), "untouched")?;
        let payload = target.join("Contents/payload");
        let before_hash = hash::file_hash_sha256(&payload, None)?;
        let before_modified = payload.metadata()?.modified()?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        let legacy_copy = version_dir.join("Legacy.app/Contents");
        file::create_dir_all(&legacy_copy)?;
        file::write(legacy_copy.join("payload"), "untouched")?;
        write_receipt(&version_dir, &cask, &artifacts)?;

        let status = installed_cask_status(&PackageRequest {
            name: cask.token.clone(),
            version: None,
            tap_url: None,
        })?;
        let PackageState::NeedsRepair { reason, .. } = status.state else {
            panic!("legacy metadata must not count as committed native state");
        };
        assert!(reason.contains("catalog-backed validation and conversion during apply"));
        assert!(!caskroom_token_dir(&cask.token).join(".metadata").exists());

        let classified = installed_cask_state(&cask, &artifacts)?;
        assert!(matches!(classified, InstalledCaskState::LegacyMise(_)));
        assert_eq!(
            reconcile_legacy_cask(&cask, classified)?,
            InstalledCaskState::Installed(cask.version.clone())
        );
        assert!(caskroom_token_dir(&cask.token).join(".metadata").is_dir());
        assert!(version_dir.join("Legacy.app").is_symlink());
        assert!(file::same_file(&version_dir.join("Legacy.app"), &target));
        assert!(!version_dir.join(".mise-cask.toml").exists());
        assert_eq!(hash::file_hash_sha256(&payload, None)?, before_hash);
        assert_eq!(payload.metadata()?.modified()?, before_modified);
        assert_eq!(
            reconcile_legacy_cask(&cask, installed_cask_state(&cask, &artifacts)?)?,
            InstalledCaskState::Installed(cask.version.clone())
        );
        Ok(())
    }

    #[test]
    fn legacy_mise_incomplete_inventory_needs_repair_without_mutation() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let mut cask = test_cask("legacy-incomplete", "1.0.0");
        cask.artifacts = serde_json::from_value(serde_json::json!([
            {"app": ["Expected.app"]}
        ]))?;
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        let receipt = CaskReceipt {
            schema_version: 3,
            version: cask.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: Vec::new(),
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: Vec::new(),
            targets: Vec::new(),
            prune_safe: true,
            prune_blocker: None,
        };
        file::write(
            version_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;

        assert!(matches!(
            reconcile_legacy_cask(&cask, InstalledCaskState::LegacyMise(Box::new(receipt)))?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        assert!(version_dir.join(".mise-cask.toml").exists());
        assert!(!caskroom_token_dir(&cask.token).join(".metadata").exists());
        Ok(())
    }

    #[test]
    fn legacy_mise_version_drift_needs_repair_without_mutation() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let catalog = test_cask("legacy-drift", "2.0.0");
        let installed = test_cask("legacy-drift", "1.0.0");
        let version_dir = caskroom_version_dir(&installed.token, &installed.version);
        file::create_dir_all(&version_dir)?;
        let target = tmp.path().join("legacy-target");
        file::write(&target, "payload")?;
        let receipt = CaskReceipt {
            schema_version: 3,
            version: installed.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: vec![target.clone()],
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: Vec::new(),
            targets: vec![CaskTargetRecord {
                path: target,
                fingerprint: cask_target_fingerprint(tmp.path().join("legacy-target").as_path())?,
                uninstall: None,
            }],
            prune_safe: true,
            prune_blocker: None,
        };
        file::write(
            version_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;
        let state = installed_cask_state(&catalog, &CaskArtifacts::default())?;
        assert!(matches!(
            reconcile_legacy_cask(&catalog, state)?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        assert!(version_dir.join(".mise-cask.toml").exists());
        assert!(
            !caskroom_token_dir(&catalog.token)
                .join(".metadata")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn legacy_mise_fingerprint_drift_needs_repair_without_mutation() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("legacy-fingerprint", "1.0.0");
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        let target = tmp.path().join("legacy-target");
        file::write(&target, "before")?;
        let fingerprint = cask_target_fingerprint(&target)?;
        file::write(&target, "after")?;
        let receipt = CaskReceipt {
            schema_version: 3,
            version: cask.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: vec![target.clone()],
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: Vec::new(),
            targets: vec![CaskTargetRecord {
                path: target,
                fingerprint,
                uninstall: None,
            }],
            prune_safe: true,
            prune_blocker: None,
        };
        file::write(
            version_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;
        let state = installed_cask_state(&cask, &CaskArtifacts::default())?;
        assert!(matches!(
            reconcile_legacy_cask(&cask, state)?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        assert!(version_dir.join(".mise-cask.toml").exists());
        assert!(!caskroom_token_dir(&cask.token).join(".metadata").exists());
        Ok(())
    }

    #[test]
    fn malformed_legacy_mise_receipt_needs_repair() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("legacy-malformed", "1.0.0");
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        file::write(version_dir.join(".mise-cask.toml"), "not = [toml")?;
        assert!(matches!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        assert!(!caskroom_token_dir(&cask.token).join(".metadata").exists());
        Ok(())
    }

    #[test]
    fn offline_status_preserves_legacy_state_as_needs_repair() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let version_dir = caskroom_version_dir("legacy-offline", "1.0.0");
        file::create_dir_all(&version_dir)?;
        file::write(version_dir.join(".mise-cask.toml"), "version = \"1.0.0\"")?;
        let request = PackageRequest {
            name: "legacy-offline".to_string(),
            version: None,
            tap_url: None,
        };
        let status = installed_cask_status(&request)?;
        let PackageState::NeedsRepair { reason, .. } = status.state else {
            panic!("legacy receipt must not count as committed native state");
        };
        assert!(reason.contains("legacy mise"));
        assert!(version_dir.join(".mise-cask.toml").exists());
        Ok(())
    }

    #[test]
    fn offline_status_reads_native_receipt_without_catalog_metadata() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        write_homebrew_cask_receipt("native-offline", "1.0.0", |receipt| {
            receipt["uninstall_artifacts"] = Value::Array(Vec::new());
        });
        let request = PackageRequest {
            name: "homebrew/cask/native-offline".to_string(),
            version: None,
            tap_url: None,
        };

        let status = installed_cask_status(&request)?;
        assert!(matches!(
            status.state,
            PackageState::Installed { version } if version == "1.0.0"
        ));
        Ok(())
    }

    #[test]
    fn offline_status_reports_absent_cask_as_missing_without_catalog_metadata() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let request = PackageRequest {
            name: "absent-offline".to_string(),
            version: None,
            tap_url: None,
        };

        let status = installed_cask_status(&request)?;
        assert!(matches!(status.state, PackageState::Missing));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn legacy_mise_missing_pkg_receipt_needs_repair_without_mutation() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("legacy-pkg", "1.0.0");
        let version_dir = caskroom_version_dir(&cask.token, &cask.version);
        file::create_dir_all(&version_dir)?;
        let target = tmp.path().join("legacy-target");
        file::write(&target, "payload")?;
        let receipt = CaskReceipt {
            schema_version: 3,
            version: cask.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: vec![target.clone()],
            fonts: Vec::new(),
            manpages: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: vec!["dev.mise.certainly-not-installed".to_string()],
            targets: vec![CaskTargetRecord {
                fingerprint: cask_target_fingerprint(&target)?,
                path: target,
                uninstall: None,
            }],
            prune_safe: false,
            prune_blocker: Some("pkg".to_string()),
        };
        file::write(
            version_dir.join(".mise-cask.toml"),
            toml::to_string_pretty(&receipt)?,
        )?;
        let state = installed_cask_state(&cask, &CaskArtifacts::default())?;
        assert!(matches!(
            reconcile_legacy_cask(&cask, state)?,
            InstalledCaskState::NeedsRepair { .. }
        ));
        assert!(version_dir.join(".mise-cask.toml").exists());
        assert!(!caskroom_token_dir(&cask.token).join(".metadata").exists());
        Ok(())
    }

    #[test]
    fn installed_version_ignores_homebrew_metadata() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
    fn pending_transaction_is_needs_repair_with_exact_phase() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let state_dir = tmp.path().join("state");
        let cask = test_cask("interrupted", "1.0.0");
        let journal = CaskTransactionJournal {
            schema_version: 2,
            token: cask.token.clone(),
            version: cask.version.clone(),
            phase: CaskTransactionPhase::RunningExternalAction {
                action: "predecessor_uninstall[0]".to_string(),
            },
            recovery: CaskRecoveryMode::Manual,
            receipt_inventory_targets: Vec::new(),
            activation_targets: Vec::new(),
            predecessor_targets: Vec::new(),
            had_predecessor_metadata: false,
            reopen_bundle_ids: Vec::new(),
            completed: Vec::new(),
        };
        write_cask_journal_in(&state_dir, &journal)?;

        let state = installed_cask_state_in(&cask, &CaskArtifacts::default(), &state_dir)?;
        let InstalledCaskState::NeedsRepair { reason, .. } = state else {
            panic!("pending transaction was not reported as NeedsRepair");
        };
        assert!(reason.contains("RunningExternalAction"));
        assert!(reason.contains("Manual"));
        assert!(cask_journal_pending_in(&state_dir, &cask.token));
        Ok(())
    }

    #[test]
    fn empty_legacy_transaction_journal_is_safe_to_discard() -> Result<()> {
        let body = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "token": "example",
            "version": "1.0.0",
            "completed": [],
        }))?;

        let journal = parse_cask_transaction_journal(&body)?;

        assert_eq!(journal.schema_version, 2);
        assert_eq!(journal.phase, CaskTransactionPhase::Prepared);
        assert_eq!(journal.recovery, CaskRecoveryMode::DiscardStaging);
        assert!(journal.completed.is_empty());
        Ok(())
    }

    #[test]
    fn completed_legacy_transaction_journal_requires_manual_recovery() -> Result<()> {
        let body = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "token": "example",
            "version": "1.0.0",
            "completed": ["pkg[0]"],
        }))?;

        let journal = parse_cask_transaction_journal(&body)?;

        assert_eq!(
            journal.phase,
            CaskTransactionPhase::RunningExternalAction {
                action: "legacy_v1_completed:pkg[0]".to_string(),
            }
        );
        assert_eq!(journal.recovery, CaskRecoveryMode::Manual);
        assert_eq!(journal.completed, ["pkg[0]"]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn homebrew_compatible_cask_lock_uses_exact_path_and_contends() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let path = tmp.path().join("var/homebrew/locks/example.cask.lock");

        let first = lock_cask("example")?;
        assert!(path.is_file());
        let err = lock_cask("example").unwrap_err().to_string();
        assert!(err.contains("another Homebrew-compatible operation"));
        drop(first);
        let _second = lock_cask("example")?;
        assert!(path.is_file());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn homebrew_cask_locks_allow_different_tokens_and_sort_batches() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());

        let alpha = lock_cask("alpha")?;
        let beta = lock_cask("beta")?;
        assert!(homebrew_cask_lock_path("alpha")?.is_file());
        assert!(homebrew_cask_lock_path("beta")?.is_file());
        drop((alpha, beta));

        let locks = lock_casks(["beta", "alpha", "beta"])?;
        assert_eq!(locks.len(), 2);
        drop(locks);
        let inverse = lock_casks(["alpha", "beta"])?;
        assert_eq!(inverse.len(), 2);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cask_lock_process_helper() -> Result<()> {
        let Ok(ready) = std::env::var("MISE_CASK_LOCK_HELPER_READY") else {
            return Ok(());
        };
        let release = PathBuf::from(std::env::var("MISE_CASK_LOCK_HELPER_RELEASE")?);
        let _lock = lock_cask("process-contention")?;
        file::write(&ready, "ready")?;
        for _ in 0..200 {
            if release.is_file() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        bail!("parent did not release cask lock helper")
    }

    #[cfg(unix)]
    #[test]
    fn homebrew_cask_lock_contends_across_processes() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let parent_cwd = std::env::current_dir()?;
        let ready = tmp.path().join("child-ready");
        let release = tmp.path().join("child-release");
        let test_name = "system::packages::brew::cask::tests::cask_lock_process_helper";
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", test_name, "--nocapture"])
            .env("MISE_SYSTEM_BREW_PREFIX", tmp.path())
            .env("MISE_CASK_LOCK_HELPER_READY", &ready)
            .env("MISE_CASK_LOCK_HELPER_RELEASE", &release)
            .env("MISE_TEST_PRESERVE_FIXTURE", "1")
            .spawn()?;
        for _ in 0..200 {
            if ready.is_file() {
                break;
            }
            if child.try_wait()?.is_some() {
                bail!("cask lock helper exited before acquiring the lock");
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        if !ready.is_file() {
            let _ = child.kill();
            bail!("cask lock helper did not acquire the lock");
        }

        let err = lock_cask("process-contention").unwrap_err().to_string();
        assert!(err.contains("another Homebrew-compatible operation"));
        file::write(&release, "release")?;
        assert!(child.wait()?.success());
        assert_eq!(std::env::current_dir()?, parent_cwd);
        let _released = lock_cask("process-contention")?;
        Ok(())
    }

    #[test]
    fn read_only_cask_status_creates_no_homebrew_lock_state() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let cask = test_cask("status-only", "1.0.0");

        assert_eq!(
            installed_cask_state(&cask, &CaskArtifacts::default())?,
            InstalledCaskState::Absent
        );
        assert!(!tmp.path().join("var/homebrew/locks").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cask_lock_rejects_symlinked_lock_directory() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let foreign = tmp.path().join("foreign-locks");
        file::create_dir_all(&foreign)?;
        file::create_dir_all(tmp.path().join("var/homebrew"))?;
        file::make_symlink(&foreign, &tmp.path().join("var/homebrew/locks"))?;

        let err = lock_cask("example").unwrap_err().to_string();
        assert!(err.contains("refusing symlinked Homebrew lock directory"));
        assert!(foreign.read_dir()?.next().is_none());
        Ok(())
    }

    #[test]
    fn installed_versions_preserve_conflict_presence_with_multiple_versions() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let token_dir = caskroom_token_dir("conflicting-cask");
        file::create_dir_all(token_dir.join("1.0.0"))?;
        file::create_dir_all(token_dir.join("2.0.0"))?;

        assert_eq!(installed_version("conflicting-cask"), None);
        assert_eq!(installed_versions("conflicting-cask").len(), 2);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn failed_activation_restores_caskroom_and_external_links() -> Result<()> {
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let predecessor = CaskTargetRecord {
            path: target.clone(),
            fingerprint: cask_target_fingerprint(&target)?,
            uninstall: None,
        };
        let mut link_transaction = ArtifactLinkTransaction::begin(
            vec![target.clone(), new_target.clone()],
            &[predecessor],
        )?;

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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
        let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
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
            depends_on: CaskDependencies::default(),
            conflicts_with: CaskConflicts::default(),
            ruby_source_path: None,
            ruby_source_checksum: None,
            tap_git_head: None,
            tap: Some("homebrew/cask".to_string()),
            auto_updates: false,
            raw_base: None,
            definition_source: "file:///font-test.json".to_string(),
            loaded_from_internal_api: false,
            platform_policy: CaskPlatformPolicy::Unspecified,
            resolved_formula_dependencies: Vec::new(),
            resolved_cask_dependencies: Vec::new(),
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

    #[test]
    fn public_catalog_merges_host_variation_then_requires_exact_membership() -> Result<()> {
        let raw = serde_json::json!({
            "token": "example",
            "version": "1.0.0",
            "url": "https://example.com/base.zip",
            "sha256": "base",
            "artifacts": [{"app": ["Example.app"]}],
            "supported_platforms": ["arm64_golden_gate"],
            "variations": {
                "arm64_golden_gate": {
                    "url": "https://example.com/golden.zip",
                    "sha256": "golden"
                }
            }
        });
        let cask = parse_public_cask_metadata(&raw.to_string(), "arm64_golden_gate")?;
        assert_eq!(cask.url, "https://example.com/golden.zip");
        assert_eq!(cask.sha256.as_deref(), Some("golden"));
        assert!(platform_policy_supports(
            &cask.platform_policy,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Arm64,
            Some(27),
            "arm64_golden_gate"
        ));
        assert!(!platform_policy_supports(
            &cask.platform_policy,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Intel,
            Some(27),
            "golden_gate"
        ));
        Ok(())
    }

    #[test]
    fn public_catalog_rejects_unknown_platform_metadata() {
        let unknown_supported = serde_json::json!({
            "token": "example", "version": "1", "url": "https://example.com/a.zip",
            "supported_platforms": ["future_os"], "variations": {}
        });
        assert!(parse_public_cask_metadata(&unknown_supported.to_string(), "arm64_tahoe").is_err());
        let unknown_variation = serde_json::json!({
            "token": "example", "version": "1", "url": "https://example.com/a.zip",
            "supported_platforms": ["arm64_tahoe"],
            "variations": {"future_os": {"url": "https://example.com/future.zip"}}
        });
        assert!(parse_public_cask_metadata(&unknown_variation.to_string(), "arm64_tahoe").is_err());
    }

    #[test]
    fn public_catalog_accepts_exact_homebrew_6_0_18_platform_universe() -> Result<()> {
        let platforms = [
            "arm64_golden_gate",
            "golden_gate",
            "arm64_tahoe",
            "tahoe",
            "arm64_sequoia",
            "sequoia",
            "arm64_sonoma",
            "sonoma",
            "arm64_ventura",
            "ventura",
            "arm64_monterey",
            "monterey",
            "arm64_big_sur",
            "big_sur",
            "catalina",
        ];
        let raw = serde_json::json!({
            "token": "example", "version": "1", "url": "https://example.com/a.zip",
            "supported_platforms": platforms,
            "variations": platforms.into_iter().map(|tag| (tag.to_string(), serde_json::json!({}))).collect::<serde_json::Map<_, _>>()
        });
        parse_public_cask_metadata(&raw.to_string(), "arm64_sequoia")?;
        Ok(())
    }

    #[test]
    fn unsupported_architecture_rejects_every_cask_policy() {
        for policy in [
            CaskPlatformPolicy::Unspecified,
            CaskPlatformPolicy::PublicSupported(BTreeSet::from(["all".to_string()])),
            CaskPlatformPolicy::Internal(CaskPlatformRequirements::default()),
        ] {
            assert!(!platform_policy_supports(
                &policy,
                super::super::tag::OperatingSystem::Linux,
                super::super::tag::Architecture::Unsupported,
                None,
                "all"
            ));
        }
    }

    #[test]
    fn internal_catalog_enforces_os_arch_and_macos_bounds() -> Result<()> {
        let (dependencies, policy) = parse_internal_cask_dependencies(&serde_json::json!({
            ":formula": ["helper"],
            ":cask": ["companion"],
            ":macos": ":sequoia",
            ":maximum_macos": ":tahoe",
            ":arch": ":arm64"
        }))?;
        assert_eq!(dependencies.formula, ["helper"]);
        assert_eq!(dependencies.cask, ["companion"]);
        assert!(platform_policy_supports(
            &policy,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Arm64,
            Some(15),
            "arm64_sequoia"
        ));
        assert!(!platform_policy_supports(
            &policy,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Arm64,
            Some(27),
            "arm64_golden_gate"
        ));
        assert!(!platform_policy_supports(
            &policy,
            super::super::tag::OperatingSystem::Linux,
            super::super::tag::Architecture::Arm64,
            None,
            "arm64_linux"
        ));

        let (_, exact) = parse_internal_cask_dependencies(&serde_json::json!({
            ":macos": [":sonoma", ":tahoe"]
        }))?;
        assert!(platform_policy_supports(
            &exact,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Intel,
            Some(26),
            "tahoe"
        ));
        assert!(!platform_policy_supports(
            &exact,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Intel,
            Some(15),
            "sequoia"
        ));

        let (_, maximum_only) = parse_internal_cask_dependencies(&serde_json::json!({
            ":maximum_macos": ":sonoma"
        }))?;
        assert!(platform_policy_supports(
            &maximum_only,
            super::super::tag::OperatingSystem::Macos,
            super::super::tag::Architecture::Intel,
            Some(14),
            "sonoma"
        ));
        assert!(!platform_policy_supports(
            &maximum_only,
            super::super::tag::OperatingSystem::Linux,
            super::super::tag::Architecture::Intel,
            None,
            "x86_64_linux"
        ));
        Ok(())
    }

    #[test]
    fn hiddenbar_like_macos_requirement_rejects_before_mutation() -> Result<()> {
        let (_, policy) = parse_internal_cask_dependencies(&serde_json::json!({
            ":macos": ":any"
        }))?;
        let mut cask = test_cask("hiddenbar", "1.10");
        cask.platform_policy = policy;
        cask.artifacts = vec![serde_json::json!({"app": "Hidden Bar.app"})];
        let artifacts = cask_artifacts(&cask)?;
        let tmp = tempfile::tempdir()?;
        let sentinel = tmp.path().join("unchanged");
        file::write(&sentinel, "before")?;

        let supported = validate_platform_support(&cask, &artifacts);
        if cfg!(target_os = "macos") {
            supported?;
        } else {
            assert!(supported.is_err());
        }
        assert_eq!(file::read_to_string(sentinel)?, "before");
        Ok(())
    }

    #[test]
    fn internal_catalog_rejects_unknown_or_conflicting_platform_inputs() {
        for invalid in [
            serde_json::json!({":platform": ":macos"}),
            serde_json::json!({":arch": ":riscv64"}),
            serde_json::json!({":macos": ":future_os"}),
            serde_json::json!({":linux": ":sometimes"}),
            serde_json::json!({":macos": ":any", ":linux": ":any"}),
        ] {
            assert!(
                parse_internal_cask_dependencies(&invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }
}
