//! Pour a bottle: extract -> relocate -> codesign -> receipt -> link.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::api::BottleFile;
use super::fetch::{OciBottleMetadata, VerifiedArtifact};
use super::lifecycle;
use super::prefix;
use super::relocate;
use super::resolve::ResolvedFormula;
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::result::Result;
use crate::ui::progress_report::SingleReport;

/// directories linked from a keg into the prefix (brew's Keg::KEG_LINK_DIRECTORIES,
/// minus etc/var, which the lifecycle finalizer installs with persistent-file
/// semantics instead of public keg links)
pub(super) const LINK_DIRS: &[&str] = &["bin", "sbin", "include", "lib", "share", "Frameworks"];
const KEG_ONLY_MARKER: &str = ".mise-keg-only";
const FINALIZATION_INCARNATION_MARKER: &str = ".brew/.mise-finalization-incarnation";
const EMULATED_BREW_VERSION: &str = "6.0.17";

#[cfg(test)]
struct RecordRepair {
    version: String,
    keg: PathBuf,
    destination: PathBuf,
}

#[derive(Debug)]
pub(super) enum FormulaInstallProvenance {
    OciBottle {
        tab: Value,
        sbom: Value,
        sbom_supplement: Option<Value>,
    },
    ArchiveBottle {
        tab: Value,
        sbom: Value,
    },
    SourceBuild {
        formula_snapshot: PathBuf,
        compiler: String,
        built_on: Value,
    },
}

#[derive(Clone, Debug, Deserialize)]
struct BottleFacts {
    #[serde(default)]
    changed_files: Vec<String>,
    source_modified_time: u64,
    compiler: String,
    #[serde(default)]
    runtime_dependencies: Vec<Value>,
    #[serde(default)]
    built_on: Option<Value>,
    #[serde(default)]
    poured_from_bottle: Option<bool>,
    #[serde(default)]
    source: Option<Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FinalizationPhase {
    Building,
    Receipt,
    Keg,
    Linked,
    SharedState,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FinalizationState {
    formula: String,
    version: String,
    provenance: String,
    phase: FinalizationPhase,
    #[serde(default)]
    predecessor_keg: Option<PathBuf>,
    #[serde(default)]
    replacement_identity: Option<FinalizationInstallIdentity>,
    #[serde(default)]
    predecessor_identity: Option<FinalizationInstallIdentity>,
    #[serde(default)]
    lifecycle_predecessor_identity: Option<FinalizationInstallIdentity>,
    #[serde(default)]
    receipt_identity: Option<FinalizationInstallIdentity>,
    #[serde(default)]
    receipt_current: Option<ReceiptCurrent>,
    #[serde(default)]
    build_incarnation: Option<String>,
    #[serde(default)]
    previous_finalization_state: Option<Vec<u8>>,
    #[serde(default)]
    lifecycle_identity_sha256: Option<String>,
    #[serde(default)]
    build_root_identity: Option<FinalizationPathIdentity>,
    #[serde(default)]
    quiesced_links: Vec<FinalizationLink>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FinalizationInstallIdentity {
    receipt_identity_sha256: String,
    snapshot_sha256: String,
    kind: FinalizationIdentityKind,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FinalizationIdentityKind {
    Mise { incarnation: String },
    Native { device: u64, inode: u64 },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FinalizationPathIdentity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FinalizationLink {
    path: PathBuf,
    raw_target: PathBuf,
    #[serde(default)]
    ancestors: Vec<(PathBuf, FinalizationPathIdentity)>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReceiptCurrent {
    Absent,
    Predecessor,
    Discarded,
    Replacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FormulaHealthKind {
    Healthy,
    Repairable,
    ReinstallRequired,
}

#[derive(Debug)]
pub(super) struct FormulaHealth {
    pub name: String,
    pub version: String,
    pub keg: PathBuf,
    pub kind: FormulaHealthKind,
    pub reasons: Vec<String>,
    pub(super) mise_owned: bool,
    pub(super) poured_from_bottle: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct InstalledReceipt {
    #[serde(default)]
    homebrew_version: String,
    #[serde(default)]
    poured_from_bottle: Option<bool>,
    #[serde(default)]
    runtime_dependencies: Vec<InstalledRuntimeDependency>,
}

#[derive(Debug, Deserialize)]
struct InstalledRuntimeDependency {
    full_name: String,
    pkg_version: String,
}

pub fn keg_path(name: &str, pkg_version: &str) -> PathBuf {
    prefix::cellar().join(name).join(pkg_version)
}

/// is this keg fully poured and linked? Every pour ends by creating the
/// `opt/<name>` symlink (even for keg-only formulae), so a Cellar directory
/// without it is a remnant of a failed install and must not block a retry.
#[cfg(test)]
pub fn keg_installed(name: &str, pkg_version: &str) -> bool {
    let keg = keg_path(name, pkg_version);
    keg.exists()
        && linked_version(name).as_deref() == Some(pkg_version)
        && !lifecycle::needs_repair(&keg)
        && !finalization_needs_repair(&keg)
}

/// the version `opt/<name>` points at, if the symlink resolves to an
/// existing keg
#[cfg(test)]
pub fn linked_version(name: &str) -> Option<String> {
    let opt = prefix::prefix().join("opt").join(name);
    record_keg(name, &opt).map(|(version, _)| version)
}

/// Capture the exact active predecessor before any keg or link mutation.
pub(super) fn active_keg(name: &str) -> Option<PathBuf> {
    let opt = prefix::prefix().join("opt").join(name);
    record_keg(name, &opt).map(|(_, keg)| keg)
}

fn links_resolving_into_keg(name: &str, keg: &Path) -> Result<Vec<FinalizationLink>> {
    let resolved_keg = resolved_path_checked(keg)?;
    let prefix_path = prefix::prefix();
    let mut candidates = vec![
        prefix_path.join("opt").join(name),
        prefix::linked_keg_record(name),
    ];
    for root in LINK_DIRS {
        let public_root = prefix_path.join(root);
        match metadata_if_exists(&public_root)? {
            None => continue,
            Some(_) => {
                for entry in walkdir::WalkDir::new(&public_root).follow_links(false) {
                    candidates.push(entry?.into_path());
                }
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    let mut links = vec![];
    for path in candidates {
        let Some(metadata) = metadata_if_exists(&path)? else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let Some(target) = resolved_symlink_target_checked(&path)? else {
            continue;
        };
        if target == resolved_keg || target.starts_with(&resolved_keg) {
            links.push(FinalizationLink {
                raw_target: std::fs::read_link(&path)?,
                ancestors: capture_real_topology_ancestors(&path)?,
                path,
            });
        }
    }
    Ok(links)
}

fn validate_quiesced_links(state: &FinalizationState) -> Result<()> {
    for link in &state.quiesced_links {
        validate_finalization_link(link)?;
    }
    Ok(())
}

fn validate_finalization_link(link: &FinalizationLink) -> Result<bool> {
    if !link.path.starts_with(prefix::prefix()) {
        bail!(
            "formula finalization link is outside Homebrew prefix: {}",
            link.path.display()
        );
    }
    if link.ancestors.is_empty() {
        bail!(
            "formula finalization link has no bound ancestors: {}",
            link.path.display()
        );
    }
    for (path, expected) in &link.ancestors {
        if !matches!(capture_path_identity(path), Ok(actual) if actual == *expected) {
            bail!(
                "formula finalization link ancestor changed: {}",
                path.display()
            );
        }
    }
    match metadata_if_exists(&link.path)? {
        None => Ok(false),
        Some(metadata) if metadata.file_type().is_symlink() => {
            if std::fs::read_link(&link.path)? != link.raw_target {
                bail!("formula finalization link changed: {}", link.path.display());
            }
            Ok(true)
        }
        Some(_) => bail!(
            "formula finalization link changed type: {}",
            link.path.display()
        ),
    }
}

fn finish_quiescing_links(state: &FinalizationState) -> Result<()> {
    validate_quiesced_links(state)?;
    for link in &state.quiesced_links {
        if validate_finalization_link(link)? {
            crate::file::remove_file(&link.path)?;
        }
    }
    Ok(())
}

fn quiesce_keg_links(keg: &Path, state: &mut FinalizationState) -> Result<()> {
    if state.quiesced_links.is_empty() {
        state.quiesced_links = links_resolving_into_keg(&state.formula, keg)?;
        write_finalization_state(keg, state)?;
    }
    finish_quiescing_links(state)
}

fn restore_quiesced_links(state: &FinalizationState) -> Result<()> {
    validate_quiesced_links(state)?;
    for link in &state.quiesced_links {
        if validate_finalization_link(link)? {
            continue;
        }
        let parent = link
            .path
            .parent()
            .ok_or_else(|| eyre::eyre!("formula finalization link has no parent"))?;
        require_real_directory(parent, "formula finalization link parent")?;
        crate::file::make_symlink(&link.raw_target, &link.path)?;
    }
    Ok(())
}

/// Return the active keg version and whether one of its active records can be repaired locally.
pub(super) fn linked_state(name: &str) -> Option<(String, bool)> {
    let opt = prefix::prefix().join("opt").join(name);
    let active = record_keg(name, &opt).or_else(|| {
        record_needs_replacement(name, &opt)
            .then(|| record_keg(name, &prefix::linked_keg_record(name)))?
    })?;
    let needs_repair = record_repair_needed(name)
        || lifecycle::needs_repair(&active.1)
        || finalization_needs_repair(&active.1);
    Some((active.0, needs_repair))
}

fn record_repair_needed(name: &str) -> bool {
    let opt = prefix::prefix().join("opt").join(name);
    let linked = prefix::linked_keg_record(name);
    if let Some((_, keg)) = record_keg(name, &opt) {
        return !keg_is_keg_only(name, &keg)
            && record_needs_replacement(name, &linked)
            && has_public_link_into(&keg);
    }
    record_needs_replacement(name, &opt) && record_keg(name, &linked).is_some()
}

/// Read the active root and its installed receipt dependency closure without
/// consulting remote metadata. Every reason is prefixed with the exact closure
/// node so a configured root cannot hide a broken transitive dependency.
pub(super) fn installed_closure_health(name: &str) -> Option<FormulaHealth> {
    let active = linked_state(name)?;
    let active = (active.0.clone(), keg_path(name, &active.0));
    let mut visited = BTreeSet::new();
    let mut nodes = vec![];
    collect_closure_health(name, &active.0, &active.1, &mut visited, &mut nodes);
    let mut root = nodes.remove(0);
    for node in nodes {
        if node.kind != FormulaHealthKind::Healthy {
            root.kind = max_health(root.kind, node.kind);
            root.reasons.extend(
                node.reasons
                    .into_iter()
                    .map(|reason| format!("dependency {}/{}: {reason}", node.name, node.version)),
            );
        }
    }
    Some(root)
}

fn collect_closure_health(
    name: &str,
    version: &str,
    keg: &Path,
    visited: &mut BTreeSet<(String, String)>,
    nodes: &mut Vec<FormulaHealth>,
) {
    if !visited.insert((name.to_string(), version.to_string())) {
        return;
    }
    let (health, dependencies) = formula_health(name, version, keg);
    nodes.push(health);
    for dependency in dependencies {
        let dependency_name = dependency
            .full_name
            .rsplit('/')
            .next()
            .unwrap_or(&dependency.full_name)
            .to_string();
        let opt = prefix::prefix().join("opt").join(&dependency_name);
        if let Some((active_version, active_keg)) =
            record_keg(&dependency_name, &opt).or_else(|| {
                record_needs_replacement(&dependency_name, &opt).then(|| {
                    record_keg(
                        &dependency_name,
                        &prefix::linked_keg_record(&dependency_name),
                    )
                })?
            })
        {
            collect_closure_health(
                &dependency_name,
                &active_version,
                &active_keg,
                visited,
                nodes,
            );
        } else {
            let dependency_keg = keg_path(&dependency_name, &dependency.pkg_version);
            nodes.push(FormulaHealth {
                name: dependency.full_name,
                version: dependency.pkg_version,
                keg: dependency_keg,
                kind: FormulaHealthKind::ReinstallRequired,
                reasons: vec!["runtime dependency is missing or has no active opt record".into()],
                mise_owned: false,
                poured_from_bottle: None,
            });
        }
    }
}

fn formula_health(
    name: &str,
    version: &str,
    keg: &Path,
) -> (FormulaHealth, Vec<InstalledRuntimeDependency>) {
    let mut kind = FormulaHealthKind::Healthy;
    let mut reasons = vec![];
    let metadata_is_real = lifecycle::validate_lifecycle_keg_ancestry(keg).is_ok();
    let keg_is_real = metadata_is_real;
    if !metadata_is_real {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push("formula keg or .brew metadata ancestry is not a real directory".into());
    }
    let receipt_path = keg.join("INSTALL_RECEIPT.json");
    let receipt = keg_is_real
        .then(|| read_regular_file_for_health(&receipt_path))
        .flatten()
        .and_then(|contents| serde_json::from_slice::<InstalledReceipt>(&contents).ok());
    let mise_owned = receipt
        .as_ref()
        .is_some_and(|receipt| receipt.homebrew_version.ends_with(" (mise)"));
    if receipt.is_none() {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push(format!(
            "receipt/provenance is missing or malformed: {}",
            receipt_path.display()
        ));
    }
    let snapshot = keg.join(".brew").join(format!("{name}.rb"));
    if !metadata_is_real
        || !snapshot
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push(format!(
            "formula snapshot is missing: {}",
            snapshot.display()
        ));
    }
    let sbom = keg.join("sbom.spdx.json");
    if (!keg_is_real)
        || read_regular_file_for_health(&sbom)
            .and_then(|contents| serde_json::from_slice::<Value>(&contents).ok())
            .is_none()
    {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push(format!("SBOM is missing or malformed: {}", sbom.display()));
    }

    let opt_matches = record_keg(name, &prefix::prefix().join("opt").join(name))
        .is_some_and(|active| active.0 == version);
    if !opt_matches {
        if record_needs_replacement(name, &prefix::prefix().join("opt").join(name))
            && record_keg(name, &prefix::linked_keg_record(name))
                .is_some_and(|active| active.0 == version)
        {
            kind = max_health(kind, FormulaHealthKind::Repairable);
            reasons.push("opt record is missing or dangling".into());
        } else {
            kind = FormulaHealthKind::ReinstallRequired;
            reasons.push("opt record is missing, foreign, or points at another keg".into());
        }
    }
    let keg_only = keg_is_keg_only(name, keg);
    if !keg_only {
        let linked_matches = record_keg(name, &prefix::linked_keg_record(name))
            .is_some_and(|active| active.0 == version);
        if !linked_matches {
            if record_needs_replacement(name, &prefix::linked_keg_record(name)) {
                kind = max_health(kind, FormulaHealthKind::Repairable);
                reasons.push("linked-keg record is missing or dangling".into());
            } else {
                kind = FormulaHealthKind::ReinstallRequired;
                reasons.push("linked-keg record is missing or ambiguously owned".into());
            }
        }
        match metadata_is_real.then(|| plan_public_topology(name, keg, false)) {
            None => {}
            Some(Ok(repairs)) if !repairs.is_empty() => {
                kind = max_health(kind, FormulaHealthKind::Repairable);
                reasons.push("public keg topology is incomplete".into());
            }
            Some(Err(error)) => {
                kind = FormulaHealthKind::ReinstallRequired;
                reasons.push(format!("public keg topology is ambiguous: {error}"));
            }
            Some(Ok(_)) => {}
        }
    }

    if mise_owned && finalization_needs_repair(keg) {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push("formula finalization stopped before completion".into());
    }
    let lifecycle_health = if metadata_is_real {
        lifecycle::health(keg, mise_owned)
    } else {
        lifecycle::LifecycleHealth::ReinstallRequired(vec![
            "formula lifecycle metadata ancestry is not a real directory".into(),
        ])
    };
    match lifecycle_health {
        lifecycle::LifecycleHealth::Healthy => {}
        lifecycle::LifecycleHealth::Repairable(lifecycle_reasons) => {
            kind = max_health(kind, FormulaHealthKind::Repairable);
            reasons.extend(lifecycle_reasons);
        }
        lifecycle::LifecycleHealth::ReinstallRequired(lifecycle_reasons) => {
            kind = FormulaHealthKind::ReinstallRequired;
            reasons.extend(lifecycle_reasons);
        }
    }
    (
        FormulaHealth {
            name: name.to_string(),
            version: version.to_string(),
            keg: keg.to_path_buf(),
            kind,
            reasons,
            mise_owned,
            poured_from_bottle: receipt
                .as_ref()
                .and_then(|receipt| receipt.poured_from_bottle),
        },
        receipt
            .map(|receipt| receipt.runtime_dependencies)
            .unwrap_or_default(),
    )
}

fn read_regular_file_for_health(path: &Path) -> Option<Vec<u8>> {
    let metadata = path.symlink_metadata().ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    std::fs::read(path).ok()
}

/// Read the formula snapshot from a checksum-verified bottle without trusting
/// the current tap source checksum. Homebrew bottles embed the exact formula
/// snapshot used to build them, which may legitimately differ from the current
/// source while package version and bottle rebuild remain unchanged.
pub(super) fn bottle_formula_snapshot_sha256(
    name: &str,
    pkg_version: &str,
    bottle: &VerifiedArtifact,
) -> Result<String> {
    let scratch = tempfile::tempdir()?;
    crate::file::untar_file(
        bottle.reader()?,
        bottle.label(),
        scratch.path(),
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: None,
            preserve_mtime: true,
        },
    )
    .wrap_err_with(|| format!("brew:{name}: failed to inspect verified bottle"))?;
    let bottle_name = scratch.path().join(name);
    require_direct_real_child(scratch.path(), &bottle_name, "bottle formula directory")?;
    let bottle_keg = bottle_name.join(pkg_version);
    require_direct_real_child(&bottle_name, &bottle_keg, "bottle keg directory")?;
    let metadata_dir = bottle_keg.join(".brew");
    require_direct_real_child(&bottle_keg, &metadata_dir, "bottle metadata directory")?;
    let snapshot = metadata_dir.join(format!("{name}.rb"));
    let metadata = snapshot.symlink_metadata().wrap_err_with(|| {
        format!(
            "brew:{name}: verified bottle has no formula snapshot at {name}/{pkg_version}/.brew/{name}.rb"
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("brew:{name}: verified bottle formula snapshot is not a regular file")
    }
    crate::hash::file_hash_sha256(&snapshot, None)
}

fn max_health(left: FormulaHealthKind, right: FormulaHealthKind) -> FormulaHealthKind {
    match (left, right) {
        (FormulaHealthKind::ReinstallRequired, _) | (_, FormulaHealthKind::ReinstallRequired) => {
            FormulaHealthKind::ReinstallRequired
        }
        (FormulaHealthKind::Repairable, _) | (_, FormulaHealthKind::Repairable) => {
            FormulaHealthKind::Repairable
        }
        _ => FormulaHealthKind::Healthy,
    }
}

/// mise records keg-only installs explicitly. Native Homebrew installs do not,
/// so recover the same fact offline from Homebrew's installed formula snapshot.
/// Only the top-level Formula DSL declaration is accepted; comments, nested
/// statements, and similarly named methods fail closed.
fn keg_is_keg_only(name: &str, keg: &Path) -> bool {
    keg.join(KEG_ONLY_MARKER)
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        || formula_snapshot_declares_keg_only(&keg.join(".brew").join(format!("{name}.rb")))
}

fn formula_snapshot_declares_keg_only(snapshot: &Path) -> bool {
    if !snapshot
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        return false;
    }
    let Ok(source) = std::fs::read_to_string(snapshot) else {
        return false;
    };
    source.lines().any(|line| {
        let Some(arguments) = line.strip_prefix("  keg_only") else {
            return false;
        };
        !line.starts_with("   ")
            && arguments
                .chars()
                .next()
                .is_none_or(|character| character.is_ascii_whitespace() || character == '(')
    })
}

pub(super) fn installed_formula_health(name: &str, version: &str) -> FormulaHealth {
    formula_health(name, version, &keg_path(name, version)).0
}

pub(super) async fn repair_formula(
    health: &FormulaHealth,
    lifecycle: &super::lifecycle::PreparedFormulaLifecycle,
    dry_run: bool,
) -> Result<bool> {
    if health.kind == FormulaHealthKind::Healthy {
        return Ok(false);
    }
    if health.kind == FormulaHealthKind::ReinstallRequired {
        bail!(
            "brew:{} requires reinstall: {}",
            health.name,
            health.reasons.join("; ")
        )
    }
    let topology = preflight_formula_repair(health, lifecycle)?;
    if dry_run {
        miseprintln!(
            "repair {}/{}: {}",
            health.name,
            health.version,
            health.reasons.join("; ")
        );
        return Ok(true);
    }
    apply_topology_repair(&topology)?;
    super::lifecycle::repair(lifecycle, health.mise_owned, false).await?;
    Ok(true)
}

pub(super) fn preflight_formula_repair(
    health: &FormulaHealth,
    lifecycle: &super::lifecycle::PreparedFormulaLifecycle,
) -> Result<Vec<TopologyRepairLink>> {
    if health.kind != FormulaHealthKind::Repairable {
        bail!("brew:{} is not lifecycle-repairable", health.name)
    }
    let topology = preflight_topology_repair(&health.name, &health.version, &health.keg)?;
    super::lifecycle::preflight_repair(lifecycle, health.mise_owned)?;
    Ok(topology)
}

#[derive(Debug)]
pub(super) struct TopologyRepairLink {
    destination: PathBuf,
    previous: TopologyPrevious,
    operation: TopologyOperation,
    ancestors: Vec<(PathBuf, FinalizationPathIdentity)>,
}

#[derive(Debug)]
enum TopologyPrevious {
    Absent,
    ExistingDirectory,
    Symlink(PathBuf),
}

fn topology_previous(path: &Path) -> Result<TopologyPrevious> {
    match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TopologyPrevious::Absent),
        Err(error) => Err(error.into()),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Ok(TopologyPrevious::Symlink(std::fs::read_link(path)?))
        }
        Ok(metadata) if metadata.file_type().is_dir() => Ok(TopologyPrevious::ExistingDirectory),
        Ok(_) => bail!(
            "topology destination has unsupported existing type: {}",
            path.display()
        ),
    }
}

fn validate_topology_previous(path: &Path, previous: &TopologyPrevious) -> Result<()> {
    let matches = match (previous, metadata_if_exists(path)?) {
        (TopologyPrevious::Absent, None) => true,
        (TopologyPrevious::ExistingDirectory, Some(metadata)) => {
            metadata.is_dir() && !metadata.file_type().is_symlink()
        }
        (TopologyPrevious::Symlink(expected), Some(metadata))
            if metadata.file_type().is_symlink() =>
        {
            std::fs::read_link(path)? == *expected
        }
        _ => false,
    };
    if !matches {
        bail!(
            "topology destination changed after preflight: {}",
            path.display()
        );
    }
    Ok(())
}

#[derive(Debug)]
enum TopologyOperation {
    Directory,
    Link(PathBuf),
}

fn topology_repair_link(
    destination: PathBuf,
    previous: TopologyPrevious,
    operation: TopologyOperation,
) -> Result<TopologyRepairLink> {
    let ancestors = capture_topology_ancestors(&destination)?;
    Ok(TopologyRepairLink {
        destination,
        previous,
        operation,
        ancestors,
    })
}

fn topology_ancestor_paths(destination: &Path) -> Result<Vec<PathBuf>> {
    let prefix_path = prefix::prefix();
    let parent = destination
        .parent()
        .ok_or_else(|| eyre::eyre!("topology destination has no parent"))?;
    let relative = parent.strip_prefix(&prefix_path).wrap_err_with(|| {
        format!(
            "topology destination is outside Homebrew prefix: {}",
            destination.display()
        )
    })?;
    let mut paths = vec![prefix_path.clone()];
    let mut current = prefix_path;
    for component in relative.components() {
        current.push(component);
        paths.push(current.clone());
    }
    Ok(paths)
}

fn capture_topology_ancestors(
    destination: &Path,
) -> Result<Vec<(PathBuf, FinalizationPathIdentity)>> {
    let mut identities = vec![];
    for path in topology_ancestor_paths(destination)? {
        match metadata_if_exists(&path)? {
            None => break,
            Some(metadata) if metadata.file_type().is_symlink() => break,
            Some(metadata) if metadata.is_dir() => {
                identities.push((path.clone(), capture_path_identity(&path)?));
            }
            Some(_) => bail!(
                "topology destination has non-directory ancestor: {}",
                path.display()
            ),
        }
    }
    Ok(identities)
}

fn capture_real_topology_ancestors(
    destination: &Path,
) -> Result<Vec<(PathBuf, FinalizationPathIdentity)>> {
    let mut identities = vec![];
    for path in topology_ancestor_paths(destination)? {
        let metadata = metadata_if_exists(&path)?.ok_or_else(|| {
            eyre::eyre!(
                "formula finalization link ancestor is missing: {}",
                path.display()
            )
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "formula finalization link ancestor is not a real directory: {}",
                path.display()
            );
        }
        identities.push((path.clone(), capture_path_identity(&path)?));
    }
    Ok(identities)
}

fn validate_topology_ancestors(
    repair: &TopologyRepairLink,
    runtime: &BTreeMap<PathBuf, FinalizationPathIdentity>,
) -> Result<()> {
    for (path, expected) in &repair.ancestors {
        if !matches!(capture_path_identity(path), Ok(actual) if actual == *expected) {
            bail!(
                "topology ancestor changed after preflight: {}",
                path.display()
            );
        }
    }
    for path in topology_ancestor_paths(&repair.destination)? {
        match metadata_if_exists(&path)? {
            None => break,
            Some(metadata) if metadata.file_type().is_symlink() => bail!(
                "topology ancestor became a symlink after preflight: {}",
                path.display()
            ),
            Some(metadata) if metadata.is_dir() => {
                if let Some(expected) = runtime.get(&path)
                    && capture_path_identity(&path)? != *expected
                {
                    bail!(
                        "topology ancestor changed during repair: {}",
                        path.display()
                    );
                }
            }
            Some(_) => bail!(
                "topology ancestor changed type after preflight: {}",
                path.display()
            ),
        }
    }
    Ok(())
}

fn record_topology_ancestors(
    destination: &Path,
    runtime: &mut BTreeMap<PathBuf, FinalizationPathIdentity>,
) -> Result<()> {
    for path in topology_ancestor_paths(destination)? {
        match metadata_if_exists(&path)? {
            Some(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                runtime.insert(path.clone(), capture_path_identity(&path)?);
            }
            Some(_) => bail!(
                "topology repair left an unsafe ancestor: {}",
                path.display()
            ),
            None => break,
        }
    }
    Ok(())
}

fn preflight_topology_repair(
    name: &str,
    version: &str,
    keg: &Path,
) -> Result<Vec<TopologyRepairLink>> {
    let keg_only = keg_is_keg_only(name, keg);
    let mut expected = vec![(prefix::prefix().join("opt").join(name), keg.to_path_buf())];
    if !keg_only {
        expected.push((prefix::linked_keg_record(name), keg.to_path_buf()));
    }
    let rack = prefix::cellar().join(name);
    let mut repairs = vec![];
    for (destination, target) in expected {
        if symlink_points_to_checked(&destination, &target)? {
            continue;
        }
        if let Some(ancestor) = brew_owned_ancestor(&destination)? {
            if path_matches_through_brew_owned_ancestor(&destination, &target, &ancestor)? {
                continue;
            }
            bail!(
                "brew:{name}/{version}: topology repair would traverse a directory symlink: {}",
                destination.display()
            )
        }
        let previous = match destination.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => TopologyPrevious::Absent,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let previous = std::fs::read_link(&destination)?;
                let resolved = resolved_symlink_target_checked(&destination)?
                    .ok_or_else(|| eyre::eyre!("could not resolve repair target"))?;
                if !resolved.starts_with(&rack) {
                    bail!(
                        "brew:{name}/{version}: topology target has ambiguous ownership: {}",
                        destination.display()
                    )
                }
                TopologyPrevious::Symlink(previous)
            }
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                "brew:{name}/{version}: topology target has ambiguous ownership: {}",
                destination.display()
            ),
        };
        repairs.push(topology_repair_link(
            destination,
            previous,
            TopologyOperation::Link(target),
        )?);
    }
    if !keg_only {
        repairs.extend(plan_public_topology(name, keg, false)?);
    }
    Ok(repairs)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KegLinkPolicy {
    Link,
    Mkpath,
    Skip,
    Info,
}

fn keg_link_policy(root: &str, relative: &Path, directory: bool) -> KegLinkPolicy {
    let path = relative.to_string_lossy();
    if directory && path.ends_with(".app") {
        return KegLinkPolicy::Skip;
    }
    match root {
        "bin" | "sbin" if directory => KegLinkPolicy::Skip,
        "include" if starts_with_numbered_prefix(&path, "postgresql@") => KegLinkPolicy::Mkpath,
        "share" => {
            if !directory
                && (path == "locale/locale.alias"
                    || path.strip_prefix("icons/").is_some_and(|relative| {
                        relative.contains('/') && relative.ends_with("/icon-theme.cache")
                    }))
            {
                return KegLinkPolicy::Skip;
            }
            if !directory && let Some(info_path) = info_path(&path) {
                if info_path == "dir" {
                    return KegLinkPolicy::Skip;
                }
                if !info_path.starts_with('.')
                    && (info_path.ends_with(".info") || info_path.ends_with(".info.gz"))
                {
                    return KegLinkPolicy::Info;
                }
            }
            if directory && share_path_requires_mkpath(&path) {
                return KegLinkPolicy::Mkpath;
            }
            KegLinkPolicy::Link
        }
        "lib" => {
            if !directory && path == "charset.alias" {
                KegLinkPolicy::Skip
            } else if directory && lib_path_requires_mkpath(&path) {
                KegLinkPolicy::Mkpath
            } else {
                KegLinkPolicy::Link
            }
        }
        "Frameworks"
            if directory
                && (path.ends_with(".framework") || path.ends_with(".framework/Versions")) =>
        {
            KegLinkPolicy::Mkpath
        }
        _ => KegLinkPolicy::Link,
    }
}

fn share_path_requires_mkpath(path: &str) -> bool {
    const EXACT: &[&str] = &[
        "aclocal",
        "cps",
        "doc",
        "info",
        "java",
        "locale",
        "man",
        "man/man1",
        "man/man2",
        "man/man3",
        "man/man4",
        "man/man5",
        "man/man6",
        "man/man7",
        "man/man8",
        "man/cat1",
        "man/cat2",
        "man/cat3",
        "man/cat4",
        "man/cat5",
        "man/cat6",
        "man/cat7",
        "man/cat8",
        "applications",
        "gnome",
        "gnome/help",
        "icons",
        "mime",
        "mime/packages",
        "mime-info",
        "pixmaps",
        "postgresql",
        "sounds",
    ];
    EXACT.contains(&path)
        || ["icons/", "zsh", "fish", "lua/", "guile/", "pypy"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        || starts_with_numbered_prefix(path, "postgresql@")
        || locale_directory(path)
}

fn locale_directory(path: &str) -> bool {
    ["locale/", "man/"].iter().any(|marker| {
        path.match_indices(marker).any(|(index, _)| {
            let locale = &path[index + marker.len()..];
            locale.starts_with('C')
                || locale.starts_with("POSIX")
                || locale.get(0..2).is_some_and(|language| {
                    language
                        .chars()
                        .all(|character| character.is_ascii_lowercase())
                })
        })
    })
}

fn info_path(path: &str) -> Option<&str> {
    path.match_indices("info/")
        .map(|(index, marker)| &path[index + marker.len()..])
        .find(|suffix| {
            *suffix == "dir"
                || (!suffix.starts_with('.')
                    && (suffix.ends_with(".info") || suffix.ends_with(".info.gz")))
        })
}

fn lib_path_requires_mkpath(path: &str) -> bool {
    ["cps", "pkgconfig", "cmake", "dtrace", "ghc", "php"].contains(&path)
        || [
            "gdk-pixbuf",
            "gio",
            "lua",
            "mecab",
            "node",
            "ocaml",
            "perl5",
            "pypy",
            "R",
            "ruby",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || starts_with_numbered_prefix(path, "postgresql@")
        || starts_with_numbered_prefix(path, "python2.")
        || starts_with_numbered_prefix(path, "python3.")
}

fn starts_with_numbered_prefix(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .and_then(|suffix| suffix.chars().next())
        .is_some_and(|character| character.is_ascii_digit())
}

fn plan_public_topology(
    name: &str,
    keg: &Path,
    allow_replacement: bool,
) -> Result<Vec<TopologyRepairLink>> {
    let mut repairs = vec![];
    for root_name in LINK_DIRS {
        let source = keg.join(root_name);
        match source.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                "brew:{name}: public keg root has ambiguous type: {}",
                source.display()
            ),
        }
        let destination = prefix::prefix().join(root_name);
        PublicTopologyPlanner {
            name,
            root_name,
            keg,
            allow_replacement,
            repairs: &mut repairs,
        }
        .plan_directory(&source, &destination, Path::new(""), KegLinkPolicy::Mkpath)?;
    }
    Ok(repairs)
}

struct PublicTopologyPlanner<'a> {
    name: &'a str,
    root_name: &'a str,
    keg: &'a Path,
    allow_replacement: bool,
    repairs: &'a mut Vec<TopologyRepairLink>,
}

impl PublicTopologyPlanner<'_> {
    fn plan_directory(
        &mut self,
        source: &Path,
        destination: &Path,
        relative: &Path,
        policy: KegLinkPolicy,
    ) -> Result<()> {
        let name = self.name;
        if policy == KegLinkPolicy::Skip {
            return Ok(());
        }
        let metadata = destination.symlink_metadata();
        if symlink_points_to_checked(destination, source)? && policy == KegLinkPolicy::Link {
            return Ok(());
        }
        if policy == KegLinkPolicy::Link
            && metadata
                .as_ref()
                .is_ok_and(|metadata| metadata.file_type().is_symlink() && destination.is_dir())
            && resolved_symlink_target_checked(destination)?
                .is_some_and(|target| target.starts_with(prefix::cellar().join(name)))
        {
            if !self.allow_replacement {
                bail!(
                    "brew:{name}: topology repair would traverse a directory symlink: {}",
                    destination.display()
                );
            }
            self.repairs.push(topology_repair_link(
                destination.to_path_buf(),
                TopologyPrevious::Symlink(std::fs::read_link(destination)?),
                TopologyOperation::Link(source.to_path_buf()),
            )?);
            return Ok(());
        }
        match metadata {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !can_overwrite(name, destination)? {
                    bail!(
                        "brew:{name}: public directory has ambiguous ancestor ownership: {}",
                        destination.display()
                    );
                }
                if policy == KegLinkPolicy::Link {
                    self.repairs.push(topology_repair_link(
                        destination.to_path_buf(),
                        TopologyPrevious::Absent,
                        TopologyOperation::Link(source.to_path_buf()),
                    )?);
                    return Ok(());
                }
                self.repairs.push(topology_repair_link(
                    destination.to_path_buf(),
                    TopologyPrevious::Absent,
                    TopologyOperation::Directory,
                )?);
            }
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(metadata) if metadata.file_type().is_symlink() && destination.is_dir() => {
                if !points_into_cellar(destination)? {
                    bail!(
                        "brew:{name}: public directory has ambiguous ownership: {}",
                        destination.display()
                    );
                }
                self.repairs.push(topology_repair_link(
                    destination.to_path_buf(),
                    TopologyPrevious::Symlink(std::fs::read_link(destination)?),
                    TopologyOperation::Directory,
                )?);
            }
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                "brew:{name}: public directory has ambiguous ownership: {}",
                destination.display()
            ),
        }
        let mut entries = std::fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let source_entry = entry.path();
            let child_relative = relative.join(entry.file_name());
            let destination_entry = destination.join(entry.file_name());
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let child_policy = keg_link_policy(self.root_name, &child_relative, true);
                self.plan_directory(
                    &source_entry,
                    &destination_entry,
                    &child_relative,
                    child_policy,
                )?;
                continue;
            }
            if !file_type.is_file() && !file_type.is_symlink() {
                bail!(
                    "brew:{name}: unsupported special public keg entry: {}",
                    source_entry.display()
                );
            }
            let policy = keg_link_policy(self.root_name, &child_relative, false);
            if policy == KegLinkPolicy::Skip
                || source_file_is_pruned(self.keg, &source_entry, &destination_entry)?
            {
                continue;
            }
            if policy == KegLinkPolicy::Info {
                bail!(
                    "brew:{name}: install-info lifecycle is unsupported for {}",
                    source_entry.display()
                );
            }
            let matches_owned_ancestor = match brew_owned_ancestor(&destination_entry)? {
                Some(ancestor) => path_matches_through_brew_owned_ancestor(
                    &destination_entry,
                    &source_entry,
                    &ancestor,
                )?,
                None => false,
            };
            if symlink_points_to_checked(&destination_entry, &source_entry)?
                || matches_owned_ancestor
            {
                continue;
            }
            if !can_overwrite(name, &destination_entry)? {
                bail!(
                    "brew:{name}: public leaf has ambiguous ownership: {}",
                    destination_entry.display()
                );
            }
            let different_target_exists = match resolved_symlink_target_checked(&destination_entry)?
            {
                Some(target) => metadata_if_exists(&target)?.is_some(),
                None => false,
            };
            if !self.allow_replacement && different_target_exists {
                bail!(
                    "brew:{name}: public leaf points at a different installed keg: {}",
                    destination_entry.display()
                );
            }
            let previous = topology_previous(&destination_entry)?;
            self.repairs.push(topology_repair_link(
                destination_entry,
                previous,
                TopologyOperation::Link(source_entry),
            )?);
        }
        Ok(())
    }
}

fn source_file_is_pruned(_keg: &Path, source: &Path, destination: &Path) -> Result<bool> {
    if source.file_name().is_some_and(|name| name == ".DS_Store") {
        return Ok(true);
    }
    if matches!(
        source.extension().and_then(|extension| extension.to_str()),
        Some("pyc" | "pyo")
    ) && source
        .components()
        .any(|component| component.as_os_str() == "site-packages")
    {
        return Ok(true);
    }
    if source.symlink_metadata()?.file_type().is_symlink() {
        let Some(resolved) = resolved_symlink_target_checked(source)? else {
            return Ok(false);
        };
        if resolved == resolved_path_checked(destination)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn apply_topology_repair(repairs: &[TopologyRepairLink]) -> Result<()> {
    let mut completed: Vec<&TopologyRepairLink> = vec![];
    let mut runtime_ancestors = BTreeMap::new();
    for repair in repairs {
        let result = (|| -> Result<()> {
            if !repair.ancestors.is_empty() {
                validate_topology_ancestors(repair, &runtime_ancestors)?;
            }
            validate_topology_previous(&repair.destination, &repair.previous)?;
            match &repair.operation {
                TopologyOperation::Directory => match &repair.previous {
                    TopologyPrevious::Absent => {
                        crate::file::create_dir_all(&repair.destination)?;
                    }
                    TopologyPrevious::ExistingDirectory => {}
                    TopologyPrevious::Symlink(_) => {
                        materialize_brew_dirs(&repair.destination.join(".mise-materialize"))?;
                    }
                },
                TopologyOperation::Link(target) => {
                    materialize_brew_dirs(&repair.destination)?;
                    crate::file::create_dir_all(repair.destination.parent().unwrap())?;
                    match &repair.previous {
                        TopologyPrevious::Absent => {
                            crate::file::make_symlink(
                                &relative_target(target, &repair.destination),
                                &repair.destination,
                            )?;
                        }
                        TopologyPrevious::Symlink(_) => {
                            let staging = repair
                                .destination
                                .parent()
                                .unwrap()
                                .join(format!(".mise-link-{}", crate::rand::random_string(16)));
                            crate::file::make_symlink(
                                &relative_target(target, &repair.destination),
                                &staging,
                            )?;
                            if let Err(error) = crate::file::rename(&staging, &repair.destination) {
                                let _ = crate::file::remove_file(&staging);
                                return Err(error);
                            }
                        }
                        TopologyPrevious::ExistingDirectory => {
                            bail!(
                                "refusing to replace existing topology directory: {}",
                                repair.destination.display()
                            );
                        }
                    }
                }
            }
            if !repair.ancestors.is_empty() {
                record_topology_ancestors(&repair.destination, &mut runtime_ancestors)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            for completed_repair in completed.into_iter().rev() {
                match completed_repair.operation {
                    TopologyOperation::Directory => match &completed_repair.previous {
                        TopologyPrevious::Absent => {
                            let _ = std::fs::remove_dir(&completed_repair.destination);
                        }
                        TopologyPrevious::Symlink(_) => {
                            let _ = crate::file::remove_all(&completed_repair.destination);
                        }
                        TopologyPrevious::ExistingDirectory => {}
                    },
                    TopologyOperation::Link(_) => {
                        if !matches!(
                            &completed_repair.previous,
                            TopologyPrevious::ExistingDirectory
                        ) {
                            let _ = crate::file::remove_file(&completed_repair.destination);
                        }
                    }
                }
                if let TopologyPrevious::Symlink(previous) = &completed_repair.previous
                    && completed_repair.destination.symlink_metadata().is_err()
                {
                    let _ = crate::file::make_symlink(previous, &completed_repair.destination);
                }
            }
            return Err(error);
        }
        completed.push(repair);
    }
    Ok(())
}

/// Restore one missing or dangling mise-owned active-keg record without relinking the keg.
#[cfg(test)]
pub(super) fn repair_link_record(name: &str, dry_run: bool) -> Result<bool> {
    let Some(repair) = pending_record_repair(name) else {
        return Ok(false);
    };
    let record = if repair.destination == prefix::linked_keg_record(name) {
        "linked-keg record"
    } else {
        "opt record"
    };
    if dry_run {
        miseprintln!("repair {name}/{}: {record}", repair.version);
        return Ok(true);
    }
    crate::file::create_dir_all(repair.destination.parent().unwrap())?;
    crate::file::make_symlink(
        &relative_target(&repair.keg, &repair.destination),
        &repair.destination,
    )
    .wrap_err_with(|| {
        format!(
            "failed to repair Homebrew {record}: {}",
            repair.destination.display()
        )
    })?;
    Ok(true)
}

/// Find a single active record that can be reconstructed from its valid counterpart.
#[cfg(test)]
fn pending_record_repair(name: &str) -> Option<RecordRepair> {
    let opt = prefix::prefix().join("opt").join(name);
    let linked = prefix::linked_keg_record(name);
    if let Some((version, keg)) = record_keg(name, &opt) {
        if keg_is_keg_only(name, &keg) {
            return None;
        }
        if record_needs_replacement(name, &linked) && has_public_link_into(&keg) {
            return Some(RecordRepair {
                version,
                keg,
                destination: linked,
            });
        }
        return None;
    }
    if record_needs_replacement(name, &opt)
        && let Some((version, keg)) = record_keg(name, &linked)
    {
        return Some(RecordRepair {
            version,
            keg,
            destination: opt,
        });
    }
    None
}

/// Resolve a record only when it targets an existing direct child of the formula rack.
fn record_keg(name: &str, record: &Path) -> Option<(String, PathBuf)> {
    let target = record_target(name, record)?.canonicalize().ok()?;
    let rack = prefix::cellar().join(name).canonicalize().ok()?;
    if target.parent()? != rack || !target.is_dir() {
        return None;
    }
    let version = target.file_name()?.to_string_lossy().to_string();
    Some((version.clone(), keg_path(name, &version)))
}

/// Resolve a record target within the formula rack without requiring the keg to exist.
fn record_target(name: &str, record: &Path) -> Option<PathBuf> {
    let target = resolved_symlink_target(record)?;
    let rack = prefix::cellar().join(name).canonicalize().ok()?;
    (target.parent()? == rack).then_some(target)
}

/// Return true only for an absent path or a dangling symlink owned by this formula rack.
fn record_needs_replacement(name: &str, path: &Path) -> bool {
    match path.symlink_metadata() {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Ok(metadata) if metadata.file_type().is_symlink() => {
            record_target(name, path).is_some() && record_keg(name, path).is_none()
        }
        Err(_) | Ok(_) => false,
    }
}

/// Check for the standard public-link shape created from a non-keg-only keg.
fn has_public_link_into(keg: &Path) -> bool {
    LINK_DIRS.iter().any(|dir| {
        let root = keg.join(dir);
        root.exists()
            && walkdir::WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .any(|entry| {
                    entry
                        .path()
                        .strip_prefix(keg)
                        .ok()
                        .map(|relative| prefix::prefix().join(relative))
                        .is_some_and(|link| symlink_points_to(&link, entry.path()))
                })
    })
}

/// Compare a symlink's one-hop target with a destination using resolved parent paths.
fn symlink_points_to(link: &Path, target: &Path) -> bool {
    resolved_symlink_target(link).as_ref() == Some(&resolved_path(target))
}

fn symlink_points_to_checked(link: &Path, target: &Path) -> Result<bool> {
    Ok(resolved_symlink_target_checked(link)?.as_ref() == Some(&resolved_path_checked(target)?))
}

/// installed versions of this formula; the active keg (per the `opt`
/// symlink, like brew) first, the rest name-sorted
pub fn installed_versions(name: &str) -> Vec<String> {
    let dir = prefix::cellar().join(name);
    let mut versions: Vec<String> = crate::file::ls(&dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().to_string();
            (!name.starts_with(".mise-")).then_some(name)
        })
        .collect();
    versions.sort();
    let opt_target = std::fs::read_link(prefix::prefix().join("opt").join(name))
        .ok()
        .and_then(|t| t.file_name().map(|f| f.to_string_lossy().to_string()));
    if let Some(active) = opt_target
        && let Some(pos) = versions.iter().position(|v| v == &active)
    {
        versions.swap(0, pos);
    }
    versions
}

pub(super) struct BottlePour<'a> {
    pub rf: &'a ResolvedFormula,
    pub tag: &'a str,
    pub bottle: &'a BottleFile,
    pub oci_metadata: Option<&'a OciBottleMetadata>,
    pub tarball: &'a VerifiedArtifact,
    pub closure: &'a [ResolvedFormula],
    pub lifecycle: &'a super::lifecycle::PreparedFormulaLifecycle,
    pub pr: &'a dyn SingleReport,
}

pub async fn pour(input: BottlePour<'_>) -> Result<()> {
    let BottlePour {
        rf,
        tag,
        bottle,
        oci_metadata,
        tarball,
        closure,
        lifecycle,
        pr,
    } = input;
    let name = &rf.formula.name;
    validate_formula_install_policy(&rf.formula)?;
    let pkg_version = rf.formula.pkg_version()?;
    let keg = keg_path(name, &pkg_version);
    prepare_formula_rack(&keg)?;
    let rack = keg.parent().unwrap().to_path_buf();
    let transaction = crate::rand::random_string(32);
    let tmp = rack.join(format!(".mise-tmp-{pkg_version}-{transaction}"));
    let scratch = rack.join(format!(".mise-extract-{pkg_version}-{transaction}"));
    for dir in [&tmp, &scratch] {
        if metadata_if_exists(dir)?.is_some() {
            bail!("formula staging path already exists: {}", dir.display());
        }
    }
    crate::file::create_dir_all(&scratch)?;
    let mut staging = OwnedStagingDirectories::default();
    staging.track(&scratch)?;

    // bottle tarballs contain <name>/<pkg_version>/...
    pr.set_message("extract".to_string());
    crate::file::untar_file(
        tarball.reader()?,
        tarball.label(),
        &scratch,
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: Some(pr),
            preserve_mtime: true,
        },
    )
    .wrap_err_with(|| format!("failed to extract bottle for {name}"))?;
    let name_dir = scratch.join(name);
    require_direct_real_child(&scratch, &name_dir, "bottle formula directory")?;
    let inner = name_dir.join(&pkg_version);
    require_direct_real_child(&name_dir, &inner, "bottle keg directory")?;
    let Some(inner_metadata) = metadata_if_exists(&inner)? else {
        bail!("unexpected bottle layout for {name}: missing {name}/{pkg_version} in archive");
    };
    if !inner_metadata.is_dir() || inner_metadata.file_type().is_symlink() {
        bail!("unexpected bottle layout for {name}: {name}/{pkg_version} is not a real directory");
    }
    require_direct_real_child(&inner, &inner.join(".brew"), "bottle metadata directory")?;
    crate::file::rename(&inner, &tmp)?;
    staging.track(&tmp)?;
    lifecycle::validate_lifecycle_keg_ancestry(&tmp)
        .wrap_err("extracted bottle keg has unsafe ancestry")?;
    crate::file::remove_all(&scratch)?;

    // Select and validate provenance while the checksum-verified archive is
    // still private. Archive bottles must carry their own receipt; absence
    // never falls through to source-build fact discovery.
    let provenance = match oci_metadata {
        Some(metadata) => FormulaInstallProvenance::OciBottle {
            tab: metadata.tab.clone(),
            sbom: read_bottle_sbom(rf, &tmp, "OCI bottle")?,
            sbom_supplement: metadata.sbom_supplement.clone(),
        },
        None => archive_bottle_provenance(rf, &tmp)?,
    };
    validate_bottle_provenance(rf, &provenance)?;

    // ":any_skip_relocation" skips binary linkage relocation, but Homebrew
    // still replaces placeholders in text files. On Linux, bottles built by
    // Homebrew < 5.1.15 are incorrectly tagged and still need ELF linkage
    // relocation (brew applies the same version check in
    // extend/os/linux/bottle_specification.rb).
    let skip_linkage = bottle.cellar == ":any_skip_relocation"
        && (cfg!(target_os = "macos") || bottled_by_homebrew_at_least(&provenance, (5, 1, 15)));
    pr.set_message("relocate".to_string());
    let report = relocate::relocate_keg(&tmp, name, skip_linkage)?;
    // arm64 macOS kills binaries whose signature doesn't match; Linux ELF
    // files have no signatures to fix
    if cfg!(target_os = "macos") && !report.changed_machos.is_empty() {
        pr.set_message("codesign".to_string());
        relocate::codesign(&report.changed_machos)
            .wrap_err_with(|| format!("failed to re-sign relocated binaries for {name}"))?;
    }

    finalize_formula(FormulaFinalizer {
        rf,
        tag,
        staged_keg: &tmp,
        keg: &keg,
        report: &report,
        closure,
        provenance,
        lifecycle,
        pr,
        existing_backup: None,
        predecessor_keg: active_keg(name),
    })
    .await
}

pub(super) fn validate_formula_install_policy(formula: &super::api::Formula) -> Result<()> {
    super::resolve::validate_formula_path_identity(formula)?;
    formula.validate_install_policy()?;
    // Pinned Homebrew skips conflict checks when it will not link the keg.
    // mise's typed boundary likewise never publishes keg-only formulae into
    // the shared public topology.
    if formula.keg_only {
        return Ok(());
    }
    for conflict in formula.conflicts_with() {
        let linked = prefix::linked_keg_record(conflict);
        let opt = prefix::prefix().join("opt").join(conflict);
        if metadata_follow_if_exists(&linked)?.is_some()
            && metadata_follow_if_exists(&opt)?.is_some()
        {
            let reason = formula
                .conflict_reason(conflict)
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default();
            bail!(
                "brew:{} conflicts with linked formula {conflict}{reason}",
                formula.name
            );
        }
    }
    Ok(())
}

fn archive_bottle_provenance(rf: &ResolvedFormula, keg: &Path) -> Result<FormulaInstallProvenance> {
    let receipt_path = keg.join("INSTALL_RECEIPT.json");
    let tab: Value = serde_json::from_slice(&read_required_regular_file(
        &receipt_path,
        "non-OCI archive bottle has no embedded receipt",
    )?)
    .wrap_err_with(|| {
        format!(
            "brew:{}: malformed embedded archive-bottle receipt",
            rf.formula.name
        )
    })?;
    let sbom = read_bottle_sbom(rf, keg, "non-OCI archive bottle")?;
    Ok(FormulaInstallProvenance::ArchiveBottle { tab, sbom })
}

fn read_bottle_sbom(rf: &ResolvedFormula, keg: &Path, kind: &str) -> Result<Value> {
    let sbom_path = keg.join("sbom.spdx.json");
    serde_json::from_slice(&read_required_regular_file(
        &sbom_path,
        &format!("brew:{}: {kind} embedded SBOM", rf.formula.name),
    )?)
    .wrap_err_with(|| format!("brew:{}: malformed embedded {kind} SBOM", rf.formula.name))
}

fn read_required_regular_file(path: &Path, description: &str) -> Result<Vec<u8>> {
    let metadata = path
        .symlink_metadata()
        .wrap_err_with(|| format!("{description} is missing: {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("{description} is not a regular file: {}", path.display());
    }
    std::fs::read(path).wrap_err_with(|| format!("could not read {}", path.display()))
}

fn validate_bottle_provenance(
    rf: &ResolvedFormula,
    provenance: &FormulaInstallProvenance,
) -> Result<()> {
    let (kind, tab, sbom, require_receipt_identity) = match provenance {
        FormulaInstallProvenance::OciBottle { tab, sbom, .. } => {
            ("OCI bottle", tab, Some(sbom), false)
        }
        FormulaInstallProvenance::ArchiveBottle { tab, sbom } => {
            ("archive bottle", tab, Some(sbom), true)
        }
        FormulaInstallProvenance::SourceBuild { .. } => {
            bail!("source-build provenance passed to bottle validation")
        }
    };
    let facts: BottleFacts = serde_json::from_value(tab.clone())
        .wrap_err_with(|| format!("brew:{}: incomplete {kind} receipt", rf.formula.name))?;
    if facts.poured_from_bottle == Some(false) {
        bail!(
            "brew:{}: {kind} receipt says it was not poured from a bottle",
            rf.formula.name
        );
    }
    if let Some(source) = &facts.source {
        let actual = source.pointer("/versions/stable").and_then(Value::as_str);
        if actual != rf.formula.versions.stable.as_deref() {
            bail!(
                "brew:{}: {kind} receipt version {:?} does not match formula version {:?}",
                rf.formula.name,
                actual,
                rf.formula.versions.stable
            );
        }
    } else if require_receipt_identity {
        bail!(
            "brew:{}: archive-bottle receipt has no source version identity",
            rf.formula.name
        );
    }
    if let Some(sbom) = sbom {
        let identity_matches =
            sbom.get("packages")
                .and_then(Value::as_array)
                .is_some_and(|packages| {
                    packages.iter().any(|package| {
                        package.get("name").and_then(Value::as_str)
                            == Some(rf.formula.name.as_str())
                            && package.get("versionInfo").and_then(Value::as_str)
                                == rf.formula.versions.stable.as_deref()
                    })
                });
        if !identity_matches {
            bail!(
                "brew:{}: {kind} SBOM identity does not match formula/version",
                rf.formula.name,
            );
        }
    }
    Ok(())
}

/// Was this bottle built by Homebrew >= `min`? Read from the receipt the
/// bottle ships with (brew calls it the tab), before we overwrite it with our
/// own. This mirrors brew's own `parsed_homebrew_version >= "5.1.15"` check —
/// brew's version format is dotted numerics, not an arbitrary tool version.
fn bottled_by_homebrew_at_least(
    provenance: &FormulaInstallProvenance,
    min: (u64, u64, u64),
) -> bool {
    let tab = match provenance {
        FormulaInstallProvenance::OciBottle { tab, .. }
        | FormulaInstallProvenance::ArchiveBottle { tab, .. } => tab,
        FormulaInstallProvenance::SourceBuild { .. } => return false,
    };
    let Some(version) = tab.get("homebrew_version").and_then(Value::as_str) else {
        return false;
    };
    // "5.1.16-31-ga1b2c3d" -> (5, 1, 16); unparseable -> (0, 0, 0) = old
    let mut parts = version
        .split(['.', '-', ' '])
        .map(|p| p.parse::<u64>().unwrap_or(0));
    let v = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    v >= min
}

pub(super) fn backup_existing_keg(keg: &Path) -> Result<Option<PathBuf>> {
    let backup = recovery_backup_path(keg)?;
    if let Some(backup_metadata) = metadata_if_exists(&backup)? {
        let state = read_finalization_state(keg)?.ok_or_else(|| {
            eyre::eyre!(
                "refusing to reuse recovery backup {} without finalization state",
                backup.display()
            )
        })?;
        validate_finalization_identity(keg, &state)?;
        if state.phase == FinalizationPhase::Complete {
            bail!(
                "refusing to reuse recovery backup {} for a completed transaction",
                backup.display()
            );
        }
        if !backup_metadata.is_dir() || backup_metadata.file_type().is_symlink() {
            bail!("recovery backup is not a directory: {}", backup.display());
        }
        if let Some(keg_metadata) = metadata_if_exists(keg)? {
            if !keg_metadata.is_dir() || keg_metadata.file_type().is_symlink() {
                bail!("interrupted keg is not a directory: {}", keg.display());
            }
            crate::file::remove_all(keg)?;
        }
        return Ok(Some(backup));
    }
    let Some(keg_metadata) = metadata_if_exists(keg)? else {
        return Ok(None);
    };
    if !keg_metadata.is_dir() || keg_metadata.file_type().is_symlink() {
        bail!("existing keg is not a real directory: {}", keg.display());
    }
    if let Some(state) = read_finalization_state(keg)?
        && state.phase != FinalizationPhase::Complete
    {
        validate_finalization_identity(keg, &state)?;
    }
    crate::file::rename(keg, &backup)?;
    Ok(Some(backup))
}

#[derive(Debug)]
pub(super) struct SourceBuildTransaction {
    pub existing_backup: Option<PathBuf>,
    pub predecessor_keg: Option<PathBuf>,
}

/// Establish durable authority for a source build before moving the current
/// keg or allowing the compiler to write into the final Cellar path.
pub(super) fn begin_source_build_transaction(
    formula: &str,
    version: &str,
    keg: &Path,
    predecessor_keg: Option<PathBuf>,
    lifecycle_identity_sha256: String,
) -> Result<SourceBuildTransaction> {
    let backup = recovery_backup_path(keg)?;
    let previous_bytes = read_finalization_state_bytes(keg)?;
    let previous_state = previous_bytes
        .as_deref()
        .map(serde_json::from_slice::<FinalizationState>)
        .transpose()
        .wrap_err_with(|| format!("brew:{formula}: unreadable formula finalization state"))?;

    if let Some(state) = previous_state.as_ref()
        && state.phase == FinalizationPhase::Building
    {
        let mut state = state.clone();
        if state.lifecycle_identity_sha256.as_deref() != Some(&lifecycle_identity_sha256) {
            bail!("brew:{formula}: source-build lifecycle plan changed during retry");
        }
        validate_finalization_identity(keg, &state)?;
        validate_lifecycle_predecessor_identity(keg, &state)?;
        finish_quiescing_links(&state)?;
        let incarnation = state.build_incarnation.as_deref().ok_or_else(|| {
            eyre::eyre!("brew:{formula}: source-build transaction has no incarnation")
        })?;
        let removed_partial =
            metadata_if_exists(keg)?.is_some() && build_keg_matches(keg, &state, incarnation)?;
        if removed_partial {
            crate::file::remove_all(keg)?;
            validate_finalization_identity(keg, &state)?;
        }
        let existing_backup = if metadata_if_exists(&backup)?.is_some() {
            Some(backup.clone())
        } else if removed_partial && state.predecessor_identity.is_some() {
            bail!("source-build recovery backup disappeared during retry");
        } else {
            backup_existing_keg(keg)?
        };
        create_bound_build_keg(keg, incarnation)?;
        state.build_root_identity = Some(capture_path_identity(keg)?);
        write_finalization_state(keg, &state)?;
        return Ok(SourceBuildTransaction {
            existing_backup,
            predecessor_keg: state.predecessor_keg.clone(),
        });
    }

    if let Some(state) = previous_state.as_ref()
        && state.phase != FinalizationPhase::Complete
    {
        bail!("brew:{formula}/{version} has an incomplete non-build finalization transaction");
    }
    if metadata_if_exists(&backup)?.is_some() {
        bail!(
            "brew:{formula}: refusing stale recovery backup without an identity-bound source-build transaction"
        );
    }

    let current_identity = if metadata_if_exists(keg)?.is_some() {
        Some(capture_finalization_install_identity(formula, keg, false)?)
    } else {
        None
    };
    let planned_backup = current_identity.as_ref().map(|_| backup.clone());
    let predecessor_keg = predecessor_keg.and_then(|predecessor| {
        if predecessor == keg {
            planned_backup.clone()
        } else {
            Some(predecessor)
        }
    });
    let lifecycle_predecessor_identity = match predecessor_keg.as_deref() {
        Some(predecessor) if planned_backup.as_deref() == Some(predecessor) => None,
        Some(predecessor) if metadata_if_exists(predecessor)?.is_some() => Some(
            capture_finalization_install_identity(formula, predecessor, false)?,
        ),
        _ => None,
    };
    let incarnation = crate::rand::random_string(32);
    let mut state = FinalizationState {
        formula: formula.to_string(),
        version: version.to_string(),
        provenance: "source_build".to_string(),
        phase: FinalizationPhase::Building,
        predecessor_keg,
        replacement_identity: None,
        predecessor_identity: current_identity.clone(),
        lifecycle_predecessor_identity,
        receipt_identity: current_identity.clone(),
        receipt_current: Some(if current_identity.is_some() {
            ReceiptCurrent::Predecessor
        } else {
            ReceiptCurrent::Absent
        }),
        build_incarnation: Some(incarnation.clone()),
        previous_finalization_state: previous_bytes,
        lifecycle_identity_sha256: Some(lifecycle_identity_sha256),
        build_root_identity: None,
        quiesced_links: vec![],
    };
    write_finalization_state(keg, &state)?;
    let result = (|| -> Result<SourceBuildTransaction> {
        if current_identity.is_some() {
            quiesce_keg_links(keg, &mut state)?;
        }
        let existing_backup = backup_existing_keg(keg)?;
        create_bound_build_keg(keg, &incarnation)?;
        state.build_root_identity = Some(capture_path_identity(keg)?);
        write_finalization_state(keg, &state)?;
        Ok(SourceBuildTransaction {
            existing_backup,
            predecessor_keg: state.predecessor_keg.clone(),
        })
    })();
    if result.is_err() {
        rollback_source_build_transaction(keg)?;
    }
    result
}

/// Roll back only a partial source-build keg carrying this transaction's
/// nonce. A foreign replacement is never removed to recover the predecessor.
pub(super) fn rollback_source_build_transaction(keg: &Path) -> Result<()> {
    let state = read_finalization_state(keg)?
        .ok_or_else(|| eyre::eyre!("refusing source-build rollback without transaction state"))?;
    if state.phase != FinalizationPhase::Building {
        bail!("refusing source-build rollback after finalization began");
    }
    validate_finalization_identity(keg, &state)?;
    let incarnation = state
        .build_incarnation
        .as_deref()
        .ok_or_else(|| eyre::eyre!("source-build transaction has no incarnation"))?;
    let backup = recovery_backup_path(keg)?;
    let has_backup = metadata_if_exists(&backup)?.is_some();
    if metadata_if_exists(keg)?.is_some() {
        if build_keg_matches(keg, &state, incarnation)? {
            crate::file::remove_all(keg)?;
        } else if has_backup {
            bail!(
                "refusing to remove unbound keg during source-build rollback: {}",
                keg.display()
            );
        }
    }
    if has_backup {
        restore_keg_backup(keg, Some(&backup))?;
    }
    restore_quiesced_links(&state)?;
    restore_finalization_state(keg, state.previous_finalization_state.as_deref())
}

fn capture_path_identity(path: &Path) -> Result<FinalizationPathIdentity> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!(
            "transaction path is not a real directory: {}",
            path.display()
        );
    }
    let (device, inode) = native_filesystem_identity(&metadata)?;
    Ok(FinalizationPathIdentity { device, inode })
}

fn build_keg_matches(keg: &Path, state: &FinalizationState, incarnation: &str) -> Result<bool> {
    if let Some(expected) = &state.build_root_identity {
        return Ok(capture_path_identity(keg)? == *expected);
    }
    build_marker_matches(keg, incarnation)
}

pub(super) fn validate_source_build_transaction(keg: &Path) -> Result<()> {
    let state = read_finalization_state(keg)?
        .ok_or_else(|| eyre::eyre!("source build has no transaction state"))?;
    if state.phase != FinalizationPhase::Building {
        bail!("source build transaction is no longer in the building phase");
    }
    validate_finalization_identity(keg, &state)
}

pub(super) fn prepare_source_build_metadata(keg: &Path) -> Result<()> {
    let state = read_finalization_state(keg)?
        .ok_or_else(|| eyre::eyre!("source build has no transaction state"))?;
    if state.phase != FinalizationPhase::Building {
        bail!("source build transaction is no longer in the building phase");
    }
    validate_finalization_identity(keg, &state)?;
    let incarnation = state
        .build_incarnation
        .as_deref()
        .ok_or_else(|| eyre::eyre!("source-build transaction has no incarnation"))?;
    let metadata_dir = keg.join(".brew");
    match metadata_if_exists(&metadata_dir)? {
        Some(_) => require_real_directory(&metadata_dir, "source-build metadata directory")?,
        None => std::fs::create_dir(&metadata_dir)?,
    }
    let marker = keg.join(FINALIZATION_INCARNATION_MARKER);
    match metadata_if_exists(&marker)? {
        Some(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if std::fs::read_to_string(&marker)? != incarnation {
                bail!(
                    "source-build incarnation marker changed: {}",
                    marker.display()
                );
            }
        }
        Some(_) => bail!(
            "source-build incarnation marker has ambiguous type: {}",
            marker.display()
        ),
        None => crate::file::write_atomic(marker, incarnation.as_bytes())?,
    }
    Ok(())
}

fn create_bound_build_keg(keg: &Path, incarnation: &str) -> Result<()> {
    if metadata_if_exists(keg)?.is_some() {
        bail!("source-build keg already exists: {}", keg.display());
    }
    let rack = keg.parent().ok_or_else(|| eyre::eyre!("keg has no rack"))?;
    crate::file::create_dir_all(rack)?;
    let staging = rack.join(format!(
        ".mise-build-{}-{incarnation}-{}",
        keg.file_name().unwrap().to_string_lossy(),
        crate::rand::random_string(32)
    ));
    if metadata_if_exists(&staging)?.is_some() {
        bail!(
            "source-build staging directory already exists: {}",
            staging.display()
        );
    }
    crate::file::create_dir_all(staging.join(".brew"))?;
    crate::file::write_atomic(
        staging.join(FINALIZATION_INCARNATION_MARKER),
        incarnation.as_bytes(),
    )?;
    crate::file::rename(staging, keg)
}

fn build_marker_matches(keg: &Path, incarnation: &str) -> Result<bool> {
    require_real_directory(&keg.join(".brew"), "source-build metadata directory")?;
    let marker = keg.join(FINALIZATION_INCARNATION_MARKER);
    let Some(metadata) = metadata_if_exists(&marker)? else {
        return Ok(false);
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    Ok(std::fs::read_to_string(marker)? == incarnation)
}

fn recovery_backup_path(keg: &Path) -> Result<PathBuf> {
    let version = keg
        .file_name()
        .ok_or_else(|| eyre::eyre!("keg has no version component"))?;
    Ok(keg
        .parent()
        .ok_or_else(|| eyre::eyre!("keg has no rack"))?
        .join(format!(".mise-backup-{}", version.to_string_lossy())))
}

pub(super) fn restore_keg_backup(keg: &Path, backup: Option<&Path>) -> Result<()> {
    if let Some(metadata) = metadata_if_exists(keg)? {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "refusing to remove non-directory keg during rollback: {}",
                keg.display()
            );
        }
        crate::file::remove_all(keg)?;
    }
    if let Some(backup) = backup {
        let metadata = metadata_if_exists(backup)?.ok_or_else(|| {
            eyre::eyre!("formula recovery backup disappeared: {}", backup.display())
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "formula recovery backup is not a real directory: {}",
                backup.display()
            );
        }
        crate::file::rename(backup, keg)?;
    }
    Ok(())
}

fn restore_bound_keg_backup(keg: &Path, backup: Option<&Path>) -> Result<()> {
    let state = read_finalization_state(keg)?
        .ok_or_else(|| eyre::eyre!("refusing formula rollback without finalization state"))?;
    validate_finalization_identity(keg, &state)?;
    restore_keg_backup(keg, backup)?;
    restore_quiesced_links(&state)
}

fn restore_uncommitted_keg(keg: &Path, backup: Option<&Path>) -> Result<()> {
    match read_finalization_state(keg)? {
        Some(state) if state.phase == FinalizationPhase::Building => {
            rollback_source_build_transaction(keg)
        }
        Some(state) if state.phase != FinalizationPhase::Complete => {
            validate_finalization_identity(keg, &state)?;
            restore_keg_backup(keg, backup)?;
            restore_quiesced_links(&state)
        }
        _ => restore_keg_backup(keg, backup),
    }
}

pub(super) struct FormulaFinalizer<'a> {
    pub rf: &'a ResolvedFormula,
    pub tag: &'a str,
    pub staged_keg: &'a Path,
    pub keg: &'a Path,
    pub report: &'a relocate::RelocationReport,
    pub closure: &'a [ResolvedFormula],
    pub provenance: FormulaInstallProvenance,
    pub lifecycle: &'a super::lifecycle::PreparedFormulaLifecycle,
    pub pr: &'a dyn SingleReport,
    pub existing_backup: Option<PathBuf>,
    pub predecessor_keg: Option<PathBuf>,
}

pub(super) async fn finalize_formula(input: FormulaFinalizer<'_>) -> Result<()> {
    let FormulaFinalizer {
        rf,
        tag,
        staged_keg,
        keg,
        report,
        closure,
        provenance,
        lifecycle,
        pr,
        existing_backup,
        predecessor_keg,
    } = input;
    let name = &rf.formula.name;
    let pkg_version = match rf.formula.pkg_version() {
        Ok(version) => version,
        Err(error) => {
            if staged_keg == keg {
                restore_uncommitted_keg(keg, existing_backup.as_deref())?;
            }
            return Err(error);
        }
    };
    let interrupted_complete = match complete_interrupted_finalization(keg) {
        Ok(complete) => complete,
        Err(error) => {
            if staged_keg == keg {
                restore_uncommitted_keg(keg, existing_backup.as_deref())?;
            }
            return Err(error);
        }
    };
    if interrupted_complete {
        if staged_keg != keg && metadata_if_exists(staged_keg)?.is_some() {
            crate::file::remove_all(staged_keg)?;
        }
        return Ok(());
    }
    let prepared_transaction = (|| -> Result<_> {
        let lifecycle_identity_sha256 = lifecycle::prepared_identity_sha256(lifecycle)?;
        let previous_finalization_state = read_finalization_state_bytes(keg)?;
        let previous_state = previous_finalization_state
            .as_deref()
            .map(serde_json::from_slice::<FinalizationState>)
            .transpose()
            .wrap_err_with(|| format!("brew:{name}: unreadable formula finalization state"))?;
        if let Some(state) = &previous_state {
            validate_finalization_identity(keg, state)?;
            if state.phase != FinalizationPhase::Complete
                && state.lifecycle_identity_sha256.as_deref()
                    != Some(lifecycle_identity_sha256.as_str())
            {
                bail!("brew:{name}: lifecycle plan changed during finalization retry");
            }
        }
        let recovery_backup = recovery_backup_path(keg)?;
        if metadata_if_exists(&recovery_backup)?.is_some()
            && (previous_state
                .as_ref()
                .is_none_or(|state| state.phase == FinalizationPhase::Complete))
            && existing_backup.as_deref() != Some(recovery_backup.as_path())
        {
            bail!(
                "brew:{name}: refusing stale recovery backup without an identity-bound incomplete transaction"
            );
        }
        let predecessor_keg = previous_state
            .as_ref()
            .filter(|state| state.phase != FinalizationPhase::Complete)
            .map(|state| state.predecessor_keg.clone())
            .unwrap_or(predecessor_keg);
        let planned_backup = if staged_keg == keg {
            existing_backup.clone()
        } else {
            let backup = recovery_backup_path(keg)?;
            (metadata_if_exists(keg)?.is_some() || metadata_if_exists(&backup)?.is_some())
                .then_some(backup)
        };
        let predecessor_keg = predecessor_keg.and_then(|predecessor| {
            if predecessor == keg {
                planned_backup.clone()
            } else {
                Some(predecessor)
            }
        });
        let lifecycle_predecessor_identity = if let Some(previous) = previous_state
            .as_ref()
            .filter(|state| state.phase != FinalizationPhase::Complete)
        {
            previous.lifecycle_predecessor_identity.clone()
        } else {
            match predecessor_keg.as_deref() {
                Some(predecessor) if planned_backup.as_deref() == Some(predecessor) => None,
                Some(predecessor) if metadata_if_exists(predecessor)?.is_some() => Some(
                    capture_finalization_install_identity(name, predecessor, false)?,
                ),
                _ => None,
            }
        };
        let build_incarnation = previous_state
            .as_ref()
            .filter(|state| state.phase == FinalizationPhase::Building)
            .and_then(|state| state.build_incarnation.clone());
        let rollback_finalization_state = original_finalization_state_bytes(
            previous_state.as_ref(),
            previous_finalization_state.clone(),
        );
        let quiesced_links = previous_state
            .as_ref()
            .filter(|state| state.phase != FinalizationPhase::Complete)
            .map(|state| state.quiesced_links.clone())
            .unwrap_or_default();
        Ok((
            previous_finalization_state,
            predecessor_keg,
            planned_backup,
            lifecycle_predecessor_identity,
            build_incarnation,
            rollback_finalization_state,
            lifecycle_identity_sha256,
            quiesced_links,
        ))
    })();
    let (
        previous_finalization_state,
        predecessor_keg,
        planned_backup,
        lifecycle_predecessor_identity,
        build_incarnation,
        rollback_finalization_state,
        lifecycle_identity_sha256,
        mut quiesced_links,
    ) = match prepared_transaction {
        Ok(prepared) => prepared,
        Err(error) => {
            if staged_keg == keg {
                restore_uncommitted_keg(keg, existing_backup.as_deref())?;
            }
            return Err(error);
        }
    };
    let provenance_name = match &provenance {
        FormulaInstallProvenance::OciBottle { .. } => "oci_bottle",
        FormulaInstallProvenance::ArchiveBottle { .. } => "archive_bottle",
        FormulaInstallProvenance::SourceBuild { .. } => "source_build",
    };
    if let Err(error) = write_receipt(rf, tag, staged_keg, report, closure, &provenance) {
        if staged_keg == keg {
            restore_uncommitted_keg(keg, existing_backup.as_deref())?;
        }
        return Err(error);
    }
    let prepared_identities = (|| -> Result<_> {
        let replacement_identity = capture_finalization_install_identity_with_incarnation(
            name,
            staged_keg,
            true,
            build_incarnation.as_deref(),
        )?;
        let planned_backup_exists = match &planned_backup {
            Some(backup) => metadata_if_exists(backup)?.is_some(),
            None => false,
        };
        let predecessor_identity = if planned_backup_exists {
            Some(capture_finalization_install_identity(
                name,
                planned_backup.as_ref().unwrap(),
                false,
            )?)
        } else if staged_keg != keg && metadata_if_exists(keg)?.is_some() {
            Some(capture_finalization_install_identity(name, keg, false)?)
        } else {
            None
        };
        let receipt_current = if staged_keg == keg {
            ReceiptCurrent::Replacement
        } else if metadata_if_exists(keg)?.is_some() && planned_backup_exists {
            ReceiptCurrent::Discarded
        } else if metadata_if_exists(keg)?.is_some() {
            ReceiptCurrent::Predecessor
        } else {
            ReceiptCurrent::Absent
        };
        let receipt_identity = if matches!(
            receipt_current,
            ReceiptCurrent::Predecessor | ReceiptCurrent::Discarded
        ) {
            if planned_backup_exists {
                Some(capture_finalization_install_identity(name, keg, false)?)
            } else {
                predecessor_identity.clone()
            }
        } else {
            None
        };
        Ok((
            replacement_identity,
            predecessor_identity,
            receipt_current,
            receipt_identity,
        ))
    })();
    let (replacement_identity, predecessor_identity, receipt_current, receipt_identity) =
        match prepared_identities {
            Ok(identities) => identities,
            Err(error) => {
                if staged_keg == keg {
                    restore_uncommitted_keg(keg, existing_backup.as_deref())?;
                    restore_finalization_state(keg, previous_finalization_state.as_deref())?;
                }
                return Err(error);
            }
        };
    let mut receipt_state = FinalizationState {
        formula: name.clone(),
        version: pkg_version.clone(),
        provenance: provenance_name.to_string(),
        phase: FinalizationPhase::Receipt,
        predecessor_keg: predecessor_keg.clone(),
        replacement_identity: Some(replacement_identity.clone()),
        predecessor_identity: predecessor_identity.clone(),
        lifecycle_predecessor_identity: lifecycle_predecessor_identity.clone(),
        receipt_identity: receipt_identity.clone(),
        receipt_current: Some(receipt_current),
        build_incarnation: None,
        previous_finalization_state: rollback_finalization_state.clone(),
        lifecycle_identity_sha256: Some(lifecycle_identity_sha256.clone()),
        build_root_identity: None,
        quiesced_links: std::mem::take(&mut quiesced_links),
    };
    if let Err(error) = write_finalization_state(keg, &receipt_state) {
        if staged_keg == keg {
            restore_uncommitted_keg(keg, existing_backup.as_deref())?;
            restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        }
        return Err(error);
    }
    if staged_keg != keg
        && receipt_state.predecessor_identity.is_some()
        && let Err(error) = quiesce_keg_links(keg, &mut receipt_state)
    {
        restore_quiesced_links(&receipt_state)?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        return Err(error);
    }
    let quiesced_links = receipt_state.quiesced_links.clone();

    let backup = if staged_keg == keg {
        existing_backup
    } else {
        let backup = match backup_existing_keg(keg) {
            Ok(backup) => backup,
            Err(error) => {
                restore_finalization_state(keg, previous_finalization_state.as_deref())?;
                return Err(error);
            }
        };
        if let Err(error) = crate::file::rename(staged_keg, keg) {
            restore_bound_keg_backup(keg, backup.as_deref())?;
            restore_finalization_state(keg, previous_finalization_state.as_deref())?;
            return Err(error);
        }
        backup
    };
    if backup != planned_backup {
        restore_bound_keg_backup(keg, backup.as_deref())?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        bail!("brew:{name}: finalization recovery backup changed during commit");
    }
    if let Err(error) = write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::Keg,
            predecessor_keg: predecessor_keg.clone(),
            replacement_identity: Some(replacement_identity.clone()),
            predecessor_identity: predecessor_identity.clone(),
            lifecycle_predecessor_identity: lifecycle_predecessor_identity.clone(),
            receipt_identity: receipt_identity.clone(),
            receipt_current: Some(receipt_current),
            build_incarnation: None,
            previous_finalization_state: rollback_finalization_state.clone(),
            lifecycle_identity_sha256: Some(lifecycle_identity_sha256.clone()),
            build_root_identity: None,
            quiesced_links: quiesced_links.clone(),
        },
    ) {
        restore_bound_keg_backup(keg, backup.as_deref())?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        return Err(error);
    }

    pr.set_message("link".to_string());
    if let Err(error) = link_keg(name, &pkg_version, rf.formula.keg_only) {
        restore_bound_keg_backup(keg, backup.as_deref())?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        return Err(error);
    }
    // Linking is already externally visible. Retain the identity-bound Keg
    // state and replacement if this durable checkpoint fails so retry can
    // idempotently finish linking without leaving dangling public paths.
    write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::Linked,
            predecessor_keg: predecessor_keg.clone(),
            replacement_identity: Some(replacement_identity.clone()),
            predecessor_identity: predecessor_identity.clone(),
            lifecycle_predecessor_identity: lifecycle_predecessor_identity.clone(),
            receipt_identity: receipt_identity.clone(),
            receipt_current: Some(receipt_current),
            build_incarnation: None,
            previous_finalization_state: rollback_finalization_state.clone(),
            lifecycle_identity_sha256: Some(lifecycle_identity_sha256.clone()),
            build_root_identity: None,
            quiesced_links: quiesced_links.clone(),
        },
    )?;

    pr.set_message("shared state".to_string());
    let linked_state = read_finalization_state(keg)?.ok_or_else(|| {
        eyre::eyre!("brew:{name}: linked finalization state disappeared before lifecycle install")
    })?;
    validate_finalization_identity(keg, &linked_state)?;
    validate_lifecycle_predecessor_identity(keg, &linked_state)?;
    super::lifecycle::install(lifecycle, predecessor_keg.as_deref())
        .await
        .wrap_err_with(|| {
            format!(
                "failed to complete Homebrew shared-state lifecycle for {name}; \
                 the linked keg and any recovery backup are retained as needs-repair"
            )
        })?;
    let linked_state = read_finalization_state(keg)?.ok_or_else(|| {
        eyre::eyre!("brew:{name}: finalization state disappeared after lifecycle install")
    })?;
    validate_finalization_identity(keg, &linked_state)?;
    write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::SharedState,
            predecessor_keg: predecessor_keg.clone(),
            replacement_identity: Some(replacement_identity.clone()),
            predecessor_identity: predecessor_identity.clone(),
            lifecycle_predecessor_identity: lifecycle_predecessor_identity.clone(),
            receipt_identity: receipt_identity.clone(),
            receipt_current: Some(receipt_current),
            build_incarnation: None,
            previous_finalization_state: None,
            lifecycle_identity_sha256: Some(lifecycle_identity_sha256.clone()),
            build_root_identity: None,
            quiesced_links: vec![],
        },
    )?;
    write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::Complete,
            predecessor_keg,
            replacement_identity: Some(replacement_identity.clone()),
            predecessor_identity: predecessor_identity.clone(),
            lifecycle_predecessor_identity,
            receipt_identity,
            receipt_current: Some(receipt_current),
            build_incarnation: None,
            previous_finalization_state: None,
            lifecycle_identity_sha256: Some(lifecycle_identity_sha256),
            build_root_identity: None,
            quiesced_links: vec![],
        },
    )?;
    if let Some(backup) = backup {
        validate_install_identity(name, keg, &replacement_identity)?;
        let predecessor_identity = predecessor_identity.as_ref().ok_or_else(|| {
            eyre::eyre!("brew:{name}: recovery backup has no bound predecessor identity")
        })?;
        validate_install_identity(name, &backup, predecessor_identity)?;
        crate::file::remove_all(backup)?;
    }
    let incarnation_marker = keg.join(FINALIZATION_INCARNATION_MARKER);
    if metadata_if_exists(&incarnation_marker)?.is_some() {
        crate::file::remove_file(incarnation_marker)?;
    }
    Ok(())
}

fn original_finalization_state_bytes(
    current: Option<&FinalizationState>,
    current_bytes: Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    match current {
        Some(state) if state.phase != FinalizationPhase::Complete => {
            state.previous_finalization_state.clone()
        }
        _ => current_bytes,
    }
}

fn finalization_state_path(keg: &Path) -> PathBuf {
    crate::dirs::STATE
        .join("brew-formula-finalization")
        .join(format!(
            "{}.json",
            crate::hash::hash_to_str(&(prefix::prefix(), keg))
        ))
}

fn validate_finalization_state_namespace(create: bool) -> Result<()> {
    let state_root: &Path = &crate::dirs::STATE;
    require_real_directory(state_root, "mise state directory")?;
    let namespace = state_root.join("brew-formula-finalization");
    match metadata_if_exists(&namespace)? {
        Some(_) => require_real_directory(&namespace, "formula finalization namespace")?,
        None if create => std::fs::create_dir(&namespace)?,
        None => return Ok(()),
    }
    let canonical_root = state_root.canonicalize()?;
    if namespace.canonicalize()?.parent() != Some(canonical_root.as_path()) {
        bail!(
            "formula finalization namespace escapes mise state: {}",
            namespace.display()
        );
    }
    Ok(())
}

fn read_finalization_state(keg: &Path) -> Result<Option<FinalizationState>> {
    read_finalization_state_bytes(keg)?
        .map(|contents| serde_json::from_slice(&contents).map_err(Into::into))
        .transpose()
}

fn read_finalization_state_bytes(keg: &Path) -> Result<Option<Vec<u8>>> {
    validate_finalization_state_namespace(false)?;
    let path = finalization_state_path(keg);
    let Some(metadata) = metadata_if_exists(&path)? else {
        return Ok(None);
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!(
            "formula finalization state is not a regular file: {}",
            path.display()
        );
    }
    Ok(Some(std::fs::read(&path).wrap_err_with(|| {
        format!("could not read {}", path.display())
    })?))
}

fn metadata_if_exists(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match path.symlink_metadata() {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn metadata_follow_if_exists(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match path.metadata() {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn require_real_directory(path: &Path, description: &str) -> Result<()> {
    let metadata = path
        .symlink_metadata()
        .wrap_err_with(|| format!("{description} is missing: {}", path.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("{description} is not a real directory: {}", path.display());
    }
    Ok(())
}

fn require_direct_real_child(parent: &Path, child: &Path, description: &str) -> Result<()> {
    require_real_directory(parent, &format!("{description} parent"))?;
    require_real_directory(child, description)?;
    let canonical_parent = parent.canonicalize()?;
    let canonical_child = child.canonicalize()?;
    if canonical_child.parent() != Some(canonical_parent.as_path()) {
        bail!("{description} escapes its parent: {}", child.display());
    }
    Ok(())
}

#[derive(Default)]
struct OwnedStagingDirectories(Vec<(PathBuf, FinalizationPathIdentity)>);

impl OwnedStagingDirectories {
    fn track(&mut self, path: &Path) -> Result<()> {
        self.0
            .push((path.to_path_buf(), capture_path_identity(path)?));
        Ok(())
    }
}

impl Drop for OwnedStagingDirectories {
    fn drop(&mut self) {
        for (path, expected) in self.0.iter().rev() {
            let Ok(actual) = capture_path_identity(path) else {
                continue;
            };
            if &actual == expected {
                let _ = crate::file::remove_all(path);
            }
        }
    }
}

fn require_regular_file_or_absent(path: &Path, description: &str) -> Result<()> {
    if let Some(metadata) = metadata_if_exists(path)?
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        bail!("{description} is not a regular file: {}", path.display());
    }
    Ok(())
}

pub(super) fn prepare_formula_rack(keg: &Path) -> Result<()> {
    let prefix_path = prefix::prefix();
    require_real_directory(&prefix_path, "Homebrew prefix")?;
    let cellar = prefix::cellar();
    match metadata_if_exists(&cellar)? {
        Some(_) => require_real_directory(&cellar, "Homebrew Cellar")?,
        None => std::fs::create_dir(&cellar)?,
    }
    let rack = keg
        .parent()
        .ok_or_else(|| eyre::eyre!("keg has no formula rack"))?;
    if rack.parent() != Some(cellar.as_path()) {
        bail!(
            "formula keg is outside the Homebrew Cellar: {}",
            keg.display()
        );
    }
    match metadata_if_exists(rack)? {
        Some(_) => require_real_directory(rack, "formula rack")?,
        None => std::fs::create_dir(rack)?,
    }
    let canonical_cellar = cellar.canonicalize()?;
    let canonical_rack = rack.canonicalize()?;
    if canonical_rack.parent() != Some(canonical_cellar.as_path()) {
        bail!(
            "formula rack escapes the Homebrew Cellar: {}",
            rack.display()
        );
    }
    Ok(())
}

fn capture_finalization_install_identity(
    formula: &str,
    keg: &Path,
    mise_owned: bool,
) -> Result<FinalizationInstallIdentity> {
    capture_finalization_install_identity_with_incarnation(formula, keg, mise_owned, None)
}

fn capture_finalization_install_identity_with_incarnation(
    formula: &str,
    keg: &Path,
    mise_owned: bool,
    existing_incarnation: Option<&str>,
) -> Result<FinalizationInstallIdentity> {
    lifecycle::validate_lifecycle_keg_ancestry(keg)?;
    let keg_metadata = keg
        .symlink_metadata()
        .wrap_err_with(|| format!("formula finalization keg is missing: {}", keg.display()))?;
    if !keg_metadata.is_dir() || keg_metadata.file_type().is_symlink() {
        bail!(
            "formula finalization keg is not a real directory: {}",
            keg.display()
        );
    }
    require_real_directory(&keg.join(".brew"), "formula metadata directory")?;
    let snapshot = keg.join(".brew").join(format!("{formula}.rb"));
    let metadata = snapshot.symlink_metadata().wrap_err_with(|| {
        format!(
            "formula finalization identity file is missing: {}",
            snapshot.display()
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!(
            "formula finalization identity file is not a regular file: {}",
            snapshot.display()
        );
    }
    let kind = if mise_owned {
        let incarnation = if let Some(incarnation) = existing_incarnation {
            let marker = keg.join(FINALIZATION_INCARNATION_MARKER);
            let metadata = marker.symlink_metadata().wrap_err_with(|| {
                format!("formula build incarnation is missing: {}", marker.display())
            })?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || crate::file::read_to_string(&marker)? != incarnation
            {
                bail!(
                    "formula build incarnation no longer matches: {}",
                    marker.display()
                );
            }
            incarnation.to_string()
        } else {
            let incarnation = crate::rand::random_string(32);
            crate::file::write(keg.join(FINALIZATION_INCARNATION_MARKER), &incarnation)?;
            incarnation
        };
        FinalizationIdentityKind::Mise { incarnation }
    } else {
        let metadata = keg.metadata()?;
        let (device, inode) = native_filesystem_identity(&metadata)?;
        FinalizationIdentityKind::Native { device, inode }
    };
    Ok(FinalizationInstallIdentity {
        receipt_identity_sha256: lifecycle::receipt_identity_sha256(keg)?,
        snapshot_sha256: crate::hash::file_hash_sha256(&snapshot, None)?,
        kind,
    })
}

#[cfg(unix)]
fn native_filesystem_identity(metadata: &std::fs::Metadata) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn native_filesystem_identity(_metadata: &std::fs::Metadata) -> Result<(u64, u64)> {
    bail!("native Homebrew keg identity is unsupported on this platform")
}

fn validate_install_identity(
    formula: &str,
    keg: &Path,
    identity: &FinalizationInstallIdentity,
) -> Result<()> {
    lifecycle::validate_lifecycle_keg_ancestry(keg)?;
    let metadata = keg
        .symlink_metadata()
        .wrap_err_with(|| format!("bound formula keg is missing: {}", keg.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("bound formula keg is not a directory: {}", keg.display());
    }
    require_real_directory(&keg.join(".brew"), "bound formula metadata directory")?;
    let snapshot = keg.join(".brew").join(format!("{formula}.rb"));
    let snapshot_metadata = snapshot
        .symlink_metadata()
        .wrap_err_with(|| format!("bound formula snapshot is missing: {}", snapshot.display()))?;
    if !snapshot_metadata.is_file() || snapshot_metadata.file_type().is_symlink() {
        bail!(
            "bound formula snapshot is not a regular file: {}",
            snapshot.display()
        );
    }
    let kind_matches = match &identity.kind {
        FinalizationIdentityKind::Mise { .. } => install_incarnation_matches(keg, identity)?,
        FinalizationIdentityKind::Native { device, inode } => {
            let metadata = keg.metadata()?;
            native_filesystem_identity(&metadata)? == (*device, *inode)
        }
    };
    if !kind_matches
        || lifecycle::receipt_identity_sha256(keg)? != identity.receipt_identity_sha256
        || crate::hash::file_hash_sha256(&snapshot, None)? != identity.snapshot_sha256
    {
        bail!(
            "formula finalization identity no longer matches keg {}",
            keg.display()
        );
    }
    Ok(())
}

fn install_incarnation_matches(keg: &Path, identity: &FinalizationInstallIdentity) -> Result<bool> {
    let FinalizationIdentityKind::Mise { incarnation } = &identity.kind else {
        return Ok(false);
    };
    build_marker_matches(keg, incarnation)
}

fn validate_finalization_identity(keg: &Path, state: &FinalizationState) -> Result<()> {
    let formula = keg
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    let version = keg.file_name().and_then(|version| version.to_str());
    if formula != Some(state.formula.as_str()) || version != Some(state.version.as_str()) {
        bail!(
            "formula finalization state does not match keg {}",
            keg.display()
        );
    }
    if let Some(predecessor) = &state.predecessor_keg
        && predecessor.parent() != keg.parent()
    {
        bail!(
            "formula finalization predecessor is outside rack: {}",
            predecessor.display()
        );
    }
    validate_quiesced_links(state)?;
    if state.phase == FinalizationPhase::Complete {
        return Ok(());
    }
    if state.phase == FinalizationPhase::Building {
        return validate_building_identity(keg, state);
    }
    let replacement = state.replacement_identity.as_ref().ok_or_else(|| {
        eyre::eyre!("incomplete formula finalization state has no replacement identity")
    })?;
    let receipt_current = state.receipt_current.ok_or_else(|| {
        eyre::eyre!("incomplete formula finalization state has no receipt-phase identity")
    })?;
    let backup = recovery_backup_path(keg)?;
    let current_is_replacement = match metadata_if_exists(keg)? {
        Some(_) => install_incarnation_matches(keg, replacement)?,
        None => false,
    };
    match state.phase {
        FinalizationPhase::Building => unreachable!(),
        FinalizationPhase::Receipt => match keg.symlink_metadata() {
            Ok(_) if current_is_replacement => {
                validate_install_identity(&state.formula, keg, replacement)?;
                if state.predecessor_identity.is_some() {
                    validate_recovery_predecessor(keg, state)?;
                } else if metadata_if_exists(&backup)?.is_some() {
                    bail!("unexpected recovery backup for predecessor-free finalization");
                }
            }
            Ok(_)
                if matches!(
                    receipt_current,
                    ReceiptCurrent::Predecessor | ReceiptCurrent::Discarded
                ) =>
            {
                let identity = state.receipt_identity.as_ref().ok_or_else(|| {
                    eyre::eyre!(
                        "incomplete formula finalization state has no receipt-phase identity"
                    )
                })?;
                validate_install_identity(&state.formula, keg, identity)?;
                if receipt_current == ReceiptCurrent::Predecessor
                    && metadata_if_exists(&backup)?.is_some()
                {
                    bail!("recovery backup exists while predecessor remains active");
                }
                if receipt_current == ReceiptCurrent::Discarded {
                    validate_recovery_predecessor(keg, state)?;
                }
            }
            Ok(_) => {
                validate_install_identity(&state.formula, keg, replacement)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if receipt_current == ReceiptCurrent::Replacement {
                    bail!("replacement keg disappeared during formula finalization");
                }
                if state.predecessor_identity.is_some() {
                    validate_recovery_predecessor(keg, state)?;
                } else if metadata_if_exists(&backup)?.is_some() {
                    bail!("unexpected recovery backup for predecessor-free finalization");
                }
            }
            Err(error) => return Err(error.into()),
        },
        FinalizationPhase::Keg | FinalizationPhase::Linked | FinalizationPhase::SharedState => {
            validate_install_identity(&state.formula, keg, replacement)?;
            if state.predecessor_identity.is_some() {
                validate_recovery_predecessor(keg, state)?;
            } else if metadata_if_exists(&backup)?.is_some() {
                bail!("unexpected recovery backup for predecessor-free finalization");
            }
        }
        FinalizationPhase::Complete => unreachable!(),
    }
    Ok(())
}

fn validate_building_identity(keg: &Path, state: &FinalizationState) -> Result<()> {
    if state.replacement_identity.is_some() {
        bail!("source-build transaction unexpectedly has a replacement identity");
    }
    let incarnation = state
        .build_incarnation
        .as_deref()
        .ok_or_else(|| eyre::eyre!("source-build transaction has no incarnation"))?;
    let backup = recovery_backup_path(keg)?;
    let backup_exists = metadata_if_exists(&backup)?.is_some();
    let keg_metadata = metadata_if_exists(keg)?;
    if let Some(metadata) = &keg_metadata
        && (!metadata.is_dir() || metadata.file_type().is_symlink())
    {
        bail!(
            "source-build keg is not a real directory: {}",
            keg.display()
        );
    }
    let keg_exists = keg_metadata.is_some();

    match (&state.predecessor_identity, backup_exists) {
        (Some(identity), true) => validate_install_identity(&state.formula, &backup, identity)?,
        (Some(identity), false) => {
            if !keg_exists {
                bail!("source-build predecessor disappeared before backup");
            }
            validate_install_identity(&state.formula, keg, identity)?;
        }
        (None, true) => bail!("source-build transaction has an unbound recovery backup"),
        (None, false) => {}
    }

    if backup_exists && keg_exists && !build_keg_matches(keg, state, incarnation)? {
        bail!(
            "source-build keg is not owned by the active transaction: {}",
            keg.display()
        );
    }
    if !backup_exists
        && state.predecessor_identity.is_none()
        && keg_exists
        && !build_keg_matches(keg, state, incarnation)?
    {
        bail!(
            "source-build keg is not owned by the active transaction: {}",
            keg.display()
        );
    }
    Ok(())
}

fn validate_lifecycle_predecessor_identity(keg: &Path, state: &FinalizationState) -> Result<()> {
    let backup = recovery_backup_path(keg)?;
    if let Some(predecessor) = state
        .predecessor_keg
        .as_deref()
        .filter(|predecessor| *predecessor != backup)
    {
        if let Some(identity) = &state.lifecycle_predecessor_identity {
            validate_install_identity(&state.formula, predecessor, identity)?;
        } else if metadata_if_exists(predecessor)?.is_some() {
            bail!(
                "unbound lifecycle predecessor appeared during formula finalization: {}",
                predecessor.display()
            );
        }
    }
    Ok(())
}

fn validate_recovery_predecessor(keg: &Path, state: &FinalizationState) -> Result<()> {
    let backup = recovery_backup_path(keg)?;
    let identity = state.predecessor_identity.as_ref().ok_or_else(|| {
        eyre::eyre!("incomplete formula finalization state has no predecessor identity")
    })?;
    validate_install_identity(&state.formula, &backup, identity)
}

pub(super) fn complete_interrupted_finalization(keg: &Path) -> Result<bool> {
    let Some(mut state) = read_finalization_state(keg)? else {
        return Ok(false);
    };
    validate_finalization_identity(keg, &state)?;
    if state.phase == FinalizationPhase::Building {
        return Ok(false);
    }
    if state.phase == FinalizationPhase::Complete {
        let marker = keg.join(FINALIZATION_INCARNATION_MARKER);
        if metadata_if_exists(&marker)?.is_some() {
            let replacement = state.replacement_identity.as_ref().ok_or_else(|| {
                eyre::eyre!("completed formula finalization has no replacement identity")
            })?;
            validate_install_identity(&state.formula, keg, replacement)?;
            let backup = recovery_backup_path(keg)?;
            if metadata_if_exists(&backup)?.is_some() {
                let predecessor = state.predecessor_identity.as_ref().ok_or_else(|| {
                    eyre::eyre!("completed formula finalization has no predecessor identity")
                })?;
                validate_install_identity(&state.formula, &backup, predecessor)?;
                crate::file::remove_all(backup)?;
            }
            crate::file::remove_file(marker)?;
        }
        return Ok(false);
    }
    match super::lifecycle::install_progress(keg) {
        super::lifecycle::LifecycleInstallProgress::Absent => {
            validate_lifecycle_predecessor_identity(keg, &state)?;
            return Ok(false);
        }
        super::lifecycle::LifecycleInstallProgress::Incomplete => {
            bail!(
                "brew:{}/{} requires manual recovery: lifecycle execution has an unknown outcome",
                state.formula,
                state.version
            )
        }
        super::lifecycle::LifecycleInstallProgress::Complete => {}
    }
    if !matches!(
        state.phase,
        FinalizationPhase::Linked | FinalizationPhase::SharedState
    ) {
        bail!(
            "brew:{}/{} has inconsistent finalization and lifecycle phases",
            state.formula,
            state.version
        );
    }
    let keg_metadata = keg
        .symlink_metadata()
        .wrap_err_with(|| format!("interrupted keg is missing: {}", keg.display()))?;
    if !keg_metadata.is_dir() || keg_metadata.file_type().is_symlink() {
        bail!("interrupted keg is not a directory: {}", keg.display());
    }
    let backup = recovery_backup_path(keg)?;
    let has_backup = if metadata_if_exists(&backup)?.is_some() {
        let backup_metadata = backup.symlink_metadata()?;
        if !backup_metadata.is_dir() || backup_metadata.file_type().is_symlink() {
            bail!("recovery backup is not a directory: {}", backup.display());
        }
        true
    } else {
        false
    };
    state.phase = FinalizationPhase::Complete;
    state.quiesced_links.clear();
    write_finalization_state(keg, &state)?;
    if has_backup {
        crate::file::remove_all(backup)?;
    }
    let marker = keg.join(FINALIZATION_INCARNATION_MARKER);
    if metadata_if_exists(&marker)?.is_some() {
        crate::file::remove_file(marker)?;
    }
    Ok(true)
}

pub(super) async fn resume_source_finalization(
    keg: &Path,
    keg_only: bool,
    lifecycle: &super::lifecycle::PreparedFormulaLifecycle,
    pr: &dyn SingleReport,
) -> Result<bool> {
    let Some(mut state) = read_finalization_state(keg)? else {
        return Ok(false);
    };
    if state.provenance != "source_build"
        || !matches!(
            state.phase,
            FinalizationPhase::Receipt | FinalizationPhase::Keg | FinalizationPhase::Linked
        )
    {
        return Ok(false);
    }
    let lifecycle_identity_sha256 = lifecycle::prepared_identity_sha256(lifecycle)?;
    if state.lifecycle_identity_sha256.as_deref() != Some(lifecycle_identity_sha256.as_str()) {
        bail!(
            "brew:{}/{} lifecycle plan changed before source finalization resume",
            state.formula,
            state.version
        );
    }
    validate_finalization_identity(keg, &state)?;
    match super::lifecycle::install_progress(keg) {
        super::lifecycle::LifecycleInstallProgress::Absent => {}
        super::lifecycle::LifecycleInstallProgress::Incomplete => {
            bail!(
                "brew:{}/{} requires manual recovery: lifecycle execution has an unknown outcome",
                state.formula,
                state.version
            );
        }
        super::lifecycle::LifecycleInstallProgress::Complete => {
            return complete_interrupted_finalization(keg);
        }
    }
    validate_lifecycle_predecessor_identity(keg, &state)?;
    if state.phase == FinalizationPhase::Receipt {
        state.phase = FinalizationPhase::Keg;
        write_finalization_state(keg, &state)?;
    }
    if state.phase == FinalizationPhase::Keg {
        pr.set_message("link".to_string());
        link_keg(&state.formula, &state.version, keg_only)?;
        state.phase = FinalizationPhase::Linked;
        write_finalization_state(keg, &state)?;
    }

    pr.set_message("shared state".to_string());
    let linked_state = read_finalization_state(keg)?.ok_or_else(|| {
        eyre::eyre!("source finalization state disappeared before lifecycle install")
    })?;
    validate_finalization_identity(keg, &linked_state)?;
    validate_lifecycle_predecessor_identity(keg, &linked_state)?;
    super::lifecycle::install(lifecycle, linked_state.predecessor_keg.as_deref())
        .await
        .wrap_err_with(|| {
            format!(
                "failed to resume Homebrew shared-state lifecycle for {}; the linked keg and any recovery backup are retained as needs-repair",
                linked_state.formula
            )
        })?;
    let mut linked_state = read_finalization_state(keg)?.ok_or_else(|| {
        eyre::eyre!("source finalization state disappeared after lifecycle install")
    })?;
    validate_finalization_identity(keg, &linked_state)?;
    linked_state.phase = FinalizationPhase::SharedState;
    linked_state.quiesced_links.clear();
    write_finalization_state(keg, &linked_state)?;
    linked_state.phase = FinalizationPhase::Complete;
    linked_state.previous_finalization_state = None;
    write_finalization_state(keg, &linked_state)?;

    let backup = recovery_backup_path(keg)?;
    if metadata_if_exists(&backup)?.is_some() {
        let replacement = linked_state.replacement_identity.as_ref().ok_or_else(|| {
            eyre::eyre!("completed source finalization has no replacement identity")
        })?;
        let predecessor = linked_state.predecessor_identity.as_ref().ok_or_else(|| {
            eyre::eyre!("completed source finalization has no predecessor identity")
        })?;
        validate_install_identity(&linked_state.formula, keg, replacement)?;
        validate_install_identity(&linked_state.formula, &backup, predecessor)?;
        crate::file::remove_all(backup)?;
    }
    let marker = keg.join(FINALIZATION_INCARNATION_MARKER);
    if metadata_if_exists(&marker)?.is_some() {
        crate::file::remove_file(marker)?;
    }
    Ok(true)
}

#[derive(Debug)]
pub(super) struct PreparedFinalizationStateRemoval {
    path: PathBuf,
    previous: PreparedFinalizationStateDisposition,
}

#[derive(Debug)]
enum PreparedFinalizationStateDisposition {
    Absent,
    Present {
        namespace_identity: FinalizationPathIdentity,
        file_identity: FinalizationPathIdentity,
        contents_sha256: String,
    },
}

pub(super) fn prepare_remove_finalization_state(
    keg: &Path,
) -> Result<PreparedFinalizationStateRemoval> {
    validate_finalization_state_namespace(false)?;
    let path = finalization_state_path(keg);
    let previous = match metadata_if_exists(&path)? {
        None => PreparedFinalizationStateDisposition::Absent,
        Some(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                bail!(
                    "formula finalization state is not a regular file: {}",
                    path.display()
                );
            }
            let contents = std::fs::read(&path)?;
            let state: FinalizationState =
                serde_json::from_slice(&contents).wrap_err_with(|| {
                    format!(
                        "formula finalization state is unreadable: {}",
                        path.display()
                    )
                })?;
            validate_finalization_identity(keg, &state)?;
            if state.phase != FinalizationPhase::Complete {
                bail!(
                    "refusing to prune incomplete formula finalization state: {}",
                    path.display()
                );
            }
            let namespace = path
                .parent()
                .ok_or_else(|| eyre::eyre!("formula finalization state has no namespace"))?;
            let (device, inode) = native_filesystem_identity(&metadata)?;
            PreparedFinalizationStateDisposition::Present {
                namespace_identity: capture_path_identity(namespace)?,
                file_identity: FinalizationPathIdentity { device, inode },
                contents_sha256: crate::hash::hash_sha256_to_str(&String::from_utf8(contents)?),
            }
        }
    };
    Ok(PreparedFinalizationStateRemoval { path, previous })
}

pub(super) fn remove_finalization_state_prepared(
    prepared: PreparedFinalizationStateRemoval,
) -> Result<()> {
    validate_finalization_state_namespace(false)?;
    match prepared.previous {
        PreparedFinalizationStateDisposition::Absent => {
            if metadata_if_exists(&prepared.path)?.is_some() {
                bail!(
                    "formula finalization state appeared after removal preflight: {}",
                    prepared.path.display()
                );
            }
        }
        PreparedFinalizationStateDisposition::Present {
            namespace_identity,
            file_identity,
            contents_sha256,
        } => {
            let namespace = prepared
                .path
                .parent()
                .ok_or_else(|| eyre::eyre!("formula finalization state has no namespace"))?;
            if capture_path_identity(namespace)? != namespace_identity {
                bail!("formula finalization namespace changed after removal preflight");
            }
            let metadata = prepared.path.symlink_metadata().wrap_err_with(|| {
                format!(
                    "formula finalization state disappeared after removal preflight: {}",
                    prepared.path.display()
                )
            })?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                bail!(
                    "formula finalization state changed type after removal preflight: {}",
                    prepared.path.display()
                );
            }
            let (device, inode) = native_filesystem_identity(&metadata)?;
            if (FinalizationPathIdentity { device, inode }) != file_identity
                || crate::hash::file_hash_sha256(&prepared.path, None)? != contents_sha256
            {
                bail!(
                    "formula finalization state changed after removal preflight: {}",
                    prepared.path.display()
                );
            }
            crate::file::remove_file(prepared.path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn remove_finalization_state(keg: &Path) -> Result<()> {
    remove_finalization_state_prepared(prepare_remove_finalization_state(keg)?)
}

fn write_finalization_state(keg: &Path, state: &FinalizationState) -> Result<()> {
    validate_finalization_state_namespace(true)?;
    let path = finalization_state_path(keg);
    if let Some(metadata) = metadata_if_exists(&path)?
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        bail!(
            "refusing to overwrite non-file finalization state: {}",
            path.display()
        );
    }
    crate::file::write_atomic(path, serde_json::to_vec_pretty(state)?)
}

fn restore_finalization_state(keg: &Path, previous: Option<&[u8]>) -> Result<()> {
    validate_finalization_state_namespace(previous.is_some())?;
    let path = finalization_state_path(keg);
    match previous {
        Some(previous) => {
            if let Some(metadata) = metadata_if_exists(&path)?
                && (!metadata.is_file() || metadata.file_type().is_symlink())
            {
                bail!(
                    "refusing to overwrite non-file finalization state: {}",
                    path.display()
                );
            }
            crate::file::write_atomic(path, previous)
        }
        None if metadata_if_exists(&path)?.is_some() => crate::file::remove_file(path),
        None => Ok(()),
    }
}

fn finalization_needs_repair(keg: &Path) -> bool {
    if validate_finalization_state_namespace(false).is_err() {
        return true;
    }
    let path = finalization_state_path(keg);
    match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return true,
    }
    crate::file::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<FinalizationState>(&contents).ok())
        .is_none_or(|state| state.phase != FinalizationPhase::Complete)
}

/// Homebrew-compatible INSTALL_RECEIPT.json for a formula whose supported
/// lifecycle has been fully finalized. Written for both poured bottles and
/// source-built kegs; `poured_from_bottle` distinguishes them the same way
/// Homebrew's tab does.
pub fn write_receipt(
    rf: &ResolvedFormula,
    tag: &str,
    keg: &Path,
    report: &relocate::RelocationReport,
    closure: &[ResolvedFormula],
    provenance: &FormulaInstallProvenance,
) -> Result<()> {
    require_real_directory(keg, "formula keg")?;
    require_real_directory(&keg.join(".brew"), "formula metadata directory")?;
    require_regular_file_or_absent(&keg.join("INSTALL_RECEIPT.json"), "formula receipt")?;
    require_regular_file_or_absent(&keg.join("sbom.spdx.json"), "formula SBOM")?;
    let derived_runtime_dependencies: Vec<Value> = closure
        .iter()
        .filter(|other| {
            rf.formula
                .dependencies_for(tag)
                .iter()
                .any(|d| d == &other.formula.name || other.formula.aliases.contains(d))
        })
        .filter_map(|dep| {
            let pkg_version = dep.formula.pkg_version().ok()?;
            Some(json!({
                "full_name": dep.formula.name,
                "version": dep.formula.versions.stable,
                "revision": dep.formula.revision,
                "pkg_version": pkg_version,
                "declared_directly": true,
            }))
        })
        .collect();
    let relocated_files: Vec<String> = report
        .changed_files
        .iter()
        .filter_map(|p| p.strip_prefix(keg).ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let (poured_from_bottle, facts) = match provenance {
        FormulaInstallProvenance::OciBottle { tab, .. }
        | FormulaInstallProvenance::ArchiveBottle { tab, .. } => (
            true,
            serde_json::from_value::<BottleFacts>(tab.clone()).wrap_err_with(|| {
                format!(
                    "brew:{}: bottle receipt facts are incomplete",
                    rf.formula.name
                )
            })?,
        ),
        FormulaInstallProvenance::SourceBuild {
            formula_snapshot,
            compiler,
            built_on,
        } => {
            let expected = keg.join(".brew").join(format!("{}.rb", rf.formula.name));
            if formula_snapshot != &expected
                || !formula_snapshot
                    .symlink_metadata()
                    .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            {
                bail!(
                    "brew:{}: source build has no verified formula snapshot at {}",
                    rf.formula.name,
                    expected.display()
                );
            }
            let source_modified_time = formula_snapshot
                .metadata()?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            (
                false,
                BottleFacts {
                    changed_files: relocated_files,
                    source_modified_time,
                    compiler: compiler.clone(),
                    runtime_dependencies: derived_runtime_dependencies,
                    built_on: Some(built_on.clone()),
                    poured_from_bottle: Some(false),
                    source: None,
                },
            )
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let receipt = json!({
        "homebrew_version": format!("{EMULATED_BREW_VERSION} (mise)"),
        "used_options": [],
        "unused_options": [],
        "built_as_bottle": poured_from_bottle,
        "poured_from_bottle": poured_from_bottle,
        "loaded_from_api": true,
        "installed_as_dependency": !rf.on_request,
        "installed_on_request": rf.on_request,
        "changed_files": facts.changed_files,
        "time": now,
        "source_modified_time": facts.source_modified_time,
        "compiler": facts.compiler,
        "aliases": rf.formula.aliases,
        "runtime_dependencies": facts.runtime_dependencies,
        "source": {
            "spec": "stable",
            "versions": {
                "stable": rf.formula.versions.stable,
                "head": null,
                "version_scheme": 0,
            },
            "path": rf.formula.ruby_source_path,
            "tap": rf.formula.tap.as_deref().unwrap_or("homebrew/core"),
            "tap_git_head": rf.formula.tap_git_head,
        },
        "arch": if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" },
        // Homebrew's Tab#to_json always emits this key. Authoritative bottle
        // metadata may omit the build host, in which case the installed tab
        // contains JSON null rather than dropping the field.
        "built_on": facts.built_on,
    });
    crate::file::write_atomic(
        keg.join("INSTALL_RECEIPT.json"),
        serde_json::to_string(&receipt)?,
    )?;
    match provenance {
        FormulaInstallProvenance::OciBottle {
            sbom,
            sbom_supplement,
            ..
        } => {
            let current: Value = serde_json::from_slice(&read_required_regular_file(
                &keg.join("sbom.spdx.json"),
                "OCI bottle SBOM",
            )?)?;
            if &current != sbom {
                bail!(
                    "brew:{}: OCI bottle SBOM changed after validation",
                    rf.formula.name
                );
            }
            update_sbom(keg, now, sbom_supplement.as_ref())?;
        }
        FormulaInstallProvenance::ArchiveBottle { sbom, .. } => {
            let current: Value = serde_json::from_slice(&read_required_regular_file(
                &keg.join("sbom.spdx.json"),
                "archive bottle SBOM",
            )?)?;
            if &current != sbom {
                bail!(
                    "brew:{}: archive-bottle SBOM changed after validation",
                    rf.formula.name
                );
            }
        }
        FormulaInstallProvenance::SourceBuild { .. } => write_source_sbom(rf, keg, now)?,
    }
    Ok(())
}

fn update_sbom(keg: &Path, time: u64, supplement: Option<&Value>) -> Result<()> {
    let path = keg.join("sbom.spdx.json");
    let mut sbom: Value =
        serde_json::from_slice(&read_required_regular_file(&path, "bottle SBOM")?)?;
    let creation = sbom
        .get_mut("creationInfo")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| eyre::eyre!("bottle SBOM has no creationInfo object"))?;
    let created = chrono::DateTime::from_timestamp(time.try_into()?, 0)
        .ok_or_else(|| eyre::eyre!("invalid SBOM creation timestamp"))?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    creation.insert("created".to_string(), Value::String(created));
    creation.insert(
        "creators".to_string(),
        json!([format!(
            "Tool: https://github.com/Homebrew/brew@{EMULATED_BREW_VERSION}"
        )]),
    );
    if let Some(supplement) = supplement {
        let sbom_object = sbom
            .as_object_mut()
            .ok_or_else(|| eyre::eyre!("bottle SBOM is not an object"))?;
        let supplement = supplement
            .as_object()
            .ok_or_else(|| eyre::eyre!("bottle SBOM supplement is not an object"))?;
        for key in ["documentDescribes", "packages", "relationships"] {
            let Some(values) = supplement.get(key).and_then(Value::as_array) else {
                continue;
            };
            sbom_object
                .entry(key.to_string())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| eyre::eyre!("bottle SBOM {key} is not an array"))?
                .extend(values.iter().cloned());
        }
    }
    crate::file::write_atomic(path, serde_json::to_vec_pretty(&sbom)?)
}

fn write_source_sbom(rf: &ResolvedFormula, keg: &Path, time: u64) -> Result<()> {
    let formula = &rf.formula;
    let version = formula
        .versions
        .stable
        .as_deref()
        .ok_or_else(|| eyre::eyre!("brew:{} has no stable version", formula.name))?;
    let source = formula
        .stable_url()
        .ok_or_else(|| eyre::eyre!("brew:{} has no stable source", formula.name))?;
    let checksum = source
        .checksum
        .as_deref()
        .ok_or_else(|| eyre::eyre!("brew:{} source has no checksum", formula.name))?;
    let created = chrono::DateTime::from_timestamp(time.try_into()?, 0)
        .ok_or_else(|| eyre::eyre!("invalid SBOM creation timestamp"))?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let namespace = format!(
        "https://mise.jdx.dev/sbom/brew/{}/{}/{}",
        formula.name, version, checksum
    );
    let sbom = json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": format!("{}-{}", formula.name, version),
        "documentNamespace": namespace,
        "creationInfo": {
            "created": created,
            "creators": [format!("Tool: mise@{}", env!("CARGO_PKG_VERSION"))],
        },
        "documentDescribes": ["SPDXRef-Package"],
        "packages": [{
            "name": formula.name,
            "SPDXID": "SPDXRef-Package",
            "versionInfo": version,
            "downloadLocation": source.url,
            "filesAnalyzed": false,
            "checksums": [{"algorithm": "SHA256", "checksumValue": checksum}],
        }],
    });
    crate::file::write_atomic(
        keg.join("sbom.spdx.json"),
        serde_json::to_vec_pretty(&sbom)?,
    )
}

/// relative symlink target from `link` to `dest`
pub(super) fn relative_target(dest: &Path, link: &Path) -> PathBuf {
    let link_dir = link.parent().unwrap();
    let mut common = 0;
    let dest_parts: Vec<_> = dest.components().collect();
    let link_parts: Vec<_> = link_dir.components().collect();
    while common < dest_parts.len()
        && common < link_parts.len()
        && dest_parts[common] == link_parts[common]
    {
        common += 1;
    }
    let mut out = PathBuf::new();
    for _ in common..link_parts.len() {
        out.push("..");
    }
    for part in &dest_parts[common..] {
        out.push(part);
    }
    out
}

/// May we overwrite `dest`? Only if it's a symlink pointing into our Cellar
/// or opt (i.e. something brew/mise created and can re-create), or anything
/// underneath a directory symlink brew created — brew links a directory it
/// owns entirely as a single symlink, so the regular files and the keg's own
/// symlinks inside are still brew's.
fn can_overwrite(name: &str, dest: &Path) -> Result<bool> {
    let symlink_ancestor = symlink_ancestor(dest)?;
    let meta = match dest.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return match symlink_ancestor {
                Some(ancestor) => points_into_cellar(&ancestor),
                None => Ok(true),
            };
        }
        Err(error) => return Err(error.into()),
        Ok(metadata) => metadata,
    };
    if let Some(ancestor) = symlink_ancestor {
        if !points_into_cellar(&ancestor)? {
            return Ok(false);
        }
        return Ok(resolved_symlink_target_checked(&ancestor)?
            .is_some_and(|target| target.starts_with(prefix::cellar().join(name))));
    }
    if !meta.is_symlink() {
        return Ok(false);
    }
    Ok(
        resolved_symlink_target_checked(dest)?.is_some_and(|target| {
            target.starts_with(prefix::cellar().join(name))
                || target.starts_with(prefix::prefix().join("opt").join(name))
        }),
    )
}

/// Does this symlink point into our Cellar or opt? Resolve the link itself once,
/// then canonicalize its parent so nested relative links retain their final
/// component while using the Cellar's filesystem spelling.
fn points_into_cellar(link: &Path) -> Result<bool> {
    let Some(target) = resolved_symlink_target_checked(link)? else {
        return Ok(false);
    };
    let cellar = canonicalize_or_lexical_missing(&prefix::cellar())?;
    let opt = canonicalize_or_lexical_missing(&prefix::prefix().join("opt"))?;
    Ok(target.starts_with(cellar) || target.starts_with(opt))
}

fn canonicalize_or_lexical_missing(path: &Path) -> Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(lexical_normalize(path)),
        Err(error) => Err(error.into()),
    }
}

/// Resolve one symlink hop relative to its parent without chasing the final component.
fn resolved_symlink_target(link: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(link).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        link.parent()?.join(target)
    };
    Some(resolved_path(&target))
}

fn resolved_symlink_target_checked(link: &Path) -> Result<Option<PathBuf>> {
    let Some(metadata) = metadata_if_exists(link)? else {
        return Ok(None);
    };
    if !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let target = std::fs::read_link(link)?;
    let target = if target.is_absolute() {
        target
    } else {
        link.parent()
            .ok_or_else(|| eyre::eyre!("symlink has no parent: {}", link.display()))?
            .join(target)
    };
    Ok(Some(resolved_path_checked(&target)?))
}

/// Canonicalize the parent of a lexically normalized path while preserving its final component.
fn resolved_path(path: &Path) -> PathBuf {
    let target = lexical_normalize(path);
    match (target.parent(), target.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(name),
        _ => target,
    }
}

fn resolved_path_checked(path: &Path) -> Result<PathBuf> {
    let target = lexical_normalize(path);
    match (target.parent(), target.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(parent) => Ok(parent.join(name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(target),
            Err(error) => Err(error.into()),
        },
        _ => Ok(target),
    }
}

/// Normalize `.` and `..` components without touching the filesystem.
pub(super) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The outermost ancestor of `dest` (strictly below the prefix) that is a
/// symlink pointing into the Cellar — i.e. a directory brew linked wholesale.
fn brew_owned_ancestor(dest: &Path) -> Result<Option<PathBuf>> {
    let Some(ancestor) = symlink_ancestor(dest)? else {
        return Ok(None);
    };
    Ok(points_into_cellar(&ancestor)?.then_some(ancestor))
}

fn symlink_ancestor(dest: &Path) -> Result<Option<PathBuf>> {
    let prefix_path = prefix::prefix();
    let mut ancestors: Vec<&Path> = dest
        .ancestors()
        .skip(1)
        .take_while(|p| *p != prefix_path && p.starts_with(&prefix_path))
        .collect();
    ancestors.reverse(); // outermost first
    for anc in ancestors {
        match anc.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Ok(Some(anc.to_path_buf()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

/// A leaf reached through a Homebrew directory link already satisfies the
/// topology only when that exact ancestor-plus-suffix maps to the expected keg
/// leaf. Merely entering the Cellar is insufficient: another keg or subtree
/// must remain a hard conflict.
fn path_matches_through_brew_owned_ancestor(
    destination: &Path,
    target: &Path,
    ancestor: &Path,
) -> Result<bool> {
    let Some(ancestor_target) = resolved_symlink_target_checked(ancestor)? else {
        return Ok(false);
    };
    let Ok(suffix) = destination.strip_prefix(ancestor) else {
        return Ok(false);
    };
    Ok(resolved_path_checked(&ancestor_target.join(suffix))? == resolved_path_checked(target)?)
}

/// Replace brew-created directory symlinks on the way to `dest` with real
/// directories of symlinks to their old contents — the same expansion brew
/// performs when another keg needs to place files inside a wholesale-linked
/// directory (resolve_any_conflicts). The replacement is fully staged before
/// the symlink is swapped out, so a failure leaves the tree unchanged.
fn materialize_brew_dirs(dest: &Path) -> Result<()> {
    while let Some(link_dir) = brew_owned_ancestor(dest)? {
        let raw_target = std::fs::read_link(&link_dir)?;
        let staging = link_dir.parent().unwrap().join(format!(
            ".mise-materialize-{}-{}",
            link_dir.file_name().unwrap().to_string_lossy(),
            crate::rand::random_string(16)
        ));
        let staged = (|| -> Result<()> {
            std::fs::create_dir(&staging)?;
            // a dangling dir symlink (keg already pruned) has nothing to preserve
            let target = lexical_normalize(&link_dir.parent().unwrap().join(&raw_target));
            if let Some(metadata) = metadata_if_exists(&target)? {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    bail!(
                        "brew-owned directory link target is not a real directory: {}",
                        target.display()
                    );
                }
                stage_materialized_tree(&target, &staging, &link_dir)?;
            }
            Ok(())
        })();
        if let Err(err) = staged {
            let _ = crate::file::remove_all(&staging);
            return Err(err);
        }
        // swap: a directory cannot be renamed over a symlink, so remove the
        // link first; if the rename then fails, put the symlink back
        if let Err(err) = crate::file::remove_file(&link_dir) {
            let _ = crate::file::remove_all(&staging);
            return Err(err);
        }
        if let Err(err) = crate::file::rename(&staging, &link_dir) {
            let _ = crate::file::make_symlink(&raw_target, &link_dir);
            let _ = crate::file::remove_all(&staging);
            return Err(err);
        }
    }
    Ok(())
}

fn stage_materialized_tree(source: &Path, staging: &Path, final_path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let staged = staging.join(entry.file_name());
        let final_child = final_path.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            crate::file::create_dir_all(&staged)?;
            stage_materialized_tree(&entry.path(), &staged, &final_child)?;
        } else if file_type.is_file() || file_type.is_symlink() {
            crate::file::make_symlink(&relative_target(&entry.path(), &final_child), &staged)?;
        } else {
            bail!(
                "cannot materialize unsupported special Homebrew entry: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

/// Create the opt symlink and (unless keg-only) link the keg's public dirs
/// into the prefix. Conflicts are detected before anything is touched, and a
/// failure partway through removes the links already created — the caller
/// rolls the keg back on error, and nothing may be left dangling into it.
pub fn link_keg(name: &str, pkg_version: &str, keg_only: bool) -> Result<()> {
    let prefix_path = prefix::prefix();
    let keg = keg_path(name, pkg_version);
    if keg_only {
        crate::file::write(keg.join(KEG_ONLY_MARKER), "")?;
    }
    // <prefix>/opt/<name> -> ../Cellar/<name>/<version> (always, even keg-only)
    let opt_link = prefix_path.join("opt").join(name);

    let mut repairs = vec![];
    if !symlink_points_to_checked(&opt_link, &keg)? {
        if !can_overwrite(name, &opt_link)? {
            bail!(
                "cannot link {name}: {} already exists and is not owned by this formula",
                opt_link.display()
            );
        }
        repairs.push(topology_repair_link(
            opt_link.clone(),
            topology_previous(&opt_link)?,
            TopologyOperation::Link(keg.clone()),
        )?);
    }
    if keg_only {
        debug!(
            "{name} is keg-only, not linking into {}",
            prefix_path.display()
        );
    } else {
        repairs.extend(plan_public_topology(name, &keg, true).wrap_err_with(|| {
            format!("cannot link {name}: public topology was not created by mise or brew")
        })?);
        let linked = prefix::linked_keg_record(name);
        if !symlink_points_to_checked(&linked, &keg)? {
            if !can_overwrite(name, &linked)? {
                bail!(
                    "cannot link {name}: {} was not created by mise or brew",
                    linked.display()
                );
            }
            repairs.push(topology_repair_link(
                linked.clone(),
                topology_previous(&linked)?,
                TopologyOperation::Link(keg),
            )?);
        }
    }
    apply_topology_repair(&repairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use jdx_tar::{Builder, EntryType, Header};
    use tokio::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    struct BrewPrefixGuard {
        previous: Option<String>,
    }

    struct FormulaStateGuard {
        keg: PathBuf,
    }

    impl FormulaStateGuard {
        fn new(keg: &Path) -> Self {
            Self {
                keg: keg.to_path_buf(),
            }
        }
    }

    impl Drop for FormulaStateGuard {
        fn drop(&mut self) {
            let _ = lifecycle::remove_owned_state(&self.keg);
            let _ = remove_finalization_state(&self.keg);
        }
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

    /// keg with a versioned dylib and its unversioned alias (the relative
    /// symlink chain every brew library bottle ships), plus a header dir
    fn write_lib_keg(prefix: &Path, name: &str, version: &str) -> Result<PathBuf> {
        let keg = prefix.join("Cellar").join(name).join(version);
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(
            keg.join(".brew").join(format!("{name}.rb")),
            format!("class {} < Formula; end\n", name.replace(['-', '@'], "_")),
        )?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            serde_json::to_vec(&json!({
                "homebrew_version": "6.0.17",
                "runtime_dependencies": [],
            }))?,
        )?;
        crate::file::write(keg.join("sbom.spdx.json"), "{}")?;
        crate::file::create_dir_all(keg.join("lib"))?;
        crate::file::write(keg.join("lib").join(format!("lib{name}.1.dylib")), version)?;
        crate::file::make_symlink(
            Path::new(&format!("lib{name}.1.dylib")),
            &keg.join("lib").join(format!("lib{name}.dylib")),
        )?;
        crate::file::create_dir_all(keg.join("include").join(name))?;
        crate::file::write(keg.join("include").join(name).join("header.h"), version)?;
        // keg-internal relative symlink inside the dir brew links wholesale
        crate::file::make_symlink(
            Path::new("header.h"),
            &keg.join("include").join(name).join("alias.h"),
        )?;
        Ok(keg)
    }

    /// link a keg the way real brew does: file symlinks for files whose
    /// parent dir is shared, one directory symlink for a dir the keg owns
    fn brew_style_link(prefix: &Path, name: &str, version: &str) -> Result<()> {
        let cellar_rel = Path::new("../Cellar").join(name).join(version);
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(
            &Path::new("../Cellar").join(name).join(version),
            &prefix.join("opt").join(name),
        )?;
        crate::file::create_dir_all(prefix.join("lib"))?;
        for lib in [format!("lib{name}.dylib"), format!("lib{name}.1.dylib")] {
            crate::file::make_symlink(
                &cellar_rel.join("lib").join(&lib),
                &prefix.join("lib").join(&lib),
            )?;
        }
        crate::file::create_dir_all(prefix.join("include"))?;
        crate::file::make_symlink(
            &cellar_rel.join("include").join(name),
            &prefix.join("include").join(name),
        )?;
        Ok(())
    }

    fn canonical_tempdir() -> Result<(tempfile::TempDir, PathBuf)> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().canonicalize()?;
        Ok((tmp, path))
    }

    #[test]
    fn legacy_bottle_snapshot_evidence_comes_from_verified_archive() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let staging = tmp.path().join("staging/foo/1/.brew");
        crate::file::create_dir_all(&staging)?;
        let snapshot = staging.join("foo.rb");
        crate::file::write(&snapshot, "class Foo < Formula\nend\n")?;
        let expected = crate::hash::file_hash_sha256(&snapshot, None)?;

        let tarball = tmp.path().join("foo.tar.gz");
        let encoder = GzEncoder::new(std::fs::File::create(&tarball)?, Compression::default());
        let mut archive = Builder::new(encoder);
        let contents = std::fs::read(&snapshot)?;
        let mut header = Header::new_gnu(EntryType::File);
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        archive.append_data(&mut header, "foo/1/.brew/foo.rb", contents.as_slice())?;
        archive.into_inner()?.finish()?;
        let bottle_sha256 = crate::hash::file_hash_sha256(&tarball, None)?;
        let bottle = VerifiedArtifact::from_path(&tarball, &bottle_sha256, None)?
            .ok_or_else(|| eyre::eyre!("test bottle checksum unexpectedly mismatched"))?;
        crate::file::write(&tarball, "swapped-cache-entry")?;

        assert_eq!(
            bottle_formula_snapshot_sha256("foo", "1", &bottle)?,
            expected
        );
        assert_eq!(
            bottle_formula_snapshot_sha256("foo", "1", &bottle)?,
            expected
        );
        Ok(())
    }

    fn resolved_formula(name: &str, version: &str) -> ResolvedFormula {
        ResolvedFormula {
            formula: serde_json::from_value(json!({
                "name": name,
                "versions": {"stable": version},
                "bottle": {},
                "urls": {"stable": {"url": "https://example.test/source.tar.gz", "checksum": "abc123"}},
                "ruby_source_path": format!("Formula/{name}.rb"),
                "tap_git_head": "deadbeef"
            }))
            .unwrap(),
            tap_raw_base: None,
            on_request: true,
        }
    }

    fn bottle_tab(version: &str) -> Value {
        json!({
            "homebrew_version": "6.0.17",
            "poured_from_bottle": true,
            "changed_files": ["bin/foo"],
            "source_modified_time": 123,
            "compiler": "bottle-clang",
            "runtime_dependencies": [],
            "built_on": {"os": "TestOS", "os_version": "1", "cpu_family": "test"},
            "source": {"versions": {"stable": version}}
        })
    }

    fn bottle_sbom(name: &str, version: &str) -> Value {
        json!({
            "spdxVersion": "SPDX-2.3",
            "creationInfo": {"created": "2026-01-01T00:00:00Z", "creators": ["Tool: brew"]},
            "packages": [{"name": name, "versionInfo": version}]
        })
    }

    fn write_source_keg(keg: &Path, marker: &str) -> Result<PathBuf> {
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::create_dir_all(keg.join("bin"))?;
        crate::file::write(keg.join(".brew/foo.rb"), "class Foo < Formula; end")?;
        crate::file::write(keg.join("bin/foo"), marker)?;
        Ok(keg.join(".brew/foo.rb"))
    }

    fn source_provenance(snapshot: PathBuf) -> FormulaInstallProvenance {
        FormulaInstallProvenance::SourceBuild {
            formula_snapshot: snapshot,
            compiler: "clang".to_string(),
            built_on: json!({"os": "TestOS", "os_version": "1", "cpu_family": "test"}),
        }
    }

    fn write_installed_formula(
        prefix: &Path,
        name: &str,
        version: &str,
        mise_owned: bool,
        dependencies: &[(&str, &str)],
    ) -> Result<PathBuf> {
        let keg = write_lib_keg(prefix, name, version)?;
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(
            keg.join(".brew").join(format!("{name}.rb")),
            format!("class {} < Formula; end\n", name.replace(['-', '@'], "_")),
        )?;
        crate::file::write(keg.join("sbom.spdx.json"), "{}")?;
        let runtime_dependencies = dependencies
            .iter()
            .map(|(full_name, pkg_version)| {
                json!({"full_name": full_name, "pkg_version": pkg_version})
            })
            .collect::<Vec<_>>();
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            serde_json::to_vec(&json!({
                "homebrew_version": if mise_owned { "5.1.15 (mise)" } else { "6.0.17" },
                "runtime_dependencies": runtime_dependencies,
            }))?,
        )?;
        link_keg(name, version, false)?;
        Ok(keg)
    }

    #[test]
    fn recognizes_only_top_level_keg_only_snapshot_declarations() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let snapshot = tmp.path().join("formula.rb");
        for declaration in [
            "class Foo < Formula\n  keg_only :versioned_formula\nend\n",
            "class Foo < Formula\n  keg_only(:shadowed_by_macos)\nend\n",
        ] {
            crate::file::write(&snapshot, declaration)?;
            assert!(formula_snapshot_declares_keg_only(&snapshot));
        }
        for non_declaration in [
            "class Foo < Formula\n  # keg_only :versioned_formula\nend\n",
            "class Foo < Formula\n    keg_only :versioned_formula\nend\n",
            "class Foo < Formula\n  keg_only?\nend\n",
            "class Foo < Formula\n  value = 'keg_only :versioned_formula'\nend\n",
        ] {
            crate::file::write(&snapshot, non_declaration)?;
            assert!(!formula_snapshot_declares_keg_only(&snapshot));
        }
        let keg = tmp.path().join("Cellar/foo/1.0");
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(keg.join(".brew/foo.rb"), "class Foo < Formula\nend\n")?;
        let external_marker = tmp.path().join("external-keg-only");
        crate::file::write(&external_marker, "")?;
        crate::file::make_symlink(&external_marker, &keg.join(KEG_ONLY_MARKER))?;
        assert!(!keg_is_keg_only("foo", &keg));
        Ok(())
    }

    #[tokio::test]
    async fn native_keg_only_formula_is_healthy_without_mise_marker_or_public_links() -> Result<()>
    {
        use crate::system::packages::{PackageRequest, PackageState, SystemPackageManager};

        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let name = "postgresql@17";
        let version = "17.6";
        let keg = write_lib_keg(&prefix, name, version)?;
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(
            keg.join(".brew/postgresql@17.rb"),
            "class PostgresqlAT17 < Formula\n  keg_only :versioned_formula\nend\n",
        )?;
        crate::file::write(keg.join("sbom.spdx.json"), "{}")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            serde_json::to_vec(&json!({
                "homebrew_version": "6.0.17",
                "runtime_dependencies": [],
            }))?,
        )?;
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(
            &Path::new("../Cellar").join(name).join(version),
            &prefix.join("opt").join(name),
        )?;

        assert!(!keg.join(KEG_ONLY_MARKER).exists());
        assert!(prefix::linked_keg_record(name).symlink_metadata().is_err());
        assert_eq!(linked_state(name), Some((version.to_string(), false)));
        assert_eq!(
            installed_formula_health(name, version).kind,
            FormulaHealthKind::Healthy
        );

        let manager = super::super::BrewManager::new();
        let status = manager
            .installed(&[PackageRequest {
                name: name.to_string(),
                version: Some(version.to_string()),
                tap_url: None,
            }])
            .await?;
        assert_eq!(
            status[0].state,
            PackageState::Installed {
                version: version.to_string()
            }
        );
        Ok(())
    }

    #[test]
    fn archive_bottle_requires_valid_receipt_before_public_mutation() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rf = resolved_formula("foo", "1.0");
        let error = archive_bottle_provenance(&rf, tmp.path()).unwrap_err();
        assert!(error.to_string().contains("no embedded receipt"));

        crate::file::write(tmp.path().join("INSTALL_RECEIPT.json"), "not json")?;
        let error = archive_bottle_provenance(&rf, tmp.path()).unwrap_err();
        assert!(error.to_string().contains("malformed embedded"));
        Ok(())
    }

    #[test]
    fn archive_bottle_receipt_never_queries_local_compiler() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let tmp = tempfile::tempdir()?;
        let rf = resolved_formula("foo", "1.0");
        let keg = tmp.path().join("foo/1.0");
        crate::file::create_dir_all(keg.join(".brew"))?;
        let sbom = bottle_sbom("foo", "1.0");
        crate::file::write(keg.join("sbom.spdx.json"), serde_json::to_vec(&sbom)?)?;
        let previous_path = crate::env::var("PATH").ok();
        crate::env::set_var("PATH", "");
        let result = write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &FormulaInstallProvenance::ArchiveBottle {
                tab: bottle_tab("1.0"),
                sbom,
            },
        );
        match previous_path {
            Some(path) => crate::env::set_var("PATH", path),
            None => crate::env::remove_var("PATH"),
        }
        result?;
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(keg.join("INSTALL_RECEIPT.json"))?)?;
        assert_eq!(receipt["compiler"], "bottle-clang");
        assert_eq!(receipt["built_on"]["os"], "TestOS");
        Ok(())
    }

    #[test]
    fn source_receipt_requires_snapshot_and_writes_sbom() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rf = resolved_formula("foo", "1.0");
        let keg = tmp.path().join("foo/1.0");
        crate::file::create_dir_all(keg.join(".brew"))?;
        let snapshot = keg.join(".brew/foo.rb");
        let provenance = FormulaInstallProvenance::SourceBuild {
            formula_snapshot: snapshot.clone(),
            compiler: "clang".to_string(),
            built_on: json!({"os": "TestOS", "os_version": "1", "cpu_family": "test"}),
        };
        assert!(
            write_receipt(&rf, "test", &keg, &Default::default(), &[], &provenance)
                .unwrap_err()
                .to_string()
                .contains("no verified formula snapshot")
        );
        crate::file::create_dir_all(snapshot.parent().unwrap())?;
        crate::file::write(&snapshot, "class Foo < Formula; end")?;
        write_receipt(&rf, "test", &keg, &Default::default(), &[], &provenance)?;
        assert!(keg.join("INSTALL_RECEIPT.json").is_file());
        let sbom: Value = serde_json::from_slice(&std::fs::read(keg.join("sbom.spdx.json"))?)?;
        assert_eq!(sbom["packages"][0]["name"], "foo");
        Ok(())
    }

    #[test]
    fn oci_and_archive_inputs_write_equivalent_authoritative_metadata() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rf = resolved_formula("foo", "1.0");
        let tab = bottle_tab("1.0");
        let base_sbom = bottle_sbom("foo", "1.0");
        let oci = tmp.path().join("oci");
        let archive = tmp.path().join("archive");
        for keg in [&oci, &archive] {
            crate::file::create_dir_all(keg.join(".brew"))?;
            crate::file::write(keg.join("sbom.spdx.json"), serde_json::to_vec(&base_sbom)?)?;
        }

        write_receipt(
            &rf,
            "test",
            &oci,
            &Default::default(),
            &[],
            &FormulaInstallProvenance::OciBottle {
                tab: tab.clone(),
                sbom: base_sbom.clone(),
                sbom_supplement: None,
            },
        )?;
        write_receipt(
            &rf,
            "test",
            &archive,
            &Default::default(),
            &[],
            &FormulaInstallProvenance::ArchiveBottle {
                tab,
                sbom: base_sbom,
            },
        )?;

        let mut oci_receipt: Value =
            serde_json::from_slice(&std::fs::read(oci.join("INSTALL_RECEIPT.json"))?)?;
        let mut archive_receipt: Value =
            serde_json::from_slice(&std::fs::read(archive.join("INSTALL_RECEIPT.json"))?)?;
        oci_receipt.as_object_mut().unwrap().remove("time");
        archive_receipt.as_object_mut().unwrap().remove("time");
        assert_eq!(oci_receipt, archive_receipt);

        let mut oci_sbom: Value =
            serde_json::from_slice(&std::fs::read(oci.join("sbom.spdx.json"))?)?;
        let mut archive_sbom: Value =
            serde_json::from_slice(&std::fs::read(archive.join("sbom.spdx.json"))?)?;
        oci_sbom.as_object_mut().unwrap().remove("creationInfo");
        archive_sbom.as_object_mut().unwrap().remove("creationInfo");
        assert_eq!(oci_sbom, archive_sbom);
        Ok(())
    }

    #[test]
    fn bottle_receipt_serializes_absent_build_host_as_null() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rf = resolved_formula("foo", "1.0");
        let keg = tmp.path().join("foo/1.0");
        crate::file::create_dir_all(keg.join(".brew"))?;
        let sbom = bottle_sbom("foo", "1.0");
        crate::file::write(keg.join("sbom.spdx.json"), serde_json::to_vec(&sbom)?)?;
        let mut tab = bottle_tab("1.0");
        tab.as_object_mut().unwrap().remove("built_on");
        let provenance = FormulaInstallProvenance::OciBottle {
            tab,
            sbom,
            sbom_supplement: None,
        };

        validate_bottle_provenance(&rf, &provenance)?;
        write_receipt(&rf, "test", &keg, &Default::default(), &[], &provenance)?;

        let receipt: Value =
            serde_json::from_slice(&std::fs::read(keg.join("INSTALL_RECEIPT.json"))?)?;
        assert_eq!(receipt["compiler"], "bottle-clang");
        assert!(receipt["built_on"].is_null());
        Ok(())
    }

    #[tokio::test]
    async fn source_finalizer_installs_shared_state_and_typed_post_install() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let mut rf = resolved_formula("foo", "1.0");
        rf.formula.post_install_steps = vec![json!({
            "type": "copy",
            "source": {"base": "share", "path": "foo/generated"},
            "target": {"base": "pkgetc", "path": "post-install"}
        })];
        let keg = keg_path("foo", "1.0");
        let _state = FormulaStateGuard::new(&keg);
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        let transaction = begin_source_build_transaction(
            "foo",
            "1.0",
            &keg,
            None,
            lifecycle::prepared_identity_sha256(&prepared)?,
        )?;
        let build_incarnation = read_finalization_state(&keg)?
            .and_then(|state| state.build_incarnation)
            .unwrap();
        let snapshot = write_source_keg(&keg, "new")?;
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::create_dir_all(keg.join(".bottle/var/foo"))?;
        crate::file::create_dir_all(keg.join("share/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "etc-default")?;
        crate::file::write(keg.join(".bottle/var/foo/state"), "var-default")?;
        crate::file::write(keg.join("share/foo/generated"), "generated")?;
        let pr = crate::ui::progress_report::QuietReport::new();

        finalize_formula(FormulaFinalizer {
            rf: &rf,
            tag: "test",
            staged_keg: &keg,
            keg: &keg,
            report: &Default::default(),
            closure: &[],
            provenance: source_provenance(snapshot),
            lifecycle: &prepared,
            pr: &pr,
            existing_backup: transaction.existing_backup,
            predecessor_keg: transaction.predecessor_keg,
        })
        .await?;

        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "etc-default"
        );
        assert_eq!(
            crate::file::read_to_string(prefix.join("var/foo/state"))?,
            "var-default"
        );
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/post-install"))?,
            "generated"
        );
        assert!(keg_installed("foo", "1.0"));
        assert!(
            keg.join(FINALIZATION_INCARNATION_MARKER)
                .symlink_metadata()
                .is_err()
        );
        let state = read_finalization_state(&keg)?.unwrap();
        assert_eq!(state.phase, FinalizationPhase::Complete);
        assert_eq!(
            state
                .replacement_identity
                .as_ref()
                .and_then(|identity| match &identity.kind {
                    FinalizationIdentityKind::Mise { incarnation } => Some(incarnation),
                    FinalizationIdentityKind::Native { .. } => None,
                }),
            Some(&build_incarnation)
        );
        Ok(())
    }

    #[tokio::test]
    async fn source_build_retry_removes_only_nonce_bound_partial_and_restores_predecessor()
    -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "1.0");
        let keg = keg_path("foo", "1.0");
        let _state = FormulaStateGuard::new(&keg);
        let snapshot = write_source_keg(&keg, "old")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(snapshot),
        )?;
        let old_receipt = std::fs::read(keg.join("INSTALL_RECEIPT.json"))?;

        let first = begin_source_build_transaction(
            "foo",
            "1.0",
            &keg,
            Some(keg.clone()),
            "test-lifecycle".into(),
        )?;
        let backup = first.existing_backup.as_ref().unwrap();
        let state = read_finalization_state(&keg)?.unwrap();
        let incarnation = state.build_incarnation.clone().unwrap();
        crate::file::write(keg.join("partial-output"), "partial")?;

        let retry = begin_source_build_transaction(
            "foo",
            "1.0",
            &keg,
            Some(keg.clone()),
            "test-lifecycle".into(),
        )?;
        assert_eq!(retry.existing_backup.as_deref(), Some(backup.as_path()));
        assert!(keg.join("partial-output").symlink_metadata().is_err());
        assert_eq!(
            crate::file::read_to_string(keg.join(FINALIZATION_INCARNATION_MARKER))?,
            incarnation
        );
        assert_eq!(crate::file::read_to_string(backup.join("bin/foo"))?, "old");

        rollback_source_build_transaction(&keg)?;
        assert_eq!(crate::file::read_to_string(keg.join("bin/foo"))?, "old");
        assert_eq!(
            std::fs::read(keg.join("INSTALL_RECEIPT.json"))?,
            old_receipt
        );
        assert!(backup.symlink_metadata().is_err());
        assert!(read_finalization_state(&keg)?.is_none());
        assert!(
            keg.join(FINALIZATION_INCARNATION_MARKER)
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn source_build_retry_preserves_foreign_replacement() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "1.0");
        let keg = keg_path("foo", "1.0");
        let _state = FormulaStateGuard::new(&keg);
        let snapshot = write_source_keg(&keg, "old")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(snapshot),
        )?;
        let first = begin_source_build_transaction(
            "foo",
            "1.0",
            &keg,
            Some(keg.clone()),
            "test-lifecycle".into(),
        )?;
        let backup = first.existing_backup.unwrap();
        crate::file::remove_all(&keg)?;
        crate::file::create_dir_all(&keg)?;
        crate::file::write(keg.join("foreign"), "native replacement")?;

        let error = begin_source_build_transaction(
            "foo",
            "1.0",
            &keg,
            Some(keg.clone()),
            "test-lifecycle".into(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not owned by the active transaction")
        );
        assert_eq!(
            crate::file::read_to_string(keg.join("foreign"))?,
            "native replacement"
        );
        assert_eq!(crate::file::read_to_string(backup.join("bin/foo"))?, "old");
        Ok(())
    }

    #[tokio::test]
    async fn finalizer_restores_predecessor_when_linking_fails() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "1.0");
        let keg = keg_path("foo", "1.0");
        let _state = FormulaStateGuard::new(&keg);
        let old_snapshot = write_source_keg(&keg, "old")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(old_snapshot),
        )?;
        let backup = backup_existing_keg(&keg)?.unwrap();
        let snapshot = write_source_keg(&keg, "new")?;
        crate::file::create_dir_all(prefix.join("bin"))?;
        crate::file::write(prefix.join("bin/foo"), "foreign")?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        let pr = crate::ui::progress_report::QuietReport::new();

        let error = finalize_formula(FormulaFinalizer {
            rf: &rf,
            tag: "test",
            staged_keg: &keg,
            keg: &keg,
            report: &Default::default(),
            closure: &[],
            provenance: source_provenance(snapshot),
            lifecycle: &prepared,
            pr: &pr,
            existing_backup: Some(backup.clone()),
            predecessor_keg: Some(keg.clone()),
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains("cannot link foo"));
        assert_eq!(crate::file::read_to_string(keg.join("bin/foo"))?, "old");
        assert_eq!(
            crate::file::read_to_string(prefix.join("bin/foo"))?,
            "foreign"
        );
        assert!(backup.symlink_metadata().is_err());
        assert!(
            keg.join(FINALIZATION_INCARNATION_MARKER)
                .symlink_metadata()
                .is_err()
        );
        assert!(!finalization_needs_repair(&keg));
        Ok(())
    }

    #[tokio::test]
    async fn identity_preparation_failure_restores_live_predecessor() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "1.0");
        let keg = keg_path("foo", "1.0");
        let _state = FormulaStateGuard::new(&keg);
        let backup = recovery_backup_path(&keg)?;
        write_source_keg(&backup, "old-without-receipt")?;
        let snapshot = write_source_keg(&keg, "new")?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        let pr = crate::ui::progress_report::QuietReport::new();

        let error = finalize_formula(FormulaFinalizer {
            rf: &rf,
            tag: "test",
            staged_keg: &keg,
            keg: &keg,
            report: &Default::default(),
            closure: &[],
            provenance: source_provenance(snapshot),
            lifecycle: &prepared,
            pr: &pr,
            existing_backup: Some(backup.clone()),
            predecessor_keg: Some(keg.clone()),
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains("receipt"));
        assert_eq!(
            crate::file::read_to_string(keg.join("bin/foo"))?,
            "old-without-receipt"
        );
        assert!(
            keg.join(FINALIZATION_INCARNATION_MARKER)
                .symlink_metadata()
                .is_err()
        );
        assert!(backup.symlink_metadata().is_err());
        assert!(read_finalization_state(&keg)?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn finalizer_retains_recoverable_state_after_shared_mutation_failure() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let mut rf = resolved_formula("foo", "1.0");
        rf.formula.post_install_steps = vec![json!({
            "type": "copy",
            "source": {"base": "share", "path": "foo/missing"},
            "target": {"base": "pkgetc", "path": "post-install"}
        })];
        let keg = keg_path("foo", "1.0");
        let _state = FormulaStateGuard::new(&keg);
        let old_snapshot = write_source_keg(&keg, "old")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(old_snapshot),
        )?;
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "old-default")?;
        crate::file::create_dir_all(prefix.join("etc/foo"))?;
        crate::file::write(prefix.join("etc/foo/config"), "old-default")?;
        let backup = backup_existing_keg(&keg)?.unwrap();
        let snapshot = write_source_keg(&keg, "new")?;
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "new-default")?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        let pr = crate::ui::progress_report::QuietReport::new();

        let _error = finalize_formula(FormulaFinalizer {
            rf: &rf,
            tag: "test",
            staged_keg: &keg,
            keg: &keg,
            report: &Default::default(),
            closure: &[],
            provenance: source_provenance(snapshot),
            lifecycle: &prepared,
            pr: &pr,
            existing_backup: Some(backup.clone()),
            predecessor_keg: Some(keg.clone()),
        })
        .await
        .unwrap_err();

        assert_eq!(crate::file::read_to_string(keg.join("bin/foo"))?, "new");
        assert_eq!(crate::file::read_to_string(backup.join("bin/foo"))?, "old");
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "new-default"
        );
        assert!(finalization_needs_repair(&keg));
        assert!(lifecycle::needs_repair(&keg));
        let error = complete_interrupted_finalization(&keg).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("lifecycle execution has an unknown outcome")
        );
        Ok(())
    }

    #[tokio::test]
    async fn finalizer_commits_completed_lifecycle_without_replay() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "2.0");
        let keg = keg_path("foo", "2.0");
        let _state = FormulaStateGuard::new(&keg);
        let snapshot = write_source_keg(&keg, "new")?;
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "new-default")?;
        let backup = recovery_backup_path(&keg)?;
        let backup_snapshot = write_source_keg(&backup, "old")?;
        write_receipt(
            &rf,
            "test",
            &backup,
            &Default::default(),
            &[],
            &source_provenance(backup_snapshot),
        )?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(snapshot),
        )?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        lifecycle::install(&prepared, Some(&backup)).await?;
        crate::file::write(prefix.join("etc/foo/config"), "user-after-install")?;
        let replacement_identity = capture_finalization_install_identity("foo", &keg, true)?;
        let predecessor_identity = capture_finalization_install_identity("foo", &backup, false)?;
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Linked,
                predecessor_keg: Some(backup.clone()),
                replacement_identity: Some(replacement_identity),
                predecessor_identity: Some(predecessor_identity),
                lifecycle_predecessor_identity: None,
                receipt_identity: None,
                receipt_current: Some(ReceiptCurrent::Replacement),
                build_incarnation: None,
                previous_finalization_state: None,
                lifecycle_identity_sha256: None,
                build_root_identity: None,
                quiesced_links: vec![],
            },
        )?;

        assert!(complete_interrupted_finalization(&keg)?);
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "user-after-install"
        );
        assert!(backup.symlink_metadata().is_err());
        assert!(!finalization_needs_repair(&keg));
        Ok(())
    }

    #[tokio::test]
    async fn finalizer_retry_uses_durable_original_predecessor() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "2.0");
        let old_keg = keg_path("foo", "1.0");
        let keg = keg_path("foo", "2.0");
        let _state = FormulaStateGuard::new(&keg);

        let old_snapshot = write_source_keg(&old_keg, "old")?;
        write_receipt(
            &resolved_formula("foo", "1.0"),
            "test",
            &old_keg,
            &Default::default(),
            &[],
            &source_provenance(old_snapshot),
        )?;
        crate::file::create_dir_all(old_keg.join(".bottle/etc/foo"))?;
        crate::file::write(old_keg.join(".bottle/etc/foo/config"), "old-default")?;
        write_source_keg(&keg, "interrupted")?;
        link_keg("foo", "2.0", false)?;
        crate::file::create_dir_all(prefix.join("etc/foo"))?;
        crate::file::write(prefix.join("etc/foo/config"), "old-default")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(keg.join(".brew/foo.rb")),
        )?;
        let replacement_identity = capture_finalization_install_identity("foo", &keg, true)?;
        let lifecycle_predecessor_identity =
            capture_finalization_install_identity("foo", &old_keg, false)?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Linked,
                predecessor_keg: Some(old_keg.clone()),
                replacement_identity: Some(replacement_identity),
                predecessor_identity: None,
                lifecycle_predecessor_identity: Some(lifecycle_predecessor_identity),
                receipt_identity: None,
                receipt_current: Some(ReceiptCurrent::Replacement),
                build_incarnation: None,
                previous_finalization_state: None,
                lifecycle_identity_sha256: Some(lifecycle::prepared_identity_sha256(&prepared)?),
                build_root_identity: None,
                quiesced_links: vec![],
            },
        )?;

        let staged = keg.parent().unwrap().join(".mise-tmp-2.0");
        let snapshot = write_source_keg(&staged, "retry")?;
        crate::file::create_dir_all(staged.join(".bottle/etc/foo"))?;
        crate::file::write(staged.join(".bottle/etc/foo/config"), "new-default")?;
        let pr = crate::ui::progress_report::QuietReport::new();

        finalize_formula(FormulaFinalizer {
            rf: &rf,
            tag: "test",
            staged_keg: &staged,
            keg: &keg,
            report: &Default::default(),
            closure: &[],
            provenance: source_provenance(snapshot),
            lifecycle: &prepared,
            pr: &pr,
            existing_backup: None,
            predecessor_keg: Some(keg.clone()),
        })
        .await?;

        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "new-default"
        );
        assert!(
            prefix
                .join("etc/foo/config.default")
                .symlink_metadata()
                .is_err()
        );
        assert!(recovery_backup_path(&keg)?.symlink_metadata().is_err());
        assert!(!finalization_needs_repair(&keg));
        Ok(())
    }

    #[tokio::test]
    async fn finalizer_rejects_stale_backup_without_state_before_mutation() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "2.0");
        let keg = keg_path("foo", "2.0");
        let _state = FormulaStateGuard::new(&keg);
        let current_snapshot = write_source_keg(&keg, "native-current")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(current_snapshot),
        )?;
        let backup = recovery_backup_path(&keg)?;
        let backup_snapshot = write_source_keg(&backup, "stale-backup")?;
        write_receipt(
            &rf,
            "test",
            &backup,
            &Default::default(),
            &[],
            &source_provenance(backup_snapshot),
        )?;
        let staged = keg.parent().unwrap().join(".mise-tmp-2.0");
        let snapshot = write_source_keg(&staged, "new")?;
        let prepared = lifecycle::prepare(&rf.formula, &staged)?;
        let pr = crate::ui::progress_report::QuietReport::new();

        let error = finalize_formula(FormulaFinalizer {
            rf: &rf,
            tag: "test",
            staged_keg: &staged,
            keg: &keg,
            report: &Default::default(),
            closure: &[],
            provenance: source_provenance(snapshot),
            lifecycle: &prepared,
            pr: &pr,
            existing_backup: None,
            predecessor_keg: Some(keg.clone()),
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains("stale recovery backup"));
        assert_eq!(
            crate::file::read_to_string(keg.join("bin/foo"))?,
            "native-current"
        );
        assert_eq!(
            crate::file::read_to_string(backup.join("bin/foo"))?,
            "stale-backup"
        );
        assert!(
            staged
                .join("INSTALL_RECEIPT.json")
                .symlink_metadata()
                .is_err()
        );
        assert!(read_finalization_state(&keg)?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn stale_bound_backup_never_deletes_native_replacement() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let rf = resolved_formula("foo", "2.0");
        let keg = keg_path("foo", "2.0");
        let _state = FormulaStateGuard::new(&keg);
        let snapshot = write_source_keg(&keg, "transaction")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(snapshot),
        )?;
        let replacement_identity = capture_finalization_install_identity("foo", &keg, true)?;
        let backup = recovery_backup_path(&keg)?;
        let backup_snapshot = write_source_keg(&backup, "predecessor")?;
        write_receipt(
            &rf,
            "test",
            &backup,
            &Default::default(),
            &[],
            &source_provenance(backup_snapshot),
        )?;
        let predecessor_identity = capture_finalization_install_identity("foo", &backup, false)?;
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Receipt,
                predecessor_keg: Some(backup.clone()),
                replacement_identity: Some(replacement_identity),
                predecessor_identity: Some(predecessor_identity),
                lifecycle_predecessor_identity: None,
                receipt_identity: None,
                receipt_current: Some(ReceiptCurrent::Replacement),
                build_incarnation: None,
                previous_finalization_state: None,
                lifecycle_identity_sha256: None,
                build_root_identity: None,
                quiesced_links: vec![],
            },
        )?;
        crate::file::remove_all(&keg)?;
        let native_snapshot = write_source_keg(&keg, "native-replacement")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(native_snapshot),
        )?;

        let error = backup_existing_keg(&keg).unwrap_err();

        assert!(error.to_string().contains("identity no longer matches"));
        assert_eq!(
            crate::file::read_to_string(keg.join("bin/foo"))?,
            "native-replacement"
        );
        assert_eq!(
            crate::file::read_to_string(backup.join("bin/foo"))?,
            "predecessor"
        );
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_finalizer_rejects_replaced_lifecycle_predecessor() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let old_keg = keg_path("foo", "1.0");
        let keg = keg_path("foo", "2.0");
        let _state = FormulaStateGuard::new(&keg);
        let old_rf = resolved_formula("foo", "1.0");
        let old_snapshot = write_source_keg(&old_keg, "old")?;
        write_receipt(
            &old_rf,
            "test",
            &old_keg,
            &Default::default(),
            &[],
            &source_provenance(old_snapshot),
        )?;
        let rf = resolved_formula("foo", "2.0");
        let snapshot = write_source_keg(&keg, "new")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(snapshot),
        )?;
        let replacement_identity = capture_finalization_install_identity("foo", &keg, true)?;
        let lifecycle_predecessor_identity =
            capture_finalization_install_identity("foo", &old_keg, false)?;
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Linked,
                predecessor_keg: Some(old_keg.clone()),
                replacement_identity: Some(replacement_identity),
                predecessor_identity: None,
                lifecycle_predecessor_identity: Some(lifecycle_predecessor_identity),
                receipt_identity: None,
                receipt_current: Some(ReceiptCurrent::Replacement),
                build_incarnation: None,
                previous_finalization_state: None,
                lifecycle_identity_sha256: None,
                build_root_identity: None,
                quiesced_links: vec![],
            },
        )?;
        crate::file::create_dir_all(prefix.join("etc/foo"))?;
        crate::file::write(prefix.join("etc/foo/config"), "user")?;
        crate::file::remove_all(&old_keg)?;
        let replacement_snapshot = write_source_keg(&old_keg, "native-replacement")?;
        write_receipt(
            &old_rf,
            "test",
            &old_keg,
            &Default::default(),
            &[],
            &source_provenance(replacement_snapshot),
        )?;

        let error = complete_interrupted_finalization(&keg).unwrap_err();

        assert!(error.to_string().contains("identity no longer matches"));
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "user"
        );
        assert_eq!(
            crate::file::read_to_string(old_keg.join("bin/foo"))?,
            "native-replacement"
        );
        Ok(())
    }

    #[tokio::test]
    async fn completed_lifecycle_no_longer_trusts_old_version_predecessor() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let old_keg = keg_path("foo", "1.0");
        let keg = keg_path("foo", "2.0");
        let _state = FormulaStateGuard::new(&keg);
        let old_rf = resolved_formula("foo", "1.0");
        let old_snapshot = write_source_keg(&old_keg, "old")?;
        write_receipt(
            &old_rf,
            "test",
            &old_keg,
            &Default::default(),
            &[],
            &source_provenance(old_snapshot),
        )?;
        let rf = resolved_formula("foo", "2.0");
        let snapshot = write_source_keg(&keg, "new")?;
        write_receipt(
            &rf,
            "test",
            &keg,
            &Default::default(),
            &[],
            &source_provenance(snapshot),
        )?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
        lifecycle::install(&prepared, Some(&old_keg)).await?;
        let replacement_identity = capture_finalization_install_identity("foo", &keg, true)?;
        let lifecycle_predecessor_identity =
            capture_finalization_install_identity("foo", &old_keg, false)?;
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Linked,
                predecessor_keg: Some(old_keg.clone()),
                replacement_identity: Some(replacement_identity),
                predecessor_identity: None,
                lifecycle_predecessor_identity: Some(lifecycle_predecessor_identity),
                receipt_identity: None,
                receipt_current: Some(ReceiptCurrent::Replacement),
                build_incarnation: None,
                previous_finalization_state: None,
                lifecycle_identity_sha256: None,
                build_root_identity: None,
                quiesced_links: vec![],
            },
        )?;
        crate::file::remove_all(&old_keg)?;
        let replacement_snapshot = write_source_keg(&old_keg, "native-replacement")?;
        write_receipt(
            &old_rf,
            "test",
            &old_keg,
            &Default::default(),
            &[],
            &source_provenance(replacement_snapshot),
        )?;

        assert!(complete_interrupted_finalization(&keg)?);

        assert_eq!(
            crate::file::read_to_string(old_keg.join("bin/foo"))?,
            "native-replacement"
        );
        assert!(
            keg.join(FINALIZATION_INCARNATION_MARKER)
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    /// the unversioned dylib alias resolves through a relative symlink chain
    /// inside the Cellar and must still be recognized as brew's
    #[test]
    fn test_upgrade_over_brew_file_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        write_lib_keg(&prefix, "foo", "2.0")?;

        link_keg("foo", "2.0", false)?;

        let lib_link = prefix.join("lib").join("libfoo.dylib");
        assert!(lib_link.symlink_metadata()?.is_symlink());
        assert_eq!(std::fs::read_to_string(&lib_link)?, "2.0");
        Ok(())
    }

    /// everything under a brew directory-level symlink is brew's and must
    /// relink without conflicts or modifying the old keg
    #[test]
    fn test_upgrade_over_brew_dir_symlink() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let old_keg = write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        write_lib_keg(&prefix, "foo", "2.0")?;

        link_keg("foo", "2.0", false)?;

        assert!(prefix.join("include/foo").is_symlink());
        assert_eq!(
            std::fs::read_link(prefix.join("include/foo"))?,
            PathBuf::from("../Cellar/foo/2.0/include/foo")
        );
        let header = prefix.join("include").join("foo").join("header.h");
        assert_eq!(std::fs::read_to_string(&header)?, "2.0");
        // a keg-internal relative symlink under the dir symlink is brew's too
        assert_eq!(
            std::fs::read_to_string(prefix.join("include").join("foo").join("alias.h"))?,
            "2.0"
        );
        // the old keg's own files survive untouched
        assert_eq!(
            std::fs::read_to_string(old_keg.join("include").join("foo").join("header.h"))?,
            "1.0"
        );
        Ok(())
    }

    /// a link into the Cellar whose target continues outside it (bottles
    /// ship symlinks to system libraries) is still brew's own link
    #[test]
    fn test_upgrade_over_link_whose_cellar_target_leaves_the_cellar() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        for version in ["1.0", "2.0"] {
            let keg = prefix.join("Cellar").join("foo").join(version);
            crate::file::create_dir_all(keg.join("lib"))?;
            crate::file::make_symlink(
                Path::new("/usr/lib/libSystem.B.dylib"),
                &keg.join("lib").join("libsys.dylib"),
            )?;
        }
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/foo/1.0"),
            &prefix.join("opt").join("foo"),
        )?;
        crate::file::create_dir_all(prefix.join("lib"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/foo/1.0/lib/libsys.dylib"),
            &prefix.join("lib").join("libsys.dylib"),
        )?;

        link_keg("foo", "2.0", false)?;

        assert_eq!(
            std::fs::read_link(prefix.join("lib").join("libsys.dylib"))?,
            PathBuf::from("../Cellar/foo/2.0/lib/libsys.dylib")
        );
        Ok(())
    }

    /// a regular file that is NOT under a brew directory symlink is foreign
    /// and must still be reported as a conflict
    #[test]
    fn test_foreign_regular_file_still_conflicts() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "2.0")?;
        crate::file::create_dir_all(prefix.join("include").join("foo"))?;
        crate::file::write(prefix.join("include").join("foo").join("header.h"), "mine")?;

        let err = link_keg("foo", "2.0", false).unwrap_err();
        assert!(err.to_string().contains("not created by mise or brew"));
        assert_eq!(
            std::fs::read_to_string(prefix.join("include").join("foo").join("header.h"))?,
            "mine"
        );
        Ok(())
    }

    /// a shared dir linked wholesale to another keg is expanded into a real
    /// directory keeping that keg's entries visible, like brew does
    #[test]
    fn test_materialize_shared_dir_owned_by_other_keg() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        // other keg owns share/xml via a dir symlink
        let other = prefix.join("Cellar").join("other").join("1.0");
        crate::file::create_dir_all(other.join("share").join("xml"))?;
        crate::file::write(other.join("share").join("xml").join("other.dtd"), "other")?;
        crate::file::create_dir_all(other.join("share/xml/schema/nested"))?;
        crate::file::write(other.join("share/xml/schema/nested/other.xsd"), "nested")?;
        crate::file::create_dir_all(prefix.join("share"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/other/1.0/share/xml"),
            &prefix.join("share").join("xml"),
        )?;
        // new keg wants a file inside share/xml
        let keg = prefix.join("Cellar").join("foo").join("2.0");
        crate::file::create_dir_all(keg.join("share").join("xml"))?;
        crate::file::write(keg.join("share").join("xml").join("foo.dtd"), "foo")?;

        link_keg("foo", "2.0", false)?;

        let xml = prefix.join("share").join("xml");
        assert!(!xml.symlink_metadata()?.is_symlink());
        assert_eq!(std::fs::read_to_string(xml.join("other.dtd"))?, "other");
        assert_eq!(std::fs::read_to_string(xml.join("foo.dtd"))?, "foo");
        assert!(xml.join("schema").is_dir());
        assert!(!xml.join("schema").symlink_metadata()?.is_symlink());
        assert!(xml.join("schema/nested").is_dir());
        assert!(!xml.join("schema/nested").symlink_metadata()?.is_symlink());
        assert!(xml.join("schema/nested/other.xsd").is_symlink());
        // the other keg must not have been polluted
        assert!(!other.join("share").join("xml").join("foo.dtd").exists());
        Ok(())
    }

    #[test]
    fn fresh_link_uses_whole_directories_and_mkpath_policy() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/foo/1.0");
        crate::file::create_dir_all(keg.join("share/private-empty"))?;
        crate::file::create_dir_all(keg.join("share/man"))?;
        crate::file::create_dir_all(prefix.join("share"))?;

        link_keg("foo", "1.0", false)?;

        assert!(prefix.join("share/private-empty").is_symlink());
        assert!(prefix.join("share/man").is_dir());
        assert!(!prefix.join("share/man").symlink_metadata()?.is_symlink());
        Ok(())
    }

    #[test]
    fn existing_real_directory_is_traversed_into_leaf_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/foo/1.0");
        crate::file::create_dir_all(keg.join("share/custom"))?;
        crate::file::write(keg.join("share/custom/value"), "foo")?;
        crate::file::create_dir_all(prefix.join("share/custom"))?;

        link_keg("foo", "1.0", false)?;

        assert!(!prefix.join("share/custom").symlink_metadata()?.is_symlink());
        assert!(prefix.join("share/custom/value").is_symlink());
        Ok(())
    }

    #[test]
    fn repair_rollback_never_removes_preexisting_real_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let existing = tmp.path().join("existing");
        crate::file::create_dir_all(&existing)?;
        let blocked_parent = tmp.path().join("blocked");
        crate::file::write(&blocked_parent, "foreign")?;
        let repairs = vec![
            TopologyRepairLink {
                destination: existing.clone(),
                previous: TopologyPrevious::ExistingDirectory,
                operation: TopologyOperation::Directory,
                ancestors: vec![],
            },
            TopologyRepairLink {
                destination: blocked_parent.join("leaf"),
                previous: TopologyPrevious::Absent,
                operation: TopologyOperation::Link(tmp.path().join("target")),
                ancestors: vec![],
            },
        ];

        assert!(apply_topology_repair(&repairs).is_err());
        assert!(existing.is_dir());
        Ok(())
    }

    #[test]
    fn topology_apply_rejects_destination_changed_after_preflight() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        crate::file::write(&target, "target")?;
        let destination = tmp.path().join("destination");
        let repairs = vec![TopologyRepairLink {
            destination: destination.clone(),
            previous: TopologyPrevious::Absent,
            operation: TopologyOperation::Link(target),
            ancestors: vec![],
        }];
        crate::file::write(&destination, "foreign-after-preflight")?;

        let error = apply_topology_repair(&repairs).unwrap_err();

        assert!(error.to_string().contains("changed after preflight"));
        assert_eq!(
            crate::file::read_to_string(destination)?,
            "foreign-after-preflight"
        );
        Ok(())
    }

    #[test]
    fn topology_apply_rejects_ancestor_replaced_after_preflight() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/foo/1.0");
        crate::file::create_dir_all(keg.join("share/example"))?;
        crate::file::write(keg.join("share/example/value"), "keg")?;
        let share = prefix.join("share");
        crate::file::create_dir_all(&share)?;
        let repairs = plan_public_topology("foo", &keg, true)?;

        let original_share = prefix.join("share-original");
        crate::file::rename(&share, &original_share)?;
        let outside = prefix.parent().unwrap().join("outside-share");
        crate::file::create_dir_all(&outside)?;
        crate::file::make_symlink(&outside, &share)?;

        let error = apply_topology_repair(&repairs).unwrap_err();

        assert!(error.to_string().contains("ancestor changed"));
        assert!(outside.read_dir()?.next().is_none());
        Ok(())
    }

    #[test]
    fn source_build_quiesces_and_rollback_restores_same_version_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1.0", false, &[])?;
        let _state = FormulaStateGuard::new(&keg);
        let public = prefix.join("lib/libfoo.1.dylib");
        assert!(prefix.join("opt/foo").is_symlink());
        assert!(prefix::linked_keg_record("foo").is_symlink());
        assert!(public.is_symlink());

        let transaction = begin_source_build_transaction(
            "foo",
            "1.0",
            &keg,
            Some(keg.clone()),
            "test-lifecycle".into(),
        )?;

        assert!(transaction.existing_backup.is_some());
        assert!(prefix.join("opt/foo").symlink_metadata().is_err());
        assert!(prefix::linked_keg_record("foo").symlink_metadata().is_err());
        assert!(public.symlink_metadata().is_err());
        rollback_source_build_transaction(&keg)?;
        assert!(prefix.join("opt/foo").is_symlink());
        assert!(prefix::linked_keg_record("foo").is_symlink());
        assert!(public.is_symlink());
        assert_eq!(crate::file::read_to_string(&public)?, "1.0");
        Ok(())
    }

    #[test]
    fn source_build_never_quiesces_through_foreign_link_parent() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_lib_keg(&prefix, "foo", "1.0")?;
        crate::file::create_dir_all(prefix.join("var"))?;
        let outside = prefix.parent().unwrap().join("outside-linked");
        crate::file::create_dir_all(outside.join("linked"))?;
        crate::file::make_symlink(&outside, &prefix.join("var/homebrew"))?;
        crate::file::make_symlink(&keg, &outside.join("linked/foo"))?;

        let error = links_resolving_into_keg("foo", &keg).unwrap_err();

        assert!(error.to_string().contains("not a real directory"));
        assert_eq!(std::fs::read_link(outside.join("linked/foo"))?, keg);
        Ok(())
    }

    #[test]
    fn formula_conflicts_require_both_native_active_records() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "bar", "1.0")?;
        link_keg("bar", "1.0", false)?;
        let formula: super::super::api::Formula = serde_json::from_value(json!({
            "name": "foo",
            "versions": {"stable": "1.0"},
            "conflicts_with": ["bar"],
            "conflicts_with_reasons": ["same command"],
        }))?;

        let error = validate_formula_install_policy(&formula).unwrap_err();
        assert!(error.to_string().contains("same command"));

        crate::file::remove_file(prefix.join("opt/bar"))?;
        validate_formula_install_policy(&formula)?;
        crate::file::make_symlink(Path::new("../Cellar/bar/1.0"), &prefix.join("opt/bar"))?;
        crate::file::remove_file(prefix::linked_keg_record("bar"))?;
        validate_formula_install_policy(&formula)?;
        crate::file::make_symlink(
            Path::new("../../../Cellar/bar/1.0"),
            &prefix::linked_keg_record("bar"),
        )?;

        let keg_only: super::super::api::Formula = serde_json::from_value(json!({
            "name": "foo",
            "versions": {"stable": "1.0"},
            "keg_only": true,
            "conflicts_with": ["bar"],
        }))?;
        validate_formula_install_policy(&keg_only)?;
        Ok(())
    }

    #[test]
    fn malformed_formula_paths_fail_before_touching_outside_prefix() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let outside = tmp.path().join("outside");
        crate::file::create_dir_all(&outside)?;
        crate::file::write(outside.join("sentinel"), "untouched")?;

        for (name, version) in [
            ("../outside", "1.0"),
            ("/tmp/outside", "1.0"),
            ("safe/name", "1.0"),
            ("safe\\name", "1.0"),
            ("safe", "../outside"),
            ("safe", "1/2"),
            ("safe", "1\\2"),
        ] {
            let formula: super::super::api::Formula = serde_json::from_value(json!({
                "name": name,
                "versions": {"stable": version},
            }))?;
            assert!(validate_formula_install_policy(&formula).is_err());
        }

        assert_eq!(
            crate::file::read_to_string(outside.join("sentinel"))?,
            "untouched"
        );
        Ok(())
    }

    #[test]
    fn retry_rollback_keeps_original_state_instead_of_nesting_incomplete_state() -> Result<()> {
        let original = br#"{"phase":"complete"}"#.to_vec();
        let current: FinalizationState = serde_json::from_value(json!({
            "formula": "foo",
            "version": "1.0",
            "provenance": "source_build",
            "phase": "keg",
            "previous_finalization_state": original,
        }))?;
        let immediate = serde_json::to_vec(&current)?;

        assert_eq!(
            original_finalization_state_bytes(Some(&current), Some(immediate)),
            current.previous_finalization_state
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn special_public_keg_entries_fail_before_link_mutation() -> Result<()> {
        use std::os::unix::net::UnixListener;

        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/foo/1.0");
        crate::file::create_dir_all(keg.join("share"))?;
        let _socket = UnixListener::bind(keg.join("share/socket"))?;

        let error = link_keg("foo", "1.0", false).unwrap_err();

        assert!(error.to_string().contains("not created by mise or brew"));
        assert!(prefix.join("opt/foo").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn cross_formula_existing_leaf_conflicts_without_mutation() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let other = prefix.join("Cellar/other/1.0/share/xml");
        crate::file::create_dir_all(&other)?;
        crate::file::write(other.join("same.dtd"), "other")?;
        crate::file::create_dir_all(prefix.join("share"))?;
        crate::file::make_symlink(
            Path::new("../Cellar/other/1.0/share/xml"),
            &prefix.join("share/xml"),
        )?;
        let keg = prefix.join("Cellar/foo/1.0");
        crate::file::create_dir_all(keg.join("share/xml"))?;
        crate::file::write(keg.join("share/xml/same.dtd"), "foo")?;

        let error = link_keg("foo", "1.0", false).unwrap_err();

        assert!(error.to_string().contains("not created by mise or brew"));
        assert!(prefix.join("share/xml").symlink_metadata()?.is_symlink());
        assert_eq!(
            crate::file::read_to_string(other.join("same.dtd"))?,
            "other"
        );
        assert!(prefix.join("opt/foo").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn link_policy_matches_pinned_homebrew_directory_rules() {
        assert_eq!(
            keg_link_policy("bin", Path::new("nested"), true),
            KegLinkPolicy::Skip
        );
        assert_eq!(
            keg_link_policy("share", Path::new("private"), true),
            KegLinkPolicy::Link
        );
        assert_eq!(
            keg_link_policy("include", Path::new("postgresql@x"), true),
            KegLinkPolicy::Link
        );
        assert_eq!(
            keg_link_policy("include", Path::new("postgresql@1beta"), true),
            KegLinkPolicy::Mkpath
        );
        assert_eq!(
            keg_link_policy("share", Path::new("man"), true),
            KegLinkPolicy::Mkpath
        );
        assert_eq!(
            keg_link_policy("share", Path::new("foo/locale/en_bad"), true),
            KegLinkPolicy::Mkpath
        );
        assert_eq!(
            keg_link_policy("share", Path::new("notlocale/en_bad"), true),
            KegLinkPolicy::Mkpath
        );
        assert_eq!(
            keg_link_policy("share", Path::new("postgresql@x"), true),
            KegLinkPolicy::Link
        );
        assert_eq!(
            keg_link_policy("share", Path::new("postgresql@1beta"), true),
            KegLinkPolicy::Mkpath
        );
        assert_eq!(
            keg_link_policy("share", Path::new("foo/info/tool.info"), false),
            KegLinkPolicy::Info
        );
        assert_eq!(
            keg_link_policy("share", Path::new("myinfo/tool.info"), false),
            KegLinkPolicy::Info
        );
        assert_eq!(
            keg_link_policy("share", Path::new("icons/icon-theme.cache"), false),
            KegLinkPolicy::Link
        );
        assert_eq!(
            keg_link_policy("share", Path::new("icons/hicolor/icon-theme.cache"), false),
            KegLinkPolicy::Skip
        );
        assert_eq!(
            keg_link_policy("lib", Path::new("python3.x"), true),
            KegLinkPolicy::Link
        );
        assert_eq!(
            keg_link_policy("lib", Path::new("python3.12beta"), true),
            KegLinkPolicy::Mkpath
        );
    }

    #[test]
    fn test_nested_relative_link_is_brew_owned() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/foo/1.0/lib");
        crate::file::create_dir_all(&keg)?;
        crate::file::write(keg.join("libfoo.1.dylib"), "")?;
        let lib = prefix.join("lib");
        crate::file::create_dir_all(&lib)?;
        crate::file::make_symlink(Path::new("../Cellar/foo/1.0/lib"), &lib.join("foo"))?;
        let nested = lib.join("libfoo.dylib");
        crate::file::make_symlink(Path::new("foo/libfoo.1.dylib"), &nested)?;

        assert!(can_overwrite("foo", &nested)?);
        Ok(())
    }

    #[test]
    fn overwrite_authorization_propagates_path_traversal_errors() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        crate::file::write(prefix.join("blocked"), "file")?;

        let error = can_overwrite("foo", &prefix.join("blocked/leaf")).unwrap_err();

        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .map(std::io::Error::kind),
            Some(std::io::ErrorKind::NotADirectory)
        );
        Ok(())
    }

    #[test]
    fn test_link_keg_maintains_homebrew_linked_record() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        link_keg("foo", "1.0", false)?;
        let linked = prefix::linked_keg_record("foo");
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );

        write_lib_keg(&prefix, "foo", "2.0")?;
        link_keg("foo", "2.0", false)?;
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/2.0")
        );

        let bar_keg = write_lib_keg(&prefix, "bar", "1.0")?;
        link_keg("bar", "1.0", true)?;
        assert!(prefix.join("opt/bar").is_symlink());
        assert!(prefix::linked_keg_record("bar").symlink_metadata().is_err());
        assert_eq!(linked_state("bar"), Some(("1.0".to_string(), false)));
        crate::file::make_symlink(
            &bar_keg.join("lib/libbar.1.dylib"),
            &prefix.join("lib/libbar.1.dylib"),
        )?;
        assert!(!repair_link_record("bar", false)?);
        assert!(prefix::linked_keg_record("bar").symlink_metadata().is_err());

        let linked = prefix::linked_keg_record("bar");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::make_symlink(Path::new("../../../Cellar/bar/1.0"), &linked)?;
        std::fs::remove_file(prefix.join("opt/bar"))?;
        assert_eq!(linked_state("bar"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("bar", false)?);
        assert_eq!(
            std::fs::read_link(prefix.join("opt/bar"))?,
            PathBuf::from("../Cellar/bar/1.0")
        );
        Ok(())
    }

    #[test]
    fn test_repairs_active_records_without_relinking_the_keg() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        let public = prefix.join("lib/libfoo.dylib");
        let public_target = std::fs::read_link(&public)?;

        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(std::fs::read_link(&public)?, public_target);
        assert_eq!(
            std::fs::read_link(prefix::linked_keg_record("foo"))?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );

        crate::file::remove_file(prefix.join("opt/foo"))?;
        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(prefix.join("opt/foo"))?,
            PathBuf::from("../Cellar/foo/1.0")
        );
        assert_eq!(std::fs::read_link(&public)?, public_target);
        Ok(())
    }

    #[test]
    fn test_topology_repair_preserves_matching_brew_directory_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        let linked = prefix::linked_keg_record("foo");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::make_symlink(Path::new("../../../Cellar/foo/1.0"), &linked)?;

        let public_dir = prefix.join("include/foo");
        let public_dir_target = std::fs::read_link(&public_dir)?;
        let public_header = public_dir.join("header.h");
        let public_header_contents = crate::file::read_to_string(&public_header)?;
        crate::file::remove_file(prefix.join("opt/foo"))?;
        crate::file::remove_file(&linked)?;

        let repairs = preflight_topology_repair("foo", "1.0", &keg)?;
        assert_eq!(repairs.len(), 2);
        apply_topology_repair(&repairs)?;

        assert_eq!(std::fs::read_link(&public_dir)?, public_dir_target);
        assert_eq!(
            crate::file::read_to_string(&public_header)?,
            public_header_contents
        );
        assert_eq!(
            std::fs::read_link(prefix.join("opt/foo"))?,
            PathBuf::from("../Cellar/foo/1.0")
        );
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );
        Ok(())
    }

    #[test]
    fn test_topology_repair_rejects_mismatched_brew_directory_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        write_lib_keg(&prefix, "foo", "2.0")?;

        let public_dir = prefix.join("include/foo");
        crate::file::remove_file(&public_dir)?;
        let mismatched_target = PathBuf::from("../Cellar/foo/2.0/include/foo");
        crate::file::make_symlink(&mismatched_target, &public_dir)?;
        crate::file::remove_file(prefix.join("opt/foo"))?;

        let error = preflight_topology_repair("foo", "1.0", &keg).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("topology repair would traverse a directory symlink")
        );
        assert_eq!(std::fs::read_link(&public_dir)?, mismatched_target);
        assert!(prefix.join("opt/foo").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn formula_health_checks_every_expected_public_leaf() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1.0", false, &[])?;
        crate::file::create_dir_all(keg.join("share/custom"))?;
        crate::file::write(keg.join("share/custom/one"), "one")?;
        crate::file::write(keg.join("share/custom/two"), "two")?;
        crate::file::create_dir_all(prefix.join("share/custom"))?;
        link_keg("foo", "1.0", false)?;
        let missing = prefix.join("share/custom/two");
        crate::file::remove_file(&missing)?;

        let health = installed_formula_health("foo", "1.0");
        assert_eq!(health.kind, FormulaHealthKind::Repairable);
        assert!(
            health
                .reasons
                .iter()
                .any(|reason| reason.contains("public keg topology"))
        );

        crate::file::write(&missing, "foreign")?;
        let health = installed_formula_health("foo", "1.0");
        assert_eq!(health.kind, FormulaHealthKind::ReinstallRequired);
        assert!(
            health
                .reasons
                .iter()
                .any(|reason| reason.contains("ambiguous"))
        );
        Ok(())
    }

    #[test]
    fn formula_health_rejects_symlinked_provenance_files() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1.0", false, &[])?;
        let receipt = keg.join("INSTALL_RECEIPT.json");
        let external = prefix.join("receipt-copy.json");
        crate::file::write(&external, std::fs::read(&receipt)?)?;
        crate::file::remove_file(&receipt)?;
        crate::file::make_symlink(&external, &receipt)?;

        let health = installed_formula_health("foo", "1.0");

        assert_eq!(health.kind, FormulaHealthKind::ReinstallRequired);
        assert!(
            health
                .reasons
                .iter()
                .any(|reason| reason.contains("receipt/provenance"))
        );
        Ok(())
    }

    #[test]
    fn test_repairs_dangling_owned_records_but_not_foreign_records() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        brew_style_link(&prefix, "foo", "1.0")?;
        let linked = prefix::linked_keg_record("foo");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::make_symlink(Path::new("../../../Cellar/foo/0.9"), &linked)?;

        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );

        let opt = prefix.join("opt/foo");
        crate::file::make_symlink(Path::new("../Cellar/foo/0.9"), &opt)?;
        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), true)));
        assert!(repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(&opt)?,
            PathBuf::from("../Cellar/foo/1.0")
        );

        crate::file::make_symlink(Path::new("/custom/missing-foo"), &linked)?;
        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), false)));
        assert!(!repair_link_record("foo", false)?);
        assert_eq!(
            std::fs::read_link(&linked)?,
            PathBuf::from("/custom/missing-foo")
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_formula_repair_restores_linked_record_without_repouring() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        use crate::system::packages::{PackageRequest, PackageState, SystemPackageManager};

        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1.0", false, &[])?;
        let _state = FormulaStateGuard::new(&keg);
        crate::file::remove_file(prefix::linked_keg_record("foo"))?;
        let public = prefix.join("lib/libfoo.dylib");
        let request = PackageRequest {
            name: "foo".to_string(),
            version: None,
            tap_url: None,
        };
        let keg_inode = keg.metadata()?.ino();
        let receipt_modified = keg.join("INSTALL_RECEIPT.json").metadata()?.modified()?;
        let public_inode = public.symlink_metadata()?.ino();

        let manager = super::super::BrewManager::new();
        let status = manager.installed(std::slice::from_ref(&request)).await?;
        assert!(matches!(
            &status[0].state,
            PackageState::NeedsRepair { installed, .. } if installed == "1.0"
        ));

        let rf = resolved_formula("foo", "1.0");
        let lifecycle = lifecycle::prepare(&rf.formula, &keg)?;
        let health = installed_formula_health("foo", "1.0");
        assert!(repair_formula(&health, &lifecycle, false).await?);

        assert_eq!(keg.metadata()?.ino(), keg_inode);
        assert_eq!(
            keg.join("INSTALL_RECEIPT.json").metadata()?.modified()?,
            receipt_modified
        );
        assert_eq!(public.symlink_metadata()?.ino(), public_inode);
        assert_eq!(
            std::fs::read_link(prefix::linked_keg_record("foo"))?,
            PathBuf::from("../../../Cellar/foo/1.0")
        );
        assert_eq!(
            manager.installed(std::slice::from_ref(&request)).await?[0].state,
            PackageState::Installed {
                version: "1.0".to_string()
            }
        );
        Ok(())
    }

    #[tokio::test]
    async fn root_status_traverses_and_repairs_legacy_dependency_lifecycle() -> Result<()> {
        use std::os::unix::fs::MetadataExt;

        use crate::system::packages::{PackageRequest, PackageState, SystemPackageManager};

        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let ca = write_installed_formula(&prefix, "ca-certificates", "1", false, &[])?;
        let openssl =
            write_installed_formula(&prefix, "openssl@3", "1", true, &[("ca-certificates", "1")])?;
        let node = write_installed_formula(&prefix, "node", "1", false, &[("openssl@3", "1")])?;
        let kimi = write_installed_formula(&prefix, "kimi-code", "1", false, &[("node", "1")])?;
        let _state = FormulaStateGuard::new(&openssl);
        crate::file::create_dir_all(openssl.join(".bottle/etc/openssl@3"))?;
        crate::file::write(
            openssl.join(".bottle/etc/openssl@3/openssl.cnf"),
            "openssl-default",
        )?;
        crate::file::create_dir_all(prefix.join("etc/ca-certificates"))?;
        crate::file::write(prefix.join("etc/ca-certificates/cert.pem"), "trusted-ca")?;
        crate::file::remove_file(prefix::linked_keg_record("openssl@3"))?;

        let keg_inode = openssl.metadata()?.ino();
        let receipt_inode = openssl.join("INSTALL_RECEIPT.json").metadata()?.ino();
        let opt_inode = prefix.join("opt/openssl@3").symlink_metadata()?.ino();
        let public_inode = prefix
            .join("lib/libopenssl@3.dylib")
            .symlink_metadata()?
            .ino();
        let manager = super::super::BrewManager::new();
        let request = PackageRequest {
            name: "kimi-code".to_string(),
            version: None,
            tap_url: None,
        };
        let status = manager.installed(std::slice::from_ref(&request)).await?;
        let PackageState::NeedsRepair { reason, .. } = &status[0].state else {
            panic!("root with unhealthy dependency must need repair")
        };
        assert!(reason.contains("dependency openssl@3/1"));
        assert!(reason.contains("linked-keg record"));
        assert!(reason.contains("lifecycle state absent"));
        assert!(reason.contains("shared lifecycle path missing"));
        assert!(
            prefix
                .join("etc/openssl@3/openssl.cnf")
                .symlink_metadata()
                .is_err()
        );
        assert!(
            prefix::linked_keg_record("openssl@3")
                .symlink_metadata()
                .is_err()
        );

        let snapshot = openssl.join(".brew/openssl@3.rb");
        let checksum = crate::hash::file_hash_sha256(&snapshot, None)?;
        let mut rf = resolved_formula("openssl@3", "1");
        rf.formula.ruby_source_checksum = Some(super::super::api::RubySourceChecksum {
            sha256: Some(checksum),
        });
        rf.formula.post_install_steps = vec![json!({
            "type": "symlink",
            "source": {"path": "{{etc}}/ca-certificates/cert.pem"},
            "target": {"path": "{{pkgetc}}/cert.pem"},
            "force": true
        })];
        let lifecycle = lifecycle::prepare(&rf.formula, &openssl)?;
        let health = installed_formula_health("openssl@3", "1");
        preflight_formula_repair(&health, &lifecycle)?;
        assert!(repair_formula(&health, &lifecycle, false).await?);

        assert_eq!(openssl.metadata()?.ino(), keg_inode);
        assert_eq!(
            openssl.join("INSTALL_RECEIPT.json").metadata()?.ino(),
            receipt_inode
        );
        assert_eq!(
            prefix.join("opt/openssl@3").symlink_metadata()?.ino(),
            opt_inode
        );
        assert_eq!(
            prefix
                .join("lib/libopenssl@3.dylib")
                .symlink_metadata()?
                .ino(),
            public_inode
        );
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/openssl@3/openssl.cnf"))?,
            "openssl-default"
        );
        assert_eq!(
            std::fs::read_link(prefix.join("etc/openssl@3/cert.pem"))?,
            prefix.join("etc/ca-certificates/cert.pem")
        );
        assert_eq!(
            manager.installed(std::slice::from_ref(&request)).await?[0].state,
            PackageState::Installed {
                version: "1".to_string()
            }
        );
        for keg in [ca, node, kimi] {
            assert!(keg.is_dir());
        }
        Ok(())
    }

    #[tokio::test]
    async fn legacy_repair_preserves_user_configuration() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1", true, &[])?;
        let _state = FormulaStateGuard::new(&keg);
        let snapshot = keg.join(".brew/foo.rb");
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "new-default")?;
        crate::file::create_dir_all(prefix.join("etc/foo"))?;
        crate::file::write(prefix.join("etc/foo/config"), "user-modified")?;

        let mut rf = resolved_formula("foo", "1");
        rf.formula.ruby_source_checksum = Some(super::super::api::RubySourceChecksum {
            sha256: Some(crate::hash::file_hash_sha256(&snapshot, None)?),
        });
        let lifecycle = lifecycle::prepare(&rf.formula, &keg)?;
        let health = installed_formula_health("foo", "1");
        assert_eq!(health.kind, FormulaHealthKind::Repairable);
        assert!(repair_formula(&health, &lifecycle, false).await?);

        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "user-modified"
        );
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config.default"))?,
            "new-default"
        );
        Ok(())
    }

    #[tokio::test]
    async fn missing_native_repair_provenance_fails_without_mutation() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1", false, &[])?;
        let _state = FormulaStateGuard::new(&keg);
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "default")?;
        let rf = resolved_formula("foo", "1");
        let lifecycle = lifecycle::prepare(&rf.formula, &keg)?;
        let health = installed_formula_health("foo", "1");
        assert_eq!(health.kind, FormulaHealthKind::ReinstallRequired);

        let error = repair_formula(&health, &lifecycle, false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("requires reinstall"));
        assert!(prefix.join("etc/foo/config").symlink_metadata().is_err());
        assert!(lifecycle::test_state_path(&keg).symlink_metadata().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn unknown_interrupted_lifecycle_is_never_replayed() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1", true, &[])?;
        let _state = FormulaStateGuard::new(&keg);
        let rf = resolved_formula("foo", "1");
        let lifecycle = lifecycle::prepare(&rf.formula, &keg)?;
        lifecycle::install(&lifecycle, None).await?;
        let state = lifecycle::test_state_path(&keg);
        let mut interrupted: Value = serde_json::from_slice(&std::fs::read(&state)?)?;
        interrupted["complete"] = json!(false);
        interrupted["phase"] = json!("shared_state");
        crate::file::write(&state, serde_json::to_vec(&interrupted)?)?;
        let health = installed_formula_health("foo", "1");
        assert_eq!(health.kind, FormulaHealthKind::ReinstallRequired);

        let error = repair_formula(&health, &lifecycle, false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unknown outcome"));
        let persisted: Value = serde_json::from_slice(&std::fs::read(&state)?)?;
        assert_eq!(persisted["complete"], false);
        assert_eq!(persisted["phase"], "shared_state");
        Ok(())
    }

    #[tokio::test]
    async fn idempotent_repair_journal_resumes_after_effect_before_checkpoint() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = write_installed_formula(&prefix, "foo", "1", true, &[])?;
        let _state = FormulaStateGuard::new(&keg);
        let source = keg.join(".bottle/etc/foo/config");
        let target = prefix.join("etc/foo/config");
        crate::file::create_dir_all(source.parent().unwrap())?;
        crate::file::write(&source, "default")?;
        crate::file::create_dir_all(target.parent().unwrap())?;
        crate::file::write(&target, "default")?;
        let rf = resolved_formula("foo", "1");
        let lifecycle = lifecycle::prepare(&rf.formula, &keg)?;
        lifecycle::install(&lifecycle, None).await?;
        let state = lifecycle::test_state_path(&keg);
        let mut persisted: Value = serde_json::from_slice(&std::fs::read(&state)?)?;
        persisted["repair"] = json!({
            "effects": [{"type": "copy", "source": source, "target": target}],
            "next": 0
        });
        crate::file::write(&state, serde_json::to_vec(&persisted)?)?;
        let health = installed_formula_health("foo", "1");
        assert_eq!(health.kind, FormulaHealthKind::Repairable);
        assert!(repair_formula(&health, &lifecycle, false).await?);

        let persisted: Value = serde_json::from_slice(&std::fs::read(&state)?)?;
        assert!(persisted["repair"].is_null());
        assert_eq!(
            crate::file::read_to_string(prefix.join("etc/foo/config"))?,
            "default"
        );
        Ok(())
    }

    #[test]
    fn test_does_not_infer_a_linked_record_without_public_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(Path::new("../Cellar/foo/1.0"), &prefix.join("opt/foo"))?;

        assert_eq!(linked_state("foo"), Some(("1.0".to_string(), false)));
        assert!(!repair_link_record("foo", false)?);
        assert!(prefix::linked_keg_record("foo").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn test_runtime_loader_does_not_make_glibc_look_linked() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        let keg = prefix.join("Cellar/glibc/1.0");
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::write(keg.join(".brew/glibc.rb"), "class Glibc < Formula; end\n")?;
        crate::file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17","runtime_dependencies":[]}"#,
        )?;
        crate::file::write(keg.join("sbom.spdx.json"), "{}")?;
        crate::file::create_dir_all(keg.join("lib"))?;
        crate::file::write(keg.join("lib/ld-linux-x86-64.so.2"), "")?;
        crate::file::create_dir_all(prefix.join("opt"))?;
        crate::file::make_symlink(Path::new("../Cellar/glibc/1.0"), &prefix.join("opt/glibc"))?;
        crate::file::create_dir_all(prefix.join("lib"))?;
        crate::file::make_symlink(
            &keg.join("lib/ld-linux-x86-64.so.2"),
            &prefix.join("lib/ld.so"),
        )?;

        assert_eq!(linked_state("glibc"), Some(("1.0".to_string(), false)));
        assert!(!repair_link_record("glibc", false)?);
        assert!(
            prefix::linked_keg_record("glibc")
                .symlink_metadata()
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_foreign_linked_record_blocks_linking_before_changes() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let (_tmp, prefix) = canonical_tempdir()?;
        let _guard = BrewPrefixGuard::set(&prefix);
        write_lib_keg(&prefix, "foo", "1.0")?;
        let linked = prefix::linked_keg_record("foo");
        crate::file::create_dir_all(linked.parent().unwrap())?;
        crate::file::write(&linked, "foreign")?;

        let err = link_keg("foo", "1.0", false).unwrap_err();

        assert!(err.to_string().contains("not created by mise or brew"));
        assert_eq!(crate::file::read_to_string(&linked)?, "foreign");
        assert!(prefix.join("opt/foo").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn test_relative_target() {
        assert_eq!(
            relative_target(
                Path::new("/opt/homebrew/Cellar/jq/1.7/bin/jq"),
                Path::new("/opt/homebrew/bin/jq"),
            ),
            PathBuf::from("../Cellar/jq/1.7/bin/jq")
        );
        assert_eq!(
            relative_target(
                Path::new("/opt/homebrew/Cellar/jq/1.7"),
                Path::new("/opt/homebrew/opt/jq"),
            ),
            PathBuf::from("../Cellar/jq/1.7")
        );
    }
}
