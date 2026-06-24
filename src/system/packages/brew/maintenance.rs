//! Import/prune helpers for declarative Homebrew bootstrap packages.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde::Deserialize;
use walkdir::WalkDir;

use super::{pour, prefix, resolve, state};
use crate::file;
use crate::result::Result;
use crate::system::packages::PackageRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFormula {
    pub name: String,
    pub version: String,
    pub tap: Option<String>,
    pub installed_on_request: bool,
}

impl InstalledFormula {
    pub fn package_name(&self) -> String {
        match &self.tap {
            Some(tap) => format!("{tap}/{}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn config_key(&self) -> String {
        format!("brew:{}", self.package_name())
    }

    pub fn tap_entry_with_urls(
        &self,
        configured_taps: &BTreeMap<String, String>,
    ) -> Result<Option<(String, String)>> {
        self.tap
            .as_ref()
            .map(|tap| {
                configured_taps
                    .get(tap)
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| default_tap_url(tap))
                    .map(|url| (tap.clone(), url))
            })
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneCandidate {
    pub name: String,
    pub version: String,
    pub keg: PathBuf,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PrunePlan {
    pub remove: Vec<PruneCandidate>,
    pub forget: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AdoptionPlan {
    records: Vec<AdoptionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdoptionRecord {
    name: String,
    version: String,
    on_request: bool,
}

impl PrunePlan {
    pub fn is_empty(&self) -> bool {
        self.remove.is_empty() && self.forget.is_empty()
    }
}

#[derive(Debug, Default, Deserialize)]
struct InstallReceipt {
    #[serde(default)]
    installed_on_request: Option<bool>,
    #[serde(default)]
    source: Option<ReceiptSource>,
    #[serde(default)]
    runtime_dependencies: Vec<RuntimeDependency>,
}

#[derive(Debug, Default, Deserialize)]
struct ReceiptSource {
    #[serde(default)]
    tap: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeDependency {
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub fn default_tap_url(tap: &str) -> Result<String> {
    let mut parts = tap.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(repo), None) if !owner.is_empty() && !repo.is_empty() => {
            Ok(format!("https://github.com/{owner}/homebrew-{repo}.git"))
        }
        _ => bail!(
            "tap '{tap}' must be in <owner>/<repo> format; supply an explicit URL for non-standard taps"
        ),
    }
}

pub fn linked_formulae(include_all: bool) -> Result<Vec<InstalledFormula>> {
    let opt = prefix::prefix().join("opt");
    let mut formulae = BTreeMap::new();
    for entry in file::ls(&opt)? {
        if !entry
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        let Some(name) = entry
            .file_name()
            .and_then(|f| f.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some((version, keg)) = linked_keg(&entry) else {
            continue;
        };
        let rack = file::desymlink_path(&prefix::cellar().join(&name));
        if !keg.starts_with(rack) {
            continue;
        }
        let receipt = read_receipt(&keg)?;
        let installed_on_request = receipt
            .as_ref()
            .and_then(|r| r.installed_on_request)
            .unwrap_or(false);
        if !include_all && !installed_on_request {
            continue;
        }
        let tap = receipt
            .and_then(|r| r.source.and_then(|s| s.tap))
            .filter(|tap| tap != "homebrew/core");
        formulae.insert(
            name.clone(),
            InstalledFormula {
                name,
                version,
                tap,
                installed_on_request,
            },
        );
    }
    Ok(formulae.into_values().collect())
}

pub async fn adoption_plan(pkgs: &[PackageRequest]) -> Result<AdoptionPlan> {
    if pkgs.is_empty() {
        return Ok(AdoptionPlan::default());
    }
    let closure = resolve::resolve_closure_with_taps(pkgs).await?;
    let records = closure
        .into_iter()
        .filter_map(|rf| {
            pour::linked_version(&rf.formula.name).map(|version| AdoptionRecord {
                name: rf.formula.name,
                version,
                on_request: rf.on_request,
            })
        })
        .collect();
    Ok(AdoptionPlan { records })
}

pub fn apply_adoption_plan(plan: &AdoptionPlan) -> Result<()> {
    if plan.records.is_empty() {
        return Ok(());
    }
    let mut ledger = state::Ledger::load();
    for record in &plan.records {
        ledger.record(&record.name, &record.version, record.on_request);
    }
    ledger.save()
}

pub async fn prune_plan(configured: &[PackageRequest]) -> Result<PrunePlan> {
    let keep = configured_formula_closure(configured).await?;
    let ledger = state::Ledger::load();
    prune_plan_from_ledger(&ledger, &keep)
}

pub fn apply_prune_plan(plan: &PrunePlan, dry_run: bool) -> Result<()> {
    if dry_run {
        for candidate in &plan.remove {
            miseprintln!("remove brew:{}@{}", candidate.name, candidate.version);
        }
        for name in &plan.forget {
            miseprintln!("forget brew:{name}");
        }
        return Ok(());
    }
    let mut ledger = state::Ledger::load();
    for candidate in &plan.remove {
        unlink_and_remove_keg(candidate)?;
        ledger.remove(&candidate.name);
    }
    for name in &plan.forget {
        ledger.remove(name);
    }
    ledger.save()?;
    prefix::setup_linux_runtime()?;
    Ok(())
}

async fn configured_formula_closure(configured: &[PackageRequest]) -> Result<HashSet<String>> {
    if configured.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(resolve::resolve_closure_with_taps(configured)
        .await?
        .into_iter()
        .map(|rf| rf.formula.name)
        .collect())
}

fn prune_plan_from_ledger(ledger: &state::Ledger, keep: &HashSet<String>) -> Result<PrunePlan> {
    let mut keep = keep.clone();
    keep.extend(unmanaged_linked_formula_closure(ledger)?);
    Ok(prune_plan_from_ledger_with_keep(ledger, &keep))
}

fn prune_plan_from_ledger_with_keep(ledger: &state::Ledger, keep: &HashSet<String>) -> PrunePlan {
    let mut plan = PrunePlan::default();
    for (name, entry) in &ledger.kegs {
        if keep.contains(name) {
            continue;
        }
        let Some(linked_version) = pour::linked_version(name) else {
            plan.forget.push(name.clone());
            continue;
        };
        if linked_version != entry.pkg_version {
            warn!(
                "brew:{name}: linked version {linked_version} differs from mise ledger {}; skipping prune until re-imported",
                entry.pkg_version
            );
            continue;
        }
        let keg = file::desymlink_path(&pour::keg_path(name, &linked_version));
        if keg.is_dir() {
            plan.remove.push(PruneCandidate {
                name: name.clone(),
                version: linked_version,
                keg,
            });
        } else {
            plan.forget.push(name.clone());
        }
    }
    plan
}

fn unmanaged_linked_formula_closure(ledger: &state::Ledger) -> Result<HashSet<String>> {
    let deps_by_formula = linked_receipt_dependencies()?;
    let roots = deps_by_formula
        .keys()
        .filter(|name| !ledger.kegs.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    Ok(dependency_closure(&deps_by_formula, roots))
}

fn linked_receipt_dependencies() -> Result<BTreeMap<String, Vec<String>>> {
    let opt = prefix::prefix().join("opt");
    let mut deps_by_formula = BTreeMap::new();
    for entry in file::ls(&opt)? {
        if !entry
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        let Some(name) = entry
            .file_name()
            .and_then(|f| f.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some((_, keg)) = linked_keg(&entry) else {
            continue;
        };
        let rack = file::desymlink_path(&prefix::cellar().join(&name));
        if !keg.starts_with(rack) {
            continue;
        }
        let deps = read_receipt(&keg)?
            .map(|receipt| {
                receipt
                    .runtime_dependencies
                    .into_iter()
                    .filter_map(|dep| {
                        dep.full_name
                            .or(dep.name)
                            .and_then(|name| normalize_dependency_name(&name))
                    })
                    .collect()
            })
            .unwrap_or_default();
        deps_by_formula.insert(name, deps);
    }
    Ok(deps_by_formula)
}

fn dependency_closure(
    deps_by_formula: &BTreeMap<String, Vec<String>>,
    roots: Vec<String>,
) -> HashSet<String> {
    let mut keep = HashSet::new();
    let mut stack = roots;
    while let Some(name) = stack.pop() {
        if !keep.insert(name.clone()) {
            continue;
        }
        if let Some(deps) = deps_by_formula.get(&name) {
            stack.extend(deps.iter().cloned());
        }
    }
    keep
}

fn normalize_dependency_name(name: &str) -> Option<String> {
    let name = name.rsplit('/').next().unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn read_receipt(keg: &Path) -> Result<Option<InstallReceipt>> {
    let path = keg.join("INSTALL_RECEIPT.json");
    if !path.exists() {
        return Ok(None);
    }
    let body = file::read_to_string(&path)?;
    serde_json::from_str(&body)
        .map(Some)
        .wrap_err_with(|| format!("failed to parse {}", path.display()))
}

fn linked_keg(opt_link: &Path) -> Option<(String, PathBuf)> {
    let target = std::fs::read_link(opt_link).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        opt_link.parent()?.join(target)
    };
    let keg = file::desymlink_path(&target);
    if !keg.is_dir() {
        return None;
    }
    let version = keg.file_name()?.to_string_lossy().to_string();
    Some((version, keg))
}

fn unlink_and_remove_keg(candidate: &PruneCandidate) -> Result<()> {
    let links = links_into_keg(&candidate.name, &candidate.keg)?;
    for link in links {
        std::fs::remove_file(&link)
            .wrap_err_with(|| format!("failed rm: {}", file::display_path(&link)))?;
        remove_empty_parents(&link, &prefix::prefix())?;
    }
    file::remove_all(&candidate.keg)?;
    let rack = prefix::cellar().join(&candidate.name);
    file::remove_dir(&rack)?;
    Ok(())
}

fn links_into_keg(name: &str, keg: &Path) -> Result<Vec<PathBuf>> {
    let prefix_path = prefix::prefix();
    let mut links = BTreeSet::new();
    let opt = prefix_path.join("opt").join(name);
    if symlink_points_into(&opt, keg) {
        links.insert(opt);
    }
    for dir in pour::LINK_DIRS {
        let root = prefix_path.join(dir);
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_symlink() && symlink_points_into(entry.path(), keg) {
                links.insert(entry.path().to_path_buf());
            }
        }
    }
    Ok(links.into_iter().collect())
}

fn symlink_points_into(link: &Path, keg: &Path) -> bool {
    if !link
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return false;
    }
    let Ok(target) = std::fs::read_link(link) else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        link.parent().unwrap_or_else(|| Path::new("/")).join(target)
    };
    let keg = file::desymlink_path(keg);
    file::desymlink_path(&target).starts_with(keg)
}

fn remove_empty_parents(path: &Path, stop: &Path) -> Result<()> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == stop || dir.parent() == Some(stop) || !dir.starts_with(stop) {
            break;
        }
        file::remove_dir(dir)?;
        if dir.exists() {
            break;
        }
        current = dir.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

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

    fn write_keg(prefix: &Path, name: &str, version: &str, receipt: &str) -> Result<PathBuf> {
        let keg = prefix.join("Cellar").join(name).join(version);
        file::create_dir_all(keg.join("bin"))?;
        file::write(keg.join("bin").join(name), "")?;
        file::write(keg.join("INSTALL_RECEIPT.json"), receipt)?;
        let opt = prefix.join("opt");
        file::create_dir_all(&opt)?;
        let opt_target = Path::new("../Cellar").join(name).join(version);
        let opt_link = opt.join(name);
        file::make_symlink(&opt_target, &opt_link)?;
        let bin = prefix.join("bin");
        file::create_dir_all(&bin)?;
        let bin_target = Path::new("../Cellar")
            .join(name)
            .join(version)
            .join("bin")
            .join(name);
        let bin_link = bin.join(name);
        file::make_symlink(&bin_target, &bin_link)?;
        Ok(file::desymlink_path(&keg))
    }

    #[test]
    fn linked_formulae_default_keeps_only_requested_formulae() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        write_keg(
            tmp.path(),
            "jq",
            "1.7",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        write_keg(
            tmp.path(),
            "onigmo",
            "6.2.0",
            r#"{"installed_on_request":false,"source":{"tap":"homebrew/core"}}"#,
        )?;

        assert_eq!(
            linked_formulae(false)?,
            vec![InstalledFormula {
                name: "jq".to_string(),
                version: "1.7".to_string(),
                tap: None,
                installed_on_request: true,
            }]
        );
        assert_eq!(linked_formulae(true)?.len(), 2);
        Ok(())
    }

    #[test]
    fn linked_formulae_infers_tapped_config_entries() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        write_keg(
            tmp.path(),
            "widget",
            "1.0.0",
            r#"{"installed_on_request":true,"source":{"tap":"acme/tools"}}"#,
        )?;

        let formula = linked_formulae(false)?.pop().unwrap();
        assert_eq!(formula.config_key(), "brew:acme/tools/widget");
        assert_eq!(
            formula.tap_entry_with_urls(&BTreeMap::new())?,
            Some((
                "acme/tools".to_string(),
                "https://github.com/acme/homebrew-tools.git".to_string(),
            ))
        );
        assert_eq!(
            formula.tap_entry_with_urls(&BTreeMap::from([(
                "acme/tools".to_string(),
                "https://brew.example.com/acme/tools.git".to_string(),
            )]))?,
            Some((
                "acme/tools".to_string(),
                "https://brew.example.com/acme/tools.git".to_string(),
            ))
        );
        Ok(())
    }

    #[test]
    fn prune_plan_keeps_configured_and_forgets_missing_ledger_entries() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        write_keg(
            tmp.path(),
            "keep",
            "1.0.0",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let remove = write_keg(
            tmp.path(),
            "remove",
            "2.0.0",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let mut ledger = state::Ledger::default();
        ledger.record("keep", "1.0.0", true);
        ledger.record("remove", "2.0.0", true);
        ledger.record("missing", "3.0.0", true);
        let keep = HashSet::from(["keep".to_string()]);

        assert_eq!(
            prune_plan_from_ledger(&ledger, &keep)?,
            PrunePlan {
                remove: vec![PruneCandidate {
                    name: "remove".to_string(),
                    version: "2.0.0".to_string(),
                    keg: remove,
                }],
                forget: vec!["missing".to_string()],
            }
        );
        Ok(())
    }

    #[test]
    fn prune_plan_keeps_ledger_deps_needed_by_unmanaged_formulae() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        write_keg(
            tmp.path(),
            "readline",
            "8.2.0",
            r#"{"installed_on_request":false,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let unused = write_keg(
            tmp.path(),
            "unused",
            "1.0.0",
            r#"{"installed_on_request":false,"source":{"tap":"homebrew/core"}}"#,
        )?;
        write_keg(
            tmp.path(),
            "external",
            "2.0.0",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"},"runtime_dependencies":[{"full_name":"readline"}]}"#,
        )?;
        let mut ledger = state::Ledger::default();
        ledger.record("readline", "8.2.0", false);
        ledger.record("unused", "1.0.0", false);

        assert_eq!(
            prune_plan_from_ledger(&ledger, &HashSet::new())?,
            PrunePlan {
                remove: vec![PruneCandidate {
                    name: "unused".to_string(),
                    version: "1.0.0".to_string(),
                    keg: unused,
                }],
                forget: vec![],
            }
        );
        Ok(())
    }

    #[test]
    fn unlink_and_remove_keg_removes_links_and_keg() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let keg = write_keg(
            tmp.path(),
            "jq",
            "1.7",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let candidate = PruneCandidate {
            name: "jq".to_string(),
            version: "1.7".to_string(),
            keg: keg.clone(),
        };

        unlink_and_remove_keg(&candidate)?;

        assert!(!tmp.path().join("bin").join("jq").exists());
        assert!(!tmp.path().join("opt").join("jq").exists());
        assert!(tmp.path().join("bin").exists());
        assert!(tmp.path().join("opt").exists());
        assert!(!keg.exists());
        assert!(!tmp.path().join("Cellar").join("jq").exists());
        Ok(())
    }

    #[test]
    fn apply_prune_plan_dry_run_removes_nothing() -> Result<()> {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let keg = write_keg(
            tmp.path(),
            "jq",
            "1.7",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let plan = PrunePlan {
            remove: vec![PruneCandidate {
                name: "jq".to_string(),
                version: "1.7".to_string(),
                keg: keg.clone(),
            }],
            forget: vec![],
        };

        apply_prune_plan(&plan, true)?;

        assert!(tmp.path().join("bin").join("jq").exists());
        assert!(tmp.path().join("opt").join("jq").exists());
        assert!(keg.exists());
        Ok(())
    }
}
