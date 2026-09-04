use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
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
use crate::system::ManagerPackageOptions;
use crate::system::packages::{
    InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager,
};
use crate::system::sudo;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressIcon, SingleReport};

mod artifacts;
mod fetch;
mod flight;
mod model;
mod paths;
mod state;

use artifacts::*;
use fetch::*;
use flight::*;
pub(super) use model::Cask;
use paths::*;
use state::*;
pub(crate) use state::{apply_cask_prune_plan, cask_formula_dependencies, cask_prune_plan};

const API_BASE: &str = "https://formulae.brew.sh/api";
const HOMEBREW_CASK_RAW: &str = "https://raw.githubusercontent.com/Homebrew/homebrew-cask";
const CASK_SHIM_RB: &str = include_str!("../cask_shim.rb");
/// where `app` artifacts are linked when [`APP_DIR_ENV`] is unset
const DEFAULT_APP_DIR: &str = "/Applications";
/// user-facing override for the `app` artifact destination, mirroring
/// `brew install --appdir`; see [`target_app_dir`] and
/// docs/bootstrap/packages/brew.md
const APP_DIR_ENV: &str = "MISE_BREW_CASK_OPT_APPDIR";
const MAX_NESTED_CASK_ARCHIVES: usize = 16;

pub(crate) struct BrewCaskManager {}

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
    completions: Vec<CompletionArtifact>,
    generated_completions: Vec<GeneratedCompletionArtifact>,
    preflight_steps: Vec<FlightStep>,
    postflight_steps: Vec<FlightStep>,
    pkg_ids: Vec<String>,
}

impl CaskArtifacts {
    fn print_install_plan(&self, cask: &Cask) -> Result<()> {
        miseprintln!("install cask {}/{}", cask.token, cask.version);
        for app in &self.apps {
            miseprintln!("link app {}", app.target_name());
        }
        for binary in &self.binaries {
            miseprintln!("link binary {}", binary.target_name()?);
        }
        for wrapper in &self.command_wrappers {
            miseprintln!("link command wrapper {}", wrapper.target_name()?);
        }
        for pkg in &self.pkgs {
            miseprintln!("install pkg {}", pkg.source);
        }
        for installer in &self.installers {
            miseprintln!("run installer {}", installer.executable);
        }
        for artifact in &self.generic {
            miseprintln!("install artifact {}", artifact.target);
        }
        for font in &self.fonts {
            miseprintln!("install font {}", font.source);
        }
        for completion in &self.completions {
            miseprintln!(
                "install {} completion {}",
                completion.shell.name(),
                completion.source
            );
        }
        for generated in &self.generated_completions {
            miseprintln!("generate completions from {}", generated.executable);
        }
        Ok(())
    }

    fn app_target_paths(&self) -> Result<Vec<PathBuf>> {
        self.apps
            .iter()
            .map(|app| app_target_path(app.target_name()))
            .collect()
    }

    fn binary_targets(&self) -> Result<Vec<PathBuf>> {
        let appdir = cask_appdir(&self.apps)?;
        self.binaries
            .iter()
            .map(|binary| binary.target_path(&appdir))
            .chain(
                self.command_wrappers
                    .iter()
                    .map(CommandWrapperArtifact::target_path),
            )
            .collect()
    }

    fn font_target_paths(&self) -> Result<Vec<PathBuf>> {
        self.fonts.iter().map(font_target_path).collect()
    }

    fn completion_target_paths(&self, cask: &Cask) -> Result<Vec<PathBuf>> {
        let mut targets = self
            .completions
            .iter()
            .map(CompletionArtifact::target_path)
            .collect::<Result<Vec<_>>>()?;
        for generated in &self.generated_completions {
            targets.extend(generated.target_paths(cask)?);
        }
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    fn generic_artifact_targets(&self) -> Result<Vec<PathBuf>> {
        self.generic
            .iter()
            .map(|artifact| generic_artifact_target_path(&artifact.target))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct CaskReceipt {
    #[serde(default)]
    schema_version: u8,
    version: String,
    /// Self-updating apps are allowed to drift from the downloaded cask. For
    /// these casks the receipt, rather than a stale Caskroom app copy, is the
    /// durable ownership record.
    #[serde(default)]
    auto_updates: bool,
    /// App bundles owned through metadata only, without a duplicate in the
    /// versioned Caskroom directory (self-updating or adopted apps).
    #[serde(default)]
    metadata_only_apps: Vec<PathBuf>,
    #[serde(default)]
    apps: Vec<PathBuf>,
    #[serde(default)]
    binaries: Vec<PathBuf>,
    #[serde(default)]
    fonts: Vec<PathBuf>,
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

impl CaskReceipt {
    /// Targets owned through the standard artifact stanzas.
    fn standard_targets(&self) -> impl Iterator<Item = &PathBuf> {
        self.apps
            .iter()
            .chain(&self.binaries)
            .chain(&self.fonts)
            .chain(&self.completions)
    }
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
pub(crate) struct CaskPruneCandidate {
    pub token: String,
    pub version: String,
    version_dir: PathBuf,
    receipt: CaskReceipt,
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

#[derive(Debug, Default)]
struct CaskDependencyClosure {
    casks: BTreeMap<(String, Option<String>), String>,
    formulae: BTreeMap<(String, Option<String>), PackageRequest>,
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
                self.install_one(pkg, opts, None, manager_options).await?;
            }
            return Ok(());
        }
        let mpr = MultiProgressReport::get();
        mpr.init_footer(false, "install", pkgs.len());
        for pkg in pkgs {
            let pr: Box<dyn SingleReport> = mpr.add(&format!("brew-cask:{}", pkg.name));
            match self
                .install_one(pkg, opts, Some(&*pr), manager_options)
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
        manager_options: &ManagerPackageOptions,
    ) -> Result<String> {
        self.install_one_with_ancestors(req, opts, pr, &BTreeSet::new(), manager_options)
            .await
    }

    async fn install_one_with_ancestors(
        &self,
        req: &PackageRequest,
        opts: &InstallOpts,
        pr: Option<&dyn SingleReport>,
        ancestors: &BTreeSet<String>,
        manager_options: &ManagerPackageOptions,
    ) -> Result<String> {
        let cask = fetch_cask(req, !opts.dry_run).await?;
        if ancestors.contains(&cask.token) {
            bail!("brew-cask:{}: dependency cycle detected", cask.token);
        }
        let mut ancestors = ancestors.clone();
        ancestors.insert(cask.token.clone());
        if let Some(version) = homebrew_installed_version(&cask.token)? {
            info!(
                "brew-cask:{}: installed and managed by Homebrew; leaving unchanged",
                cask.token
            );
            return Ok(version);
        }
        let artifacts = cask_artifacts(&cask)?;
        validate_platform_support(&cask, &artifacts)?;
        let installed_version = mise_installed_cask_version(&cask)?;
        if let Some(version) = installed_version.as_ref()
            && (cask.auto_updates || version == &cask.version)
        {
            info!("brew-cask:{}: already installed", cask.token);
            return Ok(version.clone());
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
        if !cask.depends_on.formula.is_empty() {
            let dependencies = cask
                .depends_on
                .formula
                .iter()
                .map(|name| PackageRequest {
                    name: name.clone(),
                    version: None,
                    tap_url: None,
                    desired: super::super::PackageDesiredState::Present,
                })
                .collect::<Vec<_>>();
            super::BrewManager::new()
                .install(&dependencies, opts)
                .await?;
        }
        for dependency in &cask.depends_on.cask {
            let request = PackageRequest {
                name: dependency.clone(),
                version: None,
                tap_url: None,
                desired: super::super::PackageDesiredState::Present,
            };
            Box::pin(self.install_one_with_ancestors(
                &request,
                opts,
                None,
                &ancestors,
                manager_options,
            ))
            .await?;
        }
        if opts.dry_run {
            artifacts.print_install_plan(&cask)?;
            return Ok(cask.version);
        }
        prefix::bootstrap(false)?;
        let stage = fetch_and_stage(&cask, pr).await?;
        let adopt = manager_options.brew_cask_adopt(&cask.token) && installed_version.is_none();
        if adopt && !cask.auto_updates {
            validate_adoptable_apps(&stage, &artifacts.apps)?;
        }
        let _caskroom_lock = lock_caskroom()?;
        recover_flight_backups()?;
        ensure_homebrew_did_not_take_ownership(&cask.token, &stage)?;
        if let Some(version) = mise_installed_cask_version(&cask)?
            && (cask.auto_updates || version == cask.version)
        {
            file::remove_all(stage)?;
            return Ok(version);
        }
        let previous_binaries = previous_binary_targets(&cask)?;
        let previous_fonts = previous_font_targets(&cask)?;
        let previous_completions = previous_completion_targets(&cask)?;
        let previous_flight_symlinks = previous_flight_symlink_targets(&cask)?;
        let previous_flight_directories = previous_flight_directory_targets(&cask)?;
        let previous_generic = previous_generic_targets(&cask)?;
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
        let mut flight_targets = FlightTargetTransaction::default();
        flight_targets.receipt_caskroom = Some(caskroom.clone());
        flight_targets.previous_symlinks = previous_flight_symlinks.iter().cloned().collect();
        flight_targets.previous_directories = previous_flight_directories.into_iter().collect();
        write_cask_journal(&journal)?;
        let current_completions = artifacts.completion_target_paths(&cask)?;
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
            &mut flight_targets,
        )?;
        execute_lifecycle_hook(&cask, &stage, &appdir, "preflight", pr).await?;
        if has_lifecycle_hook(&cask, "preflight") {
            record_cask_action(&mut journal, "preflight_hook")?;
        }
        // Homebrew leaves artifacts from the installed version available to
        // preflight. Back them up only after preflight so guards and commands
        // can observe those links during an upgrade. A structured preflight
        // step that replaces one has already protected it transactionally.
        for target in &previous_flight_symlinks {
            flight_targets.protect(target)?;
        }
        run_installers_before_durabilizing(
            &stage,
            &tmp_caskroom,
            &artifacts.installers,
            &mut flight_targets,
            |index| record_cask_action(&mut journal, &format!("installer[{index}]")),
        )?;
        let mut metadata_only_apps = Vec::new();
        for (index, app) in artifacts.apps.iter().enumerate() {
            if install_app(
                &stage,
                &tmp_caskroom,
                app,
                !cask.auto_updates,
                adopt,
                !cask.auto_updates,
            )? {
                metadata_only_apps.push(app_target_path(app.target_name())?);
            }
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
        for (index, artifact) in artifacts.generic.iter().enumerate() {
            install_generic_artifact(&stage, &tmp_caskroom, artifact, &mut flight_targets)?;
            record_cask_action(&mut journal, &format!("artifact[{index}]"))?;
        }
        execute_flight_steps_recording(
            &cask,
            &artifacts.postflight_steps,
            &tmp_caskroom,
            &appdir,
            "postflight_steps",
            &mut journal,
            &mut flight_targets,
        )?;
        execute_lifecycle_hook(&cask, &tmp_caskroom, &appdir, "postflight", pr).await?;
        if has_lifecycle_hook(&cask, "postflight") {
            record_cask_action(&mut journal, "postflight_hook")?;
        }
        if artifacts
            .binaries
            .iter()
            .any(|binary| payload_backed_binary(&stage, binary))
        {
            durabilize_stage_payload(&stage, &tmp_caskroom, &artifacts.apps)?;
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
        let current_binaries = artifacts.binary_targets()?;
        let current_fonts = artifacts.font_target_paths()?;
        let mut current_targets = current_binaries.clone();
        current_targets.extend(current_completions.iter().cloned());
        current_targets.extend(current_fonts.iter().cloned());
        let mut link_transaction = ArtifactLinkTransaction::begin(current_targets)?;
        let activation = replace_caskroom(&cask, &tmp_caskroom, &caskroom, || {
            retarget_transient_symlinks(&tmp_caskroom, &caskroom, &caskroom, &flight_targets)?;
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
            write_receipt_with_flight_targets(
                &caskroom,
                &cask,
                &artifacts,
                flight_targets.installed_targets(),
                flight_targets.uninstall_targets(),
                flight_targets.installed_directories(),
                &metadata_only_apps,
            )?;
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
        if let Err(err) = flight_targets.commit() {
            warn!("brew-cask: failed to remove flight target backups: {err:#}");
        }
        if let Err(err) = link_transaction.commit() {
            warn!("brew-cask: failed to remove artifact link backups: {err:#}");
        }
        record_cask_action(&mut journal, "activated")?;
        remove_obsolete_binary_links(&cask, &previous_binaries, &current_binaries)?;
        remove_obsolete_completions(&cask, &previous_completions, &current_completions)?;
        remove_obsolete_fonts(&cask, &previous_fonts, &current_fonts)?;
        remove_obsolete_generic_artifacts(
            &previous_generic,
            &artifacts.generic_artifact_targets()?,
        )?;
        remove_obsolete_flight_directories(
            &flight_targets.previous_directories,
            flight_targets.installed_directories(),
        )?;
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
            None => Ok(file_name_str(Path::new(&self.source), "binary source")?.to_string()),
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
            None => Ok(file_name_str(Path::new(&self.source), "completion source")?.to_string()),
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
            let cask = fetch_cask(req, false).await?;
            statuses.push(PackageStatus {
                request: req.clone(),
                state: package_state(req, &cask)?,
            });
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
        self.install(pkgs, opts).await
    }
}

fn install_app(
    stage: &Path,
    caskroom: &Path,
    app: &AppArtifact,
    keep_caskroom_copy: bool,
    adopt: bool,
    verify_adopt: bool,
) -> Result<bool> {
    let source = find_app(stage, &app.source)
        .ok_or_else(|| eyre!("brew-cask: app artifact '{}' was not found", app.source))?;
    let caskroom_app = caskroom.join(app_bundle_name(app.target_name())?);
    file::remove_all(&caskroom_app)?;
    let logical_target = app_target_path(app.target_name())?;
    // Hold the verified appdir open for the whole mutation and address the app
    // only by name relative to that descriptor. Nothing below resolves a
    // pathname for the application directory, so a post-validation replacement
    // of any appdir component — even by the same uid — cannot redirect the
    // copy, rename, removal, permission repair, or quarantine steps.
    let parent = ensure_trusted_appdir(
        logical_target
            .parent()
            .ok_or_else(|| eyre!("brew-cask: app target has no parent directory"))?,
    )?;
    let name = logical_target
        .file_name()
        .ok_or_else(|| eyre!("brew-cask: app target has no filename"))?
        .to_owned();
    if adopt && exists_at(&parent.fd, &name)? {
        if verify_adopt {
            let source_fingerprint = cask_target_fingerprint(&source)?;
            let target_fingerprint = cask_target_fingerprint(&logical_target)?;
            if source_fingerprint != target_fingerprint {
                bail!(
                    "brew-cask: cannot adopt '{}': existing artifact is not identical to the cask artifact",
                    logical_target.display()
                );
            }
        }
        return Ok(true);
    }

    ditto(&source, &caskroom_app)?;
    // Suffix hashes stay derived from the logical path so temporary and backup
    // names are stable across runs.
    let name_hash = crate::hash::hash_to_str(&logical_target.display().to_string());
    let tmp_name = replace_bundle_extension(&name, &format!("mise-tmp-{name_hash}"));
    let old_name = replace_bundle_extension(&name, &format!("mise-old-{name_hash}"));
    remove_all_at(&parent.fd, &tmp_name)?;
    ditto_into(&caskroom_app, &parent.fd, &tmp_name)?;
    activate_app_at(
        &parent,
        &name,
        &tmp_name,
        &old_name,
        &caskroom_app,
        &logical_target,
    )?;
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
    Ok(!keep_caskroom_copy)
}

fn validate_adoptable_apps(stage: &Path, apps: &[AppArtifact]) -> Result<()> {
    for app in apps {
        let target = app_target_path(app.target_name())?;
        if target.symlink_metadata().is_err() {
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

/// Replace an app bundle's extension for a bare file name
/// (`Firefox.app` + `mise-tmp-ab12` -> `Firefox.mise-tmp-ab12`).
fn replace_bundle_extension(name: &std::ffi::OsStr, extension: &str) -> std::ffi::OsString {
    Path::new(name).with_extension(extension).into_os_string()
}

/// Activate the staged app before replacing its Caskroom copy with a symlink.
/// A failed app swap therefore leaves the durable staged copy available for
/// recovery instead of exposing a symlink to the previous app installation.
#[cfg(unix)]
fn activate_app_at(
    parent: &TrustedOperationParent,
    name: &std::ffi::OsStr,
    tmp_name: &std::ffi::OsStr,
    old_name: &std::ffi::OsStr,
    caskroom_app: &Path,
    logical_target: &Path,
) -> Result<()> {
    if exists_at(&parent.fd, name)? {
        // Same class of pain as `brew reinstall --cask`: TCC is keyed to the
        // app identity at this path, so an atomic bundle swap clears grants.
        warn!(
            "brew-cask: replacing {} — macOS may revoke Privacy & Security \
             permissions for this app (Accessibility, Screen Recording, Full \
             Disk Access, etc.); re-grant them in System Settings if prompted. \
             To take over an existing app without replacing it, set adopt = true \
             or [bootstrap.brew] adopt = true",
            logical_target.display()
        );
    }
    swap_app_at(parent, name, tmp_name, old_name)?;
    replace_caskroom_app_with_symlink(caskroom_app, logical_target)
}

#[cfg(unix)]
fn replace_caskroom_app_with_symlink(caskroom_app: &Path, target: &Path) -> Result<()> {
    let suffix = crate::hash::hash_to_str(&caskroom_app.display().to_string());
    let staged_link = caskroom_app.with_extension(format!("mise-link-{suffix}"));
    let staged_copy = caskroom_app.with_extension(format!("mise-copy-{suffix}"));
    file::remove_all(&staged_link)?;
    file::remove_all(&staged_copy)?;
    file::make_symlink(target, &staged_link)?;
    file::rename(caskroom_app, &staged_copy)?;
    if let Err(err) = file::rename(&staged_link, caskroom_app) {
        let _ = file::rename(&staged_copy, caskroom_app);
        let _ = file::remove_all(&staged_link);
        return Err(err).wrap_err("failed to replace Caskroom app copy with symlink");
    }
    if let Err(err) = file::remove_all(&staged_copy) {
        warn!(
            "brew-cask: failed to remove staged app copy {}: {err:#}",
            staged_copy.display()
        );
    }
    Ok(())
}

/// Atomically replace an app inside `parent`, restoring the previous bundle if
/// activation fails. All operations are `*at`-relative to the verified
/// descriptor, so no application directory pathname is ever re-resolved.
#[cfg(unix)]
fn swap_app_at(
    parent: &TrustedOperationParent,
    name: &std::ffi::OsStr,
    tmp_name: &std::ffi::OsStr,
    old_name: &std::ffi::OsStr,
) -> Result<()> {
    // Atomic swap: rename the existing target aside before putting the new one
    // in place so a failure leaves the old app intact rather than nothing.
    remove_app_at(parent, old_name)?;
    if exists_at(&parent.fd, name)? {
        nix::fcntl::renameat(&parent.fd, name, &parent.fd, old_name)?;
    }
    if let Err(e) = nix::fcntl::renameat(&parent.fd, tmp_name, &parent.fd, name) {
        // Restore the old app if the swap failed.
        if exists_at(&parent.fd, old_name)? {
            let _ = nix::fcntl::renameat(&parent.fd, old_name, &parent.fd, name);
        }
        return Err(e).wrap_err_with(|| {
            format!(
                "brew-cask: failed to activate {}",
                Path::new(name).display()
            )
        });
    }
    // The replacement is already live. A cleanup failure must not report the
    // install as failed or prevent removing quarantine.
    if let Err(err) = remove_app_at(parent, old_name) {
        warn!(
            "brew-cask: failed to remove old app backup {}: {err:#}",
            Path::new(old_name).display()
        );
    }
    Ok(())
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
        if from.is_dir() {
            file::create_dir_all(to)?;
            file::copy_dir_all_preserve_symlinks(from, to)
        } else {
            file::copy(from, to)
        }
    }
}

/// Collected eagerly: callers mutate the tree they just walked.
fn symlinks_under(root: &Path, min_depth: usize) -> Result<Vec<PathBuf>> {
    Ok(WalkDir::new(root)
        .follow_links(false)
        .min_depth(min_depth)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_symlink() => Some(Ok(entry.into_path())),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<std::result::Result<Vec<_>, _>>()?)
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

        for link in symlinks_under(&destination, 0)? {
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
    for target in symlinks_under(installed_caskroom, 1)? {
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
    let staging_fd = open_dir_nofollow_at(&parent.fd, staging_name.as_str())?;
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

/// Opens a directory relative to `parent`, never following symlinks.
#[cfg(unix)]
fn open_dir_nofollow_at<Fd: std::os::fd::AsFd, P: nix::NixPath + ?Sized>(
    parent: Fd,
    name: &P,
) -> Result<std::os::fd::OwnedFd> {
    Ok(nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )?)
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
        let fd = open_dir_nofollow_at(&parent, name)?;
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
        let fd = open_dir_nofollow_at(&parent, name)?;
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

/// The receipt of the currently installed version, if there is one.
fn previous_receipt(cask: &Cask) -> Result<Option<CaskReceipt>> {
    let Some(version) = installed_version(&cask.token) else {
        return Ok(None);
    };
    read_receipt(&caskroom_version_dir(&cask.token, &version))
}

fn previous_generic_targets(cask: &Cask) -> Result<Vec<CaskTargetRecord>> {
    let Some(receipt) = previous_receipt(cask)? else {
        return Ok(Vec::new());
    };
    Ok(receipt
        .targets
        .into_iter()
        .filter(|record| receipt.generic.contains(&record.path))
        .collect())
}

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
/// without ever letting the copy resolve `name` as a pathname.
///
/// `name` is created with `mkdirat` and then opened with `openat`/`O_NOFOLLOW`,
/// so the destination is guaranteed to be a real directory this call created.
/// `ditto` is then pointed at that descriptor's working directory, so it writes
/// into the bound inode rather than resolving a relative name. Without this, a
/// same-uid process could create the predictable temporary name as a symlink
/// after the preceding removal and `ditto` would follow it, copying the
/// application outside the verified application directory.
///
/// A racing creation of `name` surfaces as `EEXIST` and fails closed rather than
/// being followed.
#[cfg(unix)]
fn ditto_into<Fd: std::os::fd::AsFd>(from: &Path, dir: Fd, name: &std::ffi::OsStr) -> Result<()> {
    nix::sys::stat::mkdirat(&dir, name, nix::sys::stat::Mode::S_IRWXU).wrap_err_with(|| {
        format!(
            "brew-cask: cannot create staging directory {}",
            Path::new(name).display()
        )
    })?;
    let destination = open_dir_nofollow_at(&dir, name).wrap_err_with(|| {
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
                return Ok(file_name_str(expanded_path, "font target")?.to_string());
            }
            Ok(expanded)
        }
        None => Ok(file_name_str(Path::new(&font.source), "font source")?.to_string()),
    }
}

fn previous_font_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    Ok(previous_receipt(cask)?
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
    reject_appdir_escape(relative, "APPDIR artifact", source)?;
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
    single_match(&matches, "APPDIR artifact", source)
}

/// `kind` and `name` build the ambiguity error, e.g. "brew-cask: APPDIR
/// artifact 'x' is ambiguous: a, b".
fn single_match(matches: &[PathBuf], kind: &str, name: &str) -> Result<Option<PathBuf>> {
    match matches {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => bail!(
            "brew-cask: {kind} '{name}' is ambiguous: {}",
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
    single_match(&matches, "completion executable", executable)
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
    let filename = file_name_str(Path::new(target_name), "completion target")?;
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

fn previous_completion_targets(cask: &Cask) -> Result<Vec<PathBuf>> {
    Ok(previous_receipt(cask)?
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

/// Copy the extracted payload into the temporary caskroom, so a binary that
/// resolves its own tree from `$0` still finds that tree once staging tears the
/// stage down. Homebrew keeps the whole payload in the versioned caskroom and
/// links only the declared artifacts out of it; a cask that ships a package
/// layout — a launcher beside the helpers, resources, and manifest it execs —
/// is only installable because of that.
///
/// Entries an artifact phase already placed are left alone, and app bundles are
/// skipped entirely: `install_app` owns those, and an auto-updating cask
/// deliberately keeps no caskroom copy of its app. Entries resolving outside the
/// stage are skipped too — a preflight that installs under the prefix and leaves
/// a link behind is already durable, and `stage_binary` links into it.
fn durabilize_stage_payload(stage: &Path, caskroom: &Path, apps: &[AppArtifact]) -> Result<()> {
    let app_sources: Vec<PathBuf> = apps
        .iter()
        .filter_map(|app| find_app(stage, &app.source))
        .map(|source| file::desymlink_path(&source))
        .collect();
    for entry in std::fs::read_dir(stage)? {
        let source = entry?.path();
        if !path_starts_with_resolved_root(&source, stage) {
            continue;
        }
        let resolved = file::desymlink_path(&source);
        if app_sources.contains(&resolved) {
            continue;
        }
        let Some(relative) = staged_relative_path(stage, &source) else {
            continue;
        };
        let target = caskroom.join(&relative);
        if !path_starts_with_resolved_root(&target, caskroom) {
            bail!(
                "brew-cask: refusing to stage cask payload through a path outside the caskroom: {}",
                target.display()
            );
        }
        if target.symlink_metadata().is_ok() {
            continue;
        }
        if let Some(parent) = target.parent() {
            file::create_dir_all(parent)?;
        }
        if source.is_dir() {
            file::copy_dir_all_preserve_symlinks(&source, &target)?;
        } else {
            file::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// Whether a binary artifact takes its source from the extracted payload, and so
/// needs that payload to outlive the stage.
fn payload_backed_binary(stage: &Path, binary: &BinaryArtifact) -> bool {
    !binary.source.contains("$APPDIR")
        && find_file_artifact(stage, &binary.source)
            .is_some_and(|source| path_starts_with_resolved_root(&source, stage))
}

/// The durable payload copy for a binary whose source is stage content, if the
/// payload carries one. `find_binary_source` searches the caskroom before the
/// stage, so this reads the artifact's own source rather than that answer: the
/// point is to know the path *within the payload*, which the binary's target
/// name does not have to match.
fn payload_binary_path(stage: &Path, caskroom: &Path, binary: &BinaryArtifact) -> Option<PathBuf> {
    let source = find_file_artifact(stage, &binary.source)?;
    if !path_starts_with_resolved_root(&source, stage) {
        return None;
    }
    let payload = caskroom.join(staged_relative_path(stage, &source)?);
    (payload.is_file() && path_starts_with_resolved_root(&payload, caskroom)).then_some(payload)
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
    // The payload is durable in the caskroom by now, so link into the tree the
    // binary shipped in rather than lifting it out of the siblings it resolves.
    // A payload whose own layout already puts the binary at the target path
    // needs nothing further, and must not be removed to be re-copied onto
    // itself.
    if let Some(payload) = payload_binary_path(stage, caskroom, binary) {
        // A cask can declare a binary the payload does not ship executable, so
        // the bit is set on the payload copy itself: it is what the target
        // resolves to, whether the link points at it or the layout already put
        // it at the target path.
        file::make_executable(&payload)?;
        if payload == caskroom_binary {
            return Ok(());
        }
        file::remove_all(&caskroom_binary)?;
        if let Some(parent) = caskroom_binary.parent() {
            file::create_dir_all(parent)?;
        }
        file::make_symlink(&payload, &caskroom_binary)?;
        return Ok(());
    }
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
        // Ephemeral stage content has to be copied into the caskroom before the
        // stage is torn down; a durable location is linked instead, so a CLI
        // that derives its own root from the resolved path of `$0` still finds
        // its tree. Resolve both sides: gcloud-cli's preflight leaves
        // `staged_path/google-cloud-sdk` as a link to the SDK it installed under
        // the prefix, and a lexical comparison would read that as stage content
        // and copy the launcher out of the tree it needs.
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
    if let Some(source) = absolute_prefixed_source(&binary.source)
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

/// Rejects an empty relative path and any component that could climb out of
/// `$APPDIR` (`..`, a root, a prefix).
fn reject_appdir_escape(relative: &Path, kind: &str, name: &str) -> Result<()> {
    if relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("brew-cask: {kind} '{name}' must stay below Applications");
    }
    Ok(())
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

#[cfg(test)]
mod tests;
