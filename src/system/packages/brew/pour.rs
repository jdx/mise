//! Pour a bottle: extract -> relocate -> codesign -> receipt -> link.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::api::BottleFile;
use super::fetch::OciBottleMetadata;
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
    let receipt_path = keg.join("INSTALL_RECEIPT.json");
    let receipt = std::fs::read(&receipt_path)
        .ok()
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
    if !snapshot.is_file() {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push(format!(
            "formula snapshot is missing: {}",
            snapshot.display()
        ));
    }
    let sbom = keg.join("sbom.spdx.json");
    if std::fs::read(&sbom)
        .ok()
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
        if keg_has_linkable_entries(keg) && !has_public_link_into(keg) {
            kind = max_health(kind, FormulaHealthKind::Repairable);
            reasons.push("public keg links are missing".into());
        }
    }

    if mise_owned && finalization_needs_repair(keg) {
        kind = FormulaHealthKind::ReinstallRequired;
        reasons.push("formula finalization stopped before completion".into());
    }
    match lifecycle::health(keg, mise_owned) {
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

/// Read the formula snapshot from a checksum-verified bottle without trusting
/// the current tap source checksum. Homebrew bottles embed the exact formula
/// snapshot used to build them, which may legitimately differ from the current
/// source while package version and bottle rebuild remain unchanged.
pub(super) fn bottle_formula_snapshot_sha256(
    name: &str,
    pkg_version: &str,
    tarball: &Path,
) -> Result<String> {
    let scratch = tempfile::tempdir()?;
    crate::file::untar(
        tarball,
        scratch.path(),
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: None,
            preserve_mtime: true,
        },
    )
    .wrap_err_with(|| format!("brew:{name}: failed to inspect verified bottle"))?;
    let snapshot = scratch
        .path()
        .join(name)
        .join(pkg_version)
        .join(".brew")
        .join(format!("{name}.rb"));
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

fn keg_has_linkable_entries(keg: &Path) -> bool {
    LINK_DIRS.iter().any(|directory| {
        let root = keg.join(directory);
        root.is_dir()
            && walkdir::WalkDir::new(root)
                .min_depth(1)
                .follow_links(false)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .any(|entry| !entry.file_type().is_dir())
    })
}

/// mise records keg-only installs explicitly. Native Homebrew installs do not,
/// so recover the same fact offline from Homebrew's installed formula snapshot.
/// Only the top-level Formula DSL declaration is accepted; comments, nested
/// statements, and similarly named methods fail closed.
fn keg_is_keg_only(name: &str, keg: &Path) -> bool {
    keg.join(KEG_ONLY_MARKER).is_file()
        || formula_snapshot_declares_keg_only(&keg.join(".brew").join(format!("{name}.rb")))
}

fn formula_snapshot_declares_keg_only(snapshot: &Path) -> bool {
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
    target: PathBuf,
    destination: PathBuf,
    previous: Option<PathBuf>,
}

fn preflight_topology_repair(
    name: &str,
    version: &str,
    keg: &Path,
) -> Result<Vec<TopologyRepairLink>> {
    let keg_only = keg_is_keg_only(name, keg);
    let mut expected = vec![(prefix::prefix().join("opt").join(name), keg.to_path_buf())];
    if !keg_only {
        for directory in LINK_DIRS {
            let root = keg.join(directory);
            if !root.exists() {
                continue;
            }
            for entry in walkdir::WalkDir::new(root).follow_links(false) {
                let entry = entry?;
                if !entry.file_type().is_dir() {
                    expected.push((
                        prefix::prefix().join(entry.path().strip_prefix(keg)?),
                        entry.path().to_path_buf(),
                    ));
                }
            }
        }
        expected.push((prefix::linked_keg_record(name), keg.to_path_buf()));
    }
    let rack = prefix::cellar().join(name);
    let mut repairs = vec![];
    for (destination, target) in expected {
        if symlink_points_to(&destination, &target) {
            continue;
        }
        if let Some(ancestor) = brew_owned_ancestor(&destination) {
            if path_matches_through_brew_owned_ancestor(&destination, &target, &ancestor) {
                continue;
            }
            bail!(
                "brew:{name}/{version}: topology repair would traverse a directory symlink: {}",
                destination.display()
            )
        }
        let previous = match destination.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let previous = std::fs::read_link(&destination)?;
                let resolved = resolved_symlink_target(&destination)
                    .ok_or_else(|| eyre::eyre!("could not resolve repair target"))?;
                if !resolved.starts_with(&rack) {
                    bail!(
                        "brew:{name}/{version}: topology target has ambiguous ownership: {}",
                        destination.display()
                    )
                }
                Some(previous)
            }
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                "brew:{name}/{version}: topology target has ambiguous ownership: {}",
                destination.display()
            ),
        };
        repairs.push(TopologyRepairLink {
            target,
            destination,
            previous,
        });
    }
    Ok(repairs)
}

fn apply_topology_repair(repairs: &[TopologyRepairLink]) -> Result<()> {
    let mut completed: Vec<&TopologyRepairLink> = vec![];
    for repair in repairs {
        let result = (|| -> Result<()> {
            crate::file::create_dir_all(repair.destination.parent().unwrap())?;
            if repair.destination.symlink_metadata().is_ok() {
                crate::file::remove_file(&repair.destination)?;
            }
            crate::file::make_symlink(
                &relative_target(&repair.target, &repair.destination),
                &repair.destination,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            for completed_repair in completed.into_iter().rev() {
                let _ = crate::file::remove_file(&completed_repair.destination);
                if let Some(previous) = &completed_repair.previous {
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
    pub tarball: &'a Path,
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
    let pkg_version = rf.formula.pkg_version()?;
    let keg = keg_path(name, &pkg_version);
    let rack = keg.parent().unwrap().to_path_buf();
    let tmp = rack.join(format!(".mise-tmp-{pkg_version}"));
    let scratch = rack.join(format!(".mise-extract-{pkg_version}"));
    for dir in [&tmp, &scratch] {
        if dir.exists() {
            crate::file::remove_all(dir)?;
        }
    }
    crate::file::create_dir_all(&scratch)?;

    // bottle tarballs contain <name>/<pkg_version>/...
    pr.set_message("extract".to_string());
    crate::file::untar(
        tarball,
        &scratch,
        ExtractionFormat::TarGz,
        &ExtractOptions {
            strip_components: 0,
            pr: Some(pr),
            preserve_mtime: true,
        },
    )
    .wrap_err_with(|| format!("failed to extract bottle for {name}"))?;
    let inner = scratch.join(name).join(&pkg_version);
    if !inner.exists() {
        bail!("unexpected bottle layout for {name}: missing {name}/{pkg_version} in archive");
    }
    crate::file::rename(&inner, &tmp)?;
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

fn archive_bottle_provenance(rf: &ResolvedFormula, keg: &Path) -> Result<FormulaInstallProvenance> {
    let receipt_path = keg.join("INSTALL_RECEIPT.json");
    let tab: Value = serde_json::from_slice(&std::fs::read(&receipt_path).wrap_err_with(|| {
        format!(
            "brew:{}: non-OCI archive bottle has no embedded receipt at {}; no package state was changed",
            rf.formula.name,
            receipt_path.display()
        )
    })?)
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
    serde_json::from_slice(&std::fs::read(&sbom_path).wrap_err_with(|| {
        format!(
            "brew:{}: {kind} has no embedded SBOM at {}",
            rf.formula.name,
            sbom_path.display()
        )
    })?)
    .wrap_err_with(|| format!("brew:{}: malformed embedded {kind} SBOM", rf.formula.name))
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
    if backup.symlink_metadata().is_ok() {
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
        let backup_metadata = backup.symlink_metadata()?;
        if !backup_metadata.is_dir() || backup_metadata.file_type().is_symlink() {
            bail!("recovery backup is not a directory: {}", backup.display());
        }
        if let Ok(keg_metadata) = keg.symlink_metadata() {
            if !keg_metadata.is_dir() || keg_metadata.file_type().is_symlink() {
                bail!("interrupted keg is not a directory: {}", keg.display());
            }
            crate::file::remove_all(keg)?;
        }
        return Ok(Some(backup));
    }
    if keg.symlink_metadata().is_err() {
        return Ok(None);
    }
    crate::file::rename(keg, &backup)?;
    Ok(Some(backup))
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
    if keg.symlink_metadata().is_ok() {
        crate::file::remove_all(keg)?;
    }
    if let Some(backup) = backup {
        crate::file::rename(backup, keg)?;
    }
    Ok(())
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
    let pkg_version = rf.formula.pkg_version()?;
    if complete_interrupted_finalization(keg)? {
        if staged_keg != keg && staged_keg.symlink_metadata().is_ok() {
            crate::file::remove_all(staged_keg)?;
        }
        return Ok(());
    }
    let previous_finalization_state = std::fs::read(finalization_state_path(keg)).ok();
    let previous_state = previous_finalization_state
        .as_deref()
        .map(serde_json::from_slice::<FinalizationState>)
        .transpose()
        .wrap_err_with(|| format!("brew:{name}: unreadable formula finalization state"))?;
    if let Some(state) = &previous_state {
        validate_finalization_identity(keg, state)?;
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
        (keg.symlink_metadata().is_ok() || backup.symlink_metadata().is_ok()).then_some(backup)
    };
    let predecessor_keg = predecessor_keg.and_then(|predecessor| {
        if predecessor == keg {
            planned_backup.clone()
        } else {
            Some(predecessor)
        }
    });
    let provenance_name = match &provenance {
        FormulaInstallProvenance::OciBottle { .. } => "oci_bottle",
        FormulaInstallProvenance::ArchiveBottle { .. } => "archive_bottle",
        FormulaInstallProvenance::SourceBuild { .. } => "source_build",
    };
    if let Err(error) = write_receipt(rf, tag, staged_keg, report, closure, &provenance) {
        if staged_keg == keg {
            restore_keg_backup(keg, existing_backup.as_deref())?;
        }
        return Err(error);
    }
    if let Err(error) = write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::Receipt,
            predecessor_keg: predecessor_keg.clone(),
        },
    ) {
        if staged_keg == keg {
            restore_keg_backup(keg, existing_backup.as_deref())?;
            restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        }
        return Err(error);
    }

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
            restore_keg_backup(keg, backup.as_deref())?;
            restore_finalization_state(keg, previous_finalization_state.as_deref())?;
            return Err(error);
        }
        backup
    };
    if backup != planned_backup {
        restore_keg_backup(keg, backup.as_deref())?;
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
        },
    ) {
        restore_keg_backup(keg, backup.as_deref())?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        return Err(error);
    }

    pr.set_message("link".to_string());
    if let Err(error) = link_keg(name, &pkg_version, rf.formula.keg_only) {
        restore_keg_backup(keg, backup.as_deref())?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        return Err(error);
    }
    if let Err(error) = write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::Linked,
            predecessor_keg: predecessor_keg.clone(),
        },
    ) {
        restore_keg_backup(keg, backup.as_deref())?;
        restore_finalization_state(keg, previous_finalization_state.as_deref())?;
        return Err(error);
    }

    pr.set_message("shared state".to_string());
    super::lifecycle::install(lifecycle, predecessor_keg.as_deref())
        .await
        .wrap_err_with(|| {
            format!(
                "failed to complete Homebrew shared-state lifecycle for {name}; \
                 the linked keg and any recovery backup are retained as needs-repair"
            )
        })?;
    write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::SharedState,
            predecessor_keg: predecessor_keg.clone(),
        },
    )?;
    if let Some(backup) = backup {
        crate::file::remove_all(backup)?;
    }
    write_finalization_state(
        keg,
        &FinalizationState {
            formula: name.clone(),
            version: pkg_version.clone(),
            provenance: provenance_name.to_string(),
            phase: FinalizationPhase::Complete,
            predecessor_keg,
        },
    )?;
    Ok(())
}

fn finalization_state_path(keg: &Path) -> PathBuf {
    crate::dirs::STATE
        .join("brew-formula-finalization")
        .join(format!(
            "{}.json",
            crate::hash::hash_to_str(&(prefix::prefix(), keg))
        ))
}

fn read_finalization_state(keg: &Path) -> Result<Option<FinalizationState>> {
    let path = finalization_state_path(keg);
    if path.symlink_metadata().is_err() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(
        &crate::file::read_to_string(&path)
            .wrap_err_with(|| format!("could not read {}", path.display()))?,
    )?))
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
    Ok(())
}

pub(super) fn complete_interrupted_finalization(keg: &Path) -> Result<bool> {
    let Some(mut state) = read_finalization_state(keg)? else {
        return Ok(false);
    };
    validate_finalization_identity(keg, &state)?;
    if state.phase == FinalizationPhase::Complete {
        return Ok(false);
    }
    match super::lifecycle::install_progress(keg) {
        super::lifecycle::LifecycleInstallProgress::Absent => return Ok(false),
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
    if backup.symlink_metadata().is_ok() {
        let backup_metadata = backup.symlink_metadata()?;
        if !backup_metadata.is_dir() || backup_metadata.file_type().is_symlink() {
            bail!("recovery backup is not a directory: {}", backup.display());
        }
        crate::file::remove_all(backup)?;
    }
    state.phase = FinalizationPhase::Complete;
    write_finalization_state(keg, &state)?;
    Ok(true)
}

pub(super) fn remove_finalization_state(keg: &Path) -> Result<()> {
    let path = finalization_state_path(keg);
    if path.symlink_metadata().is_ok() {
        crate::file::remove_file(path)?;
    }
    Ok(())
}

fn write_finalization_state(keg: &Path, state: &FinalizationState) -> Result<()> {
    let path = finalization_state_path(keg);
    crate::file::create_dir_all(path.parent().unwrap())?;
    crate::file::write(path, serde_json::to_vec_pretty(state)?)
}

fn restore_finalization_state(keg: &Path, previous: Option<&[u8]>) -> Result<()> {
    let path = finalization_state_path(keg);
    match previous {
        Some(previous) => crate::file::write(path, previous),
        None if path.symlink_metadata().is_ok() => crate::file::remove_file(path),
        None => Ok(()),
    }
}

fn finalization_needs_repair(keg: &Path) -> bool {
    let path = finalization_state_path(keg);
    if path.symlink_metadata().is_err() {
        return false;
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
            if formula_snapshot != &expected || !formula_snapshot.is_file() {
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
    crate::file::write(
        keg.join("INSTALL_RECEIPT.json"),
        serde_json::to_string(&receipt)?,
    )?;
    match provenance {
        FormulaInstallProvenance::OciBottle {
            sbom,
            sbom_supplement,
            ..
        } => {
            let current: Value =
                serde_json::from_slice(&std::fs::read(keg.join("sbom.spdx.json"))?)?;
            if &current != sbom {
                bail!(
                    "brew:{}: OCI bottle SBOM changed after validation",
                    rf.formula.name
                );
            }
            update_sbom(keg, now, sbom_supplement.as_ref())?;
        }
        FormulaInstallProvenance::ArchiveBottle { sbom, .. } => {
            let current: Value =
                serde_json::from_slice(&std::fs::read(keg.join("sbom.spdx.json"))?)?;
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
    let mut sbom: Value = serde_json::from_slice(
        &std::fs::read(&path)
            .wrap_err_with(|| format!("missing bottle SBOM at {}", path.display()))?,
    )?;
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
    crate::file::write(path, serde_json::to_vec_pretty(&sbom)?)
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
    crate::file::write(
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
fn can_overwrite(dest: &Path) -> bool {
    let Ok(meta) = dest.symlink_metadata() else {
        return true; // doesn't exist
    };
    if brew_owned_ancestor(dest).is_some() {
        return true;
    }
    if !meta.is_symlink() {
        return false;
    }
    points_into_cellar(dest)
}

/// Does this symlink point into our Cellar or opt? Resolve the link itself once,
/// then canonicalize its parent so nested relative links retain their final
/// component while using the Cellar's filesystem spelling.
fn points_into_cellar(link: &Path) -> bool {
    let Some(target) = resolved_symlink_target(link) else {
        return false;
    };
    let cellar = prefix::cellar()
        .canonicalize()
        .unwrap_or_else(|_| prefix::cellar());
    let opt = prefix::prefix()
        .join("opt")
        .canonicalize()
        .unwrap_or_else(|_| prefix::prefix().join("opt"));
    target.starts_with(cellar) || target.starts_with(opt)
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
fn brew_owned_ancestor(dest: &Path) -> Option<PathBuf> {
    let prefix_path = prefix::prefix();
    let mut ancestors: Vec<&Path> = dest
        .ancestors()
        .skip(1)
        .take_while(|p| *p != prefix_path && p.starts_with(&prefix_path))
        .collect();
    ancestors.reverse(); // outermost first
    for anc in ancestors {
        if anc
            .symlink_metadata()
            .map(|m| m.is_symlink())
            .unwrap_or(false)
        {
            return points_into_cellar(anc).then(|| anc.to_path_buf());
        }
    }
    None
}

/// A leaf reached through a Homebrew directory link already satisfies the
/// topology only when that exact ancestor-plus-suffix maps to the expected keg
/// leaf. Merely entering the Cellar is insufficient: another keg or subtree
/// must remain a hard conflict.
fn path_matches_through_brew_owned_ancestor(
    destination: &Path,
    target: &Path,
    ancestor: &Path,
) -> bool {
    let Some(ancestor_target) = resolved_symlink_target(ancestor) else {
        return false;
    };
    let Ok(suffix) = destination.strip_prefix(ancestor) else {
        return false;
    };
    resolved_path(&ancestor_target.join(suffix)) == resolved_path(target)
}

/// Replace brew-created directory symlinks on the way to `dest` with real
/// directories of symlinks to their old contents — the same expansion brew
/// performs when another keg needs to place files inside a wholesale-linked
/// directory (resolve_any_conflicts). The replacement is fully staged before
/// the symlink is swapped out, so a failure leaves the tree unchanged.
fn materialize_brew_dirs(dest: &Path) -> Result<()> {
    while let Some(link_dir) = brew_owned_ancestor(dest) {
        let raw_target = std::fs::read_link(&link_dir)?;
        let staging = link_dir.parent().unwrap().join(format!(
            ".mise-materialize-{}",
            link_dir.file_name().unwrap().to_string_lossy()
        ));
        let staged = (|| -> Result<()> {
            if staging.exists() {
                crate::file::remove_all(&staging)?;
            }
            crate::file::create_dir_all(&staging)?;
            // a dangling dir symlink (keg already pruned) has nothing to preserve
            let target = lexical_normalize(&link_dir.parent().unwrap().join(&raw_target));
            if target.is_dir() {
                for entry in std::fs::read_dir(&target)? {
                    let entry = entry?;
                    // targets are relative to the link's final location
                    let child_link = link_dir.join(entry.file_name());
                    crate::file::make_symlink(
                        &relative_target(&entry.path(), &child_link),
                        &staging.join(entry.file_name()),
                    )?;
                }
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

    let mut conflicts: Vec<PathBuf> = vec![];
    // (dest in prefix, target in keg); opt first
    let mut links: Vec<(PathBuf, PathBuf)> = vec![(opt_link.clone(), keg.clone())];
    if keg_only {
        debug!(
            "{name} is keg-only, not linking into {}",
            prefix_path.display()
        );
    } else {
        for dir in LINK_DIRS {
            let src_root = keg.join(dir);
            if !src_root.exists() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&src_root).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_dir() {
                    continue;
                }
                let rel = entry.path().strip_prefix(&keg)?;
                let dest = prefix_path.join(rel);
                if !can_overwrite(&dest) {
                    conflicts.push(dest);
                } else {
                    links.push((dest, entry.path().to_path_buf()));
                }
            }
        }
        let linked = prefix::linked_keg_record(name);
        if can_overwrite(&linked) {
            links.push((linked, keg.clone()));
        } else {
            conflicts.push(linked);
        }
    }
    if !conflicts.is_empty() {
        // nothing has been linked yet, and the caller rolls the keg back on
        // this error — so don't claim it remains usable
        bail!(
            "cannot link {name}: these files already exist and were not created by mise or brew:\n{}\n\
             Remove or rename them, then re-run `mise bootstrap packages apply`",
            conflicts
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    // remember every symlink we overwrite (upgrades replace the previous
    // version's links, opt included) so a failed link restores all of them
    let mut created: Vec<PathBuf> = vec![];
    let mut replaced: Vec<(PathBuf, PathBuf)> = vec![];
    let mut failure: Option<eyre::Report> = None;
    for (dest, target) in &links {
        let made = (|| -> Result<()> {
            // a parent that is a brew directory symlink must become a real
            // directory first — otherwise the link below would be created
            // inside (and delete files from) the old keg it points to
            materialize_brew_dirs(dest)?;
            crate::file::create_dir_all(dest.parent().unwrap())?;
            if dest.symlink_metadata().is_ok() {
                if let Ok(prev) = std::fs::read_link(dest) {
                    replaced.push((dest.clone(), prev));
                }
                crate::file::remove_file(dest)?;
            }
            crate::file::make_symlink(&relative_target(target, dest), dest)?;
            Ok(())
        })();
        if let Err(err) = made {
            failure = Some(err);
            break;
        }
        created.push(dest.clone());
    }
    if let Some(err) = failure {
        for dest in created {
            let _ = crate::file::remove_file(&dest);
        }
        for (dest, prev) in replaced {
            let _ = crate::file::make_symlink(&prev, &dest);
        }
        return Err(err);
    }
    Ok(())
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

        assert_eq!(
            bottle_formula_snapshot_sha256("foo", "1", &tarball)?,
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
        crate::file::create_dir_all(&keg)?;
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
        crate::file::create_dir_all(&keg)?;
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
            crate::file::create_dir_all(keg)?;
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
        crate::file::create_dir_all(&keg)?;
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
        let snapshot = write_source_keg(&keg, "new")?;
        crate::file::create_dir_all(keg.join(".bottle/etc/foo"))?;
        crate::file::create_dir_all(keg.join(".bottle/var/foo"))?;
        crate::file::create_dir_all(keg.join("share/foo"))?;
        crate::file::write(keg.join(".bottle/etc/foo/config"), "etc-default")?;
        crate::file::write(keg.join(".bottle/var/foo/state"), "var-default")?;
        crate::file::write(keg.join("share/foo/generated"), "generated")?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
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
            existing_backup: None,
            predecessor_keg: None,
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
        write_source_keg(&keg, "old")?;
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
        assert!(!finalization_needs_repair(&keg));
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
        write_source_keg(&keg, "old")?;
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
        write_source_keg(&backup, "old")?;
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
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Linked,
                predecessor_keg: Some(backup.clone()),
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

        write_source_keg(&old_keg, "old")?;
        crate::file::create_dir_all(old_keg.join(".bottle/etc/foo"))?;
        crate::file::write(old_keg.join(".bottle/etc/foo/config"), "old-default")?;
        write_source_keg(&keg, "interrupted")?;
        link_keg("foo", "2.0", false)?;
        crate::file::create_dir_all(prefix.join("etc/foo"))?;
        crate::file::write(prefix.join("etc/foo/config"), "old-default")?;
        write_finalization_state(
            &keg,
            &FinalizationState {
                formula: "foo".into(),
                version: "2.0".into(),
                provenance: "source_build".into(),
                phase: FinalizationPhase::Linked,
                predecessor_keg: Some(old_keg.clone()),
            },
        )?;

        let staged = keg.parent().unwrap().join(".mise-tmp-2.0");
        let snapshot = write_source_keg(&staged, "retry")?;
        crate::file::create_dir_all(staged.join(".bottle/etc/foo"))?;
        crate::file::write(staged.join(".bottle/etc/foo/config"), "new-default")?;
        let prepared = lifecycle::prepare(&rf.formula, &keg)?;
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
        // the other keg must not have been polluted
        assert!(!other.join("share").join("xml").join("foo.dtd").exists());
        Ok(())
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

        assert!(can_overwrite(&nested));
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
