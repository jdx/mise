//! Import/prune helpers for declarative Homebrew bootstrap packages.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};
use serde::Deserialize;
use walkdir::WalkDir;

use super::{api, lifecycle, pour, prefix, resolve};
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
}

#[derive(Debug)]
struct PreparedLinkRemoval {
    path: PathBuf,
    ancestry: lifecycle::DirectoryAncestry,
    device: u64,
    inode: u64,
    target: PathBuf,
}

impl PrunePlan {
    pub fn is_empty(&self) -> bool {
        self.remove.is_empty()
    }
}

#[derive(Debug, Default, Deserialize)]
struct InstallReceipt {
    #[serde(default)]
    installed_on_request: Option<bool>,
    #[serde(default)]
    source: Option<ReceiptSource>,
}

#[derive(Debug, Default, Deserialize)]
struct ReceiptSource {
    #[serde(default)]
    tap: Option<String>,
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

pub async fn prune_plan(configured: &[PackageRequest]) -> Result<PrunePlan> {
    let keep = configured_package_closure(configured).await?;
    prune_plan_from_linked_formulae(&keep)
}

pub fn apply_prune_plan(plan: &PrunePlan, dry_run: bool) -> Result<()> {
    if dry_run {
        for candidate in &plan.remove {
            miseprintln!("remove brew:{}@{}", candidate.name, candidate.version);
        }
        return Ok(());
    }
    for candidate in &plan.remove {
        unlink_and_remove_keg(candidate)?;
    }
    prefix::setup_linux_runtime()?;
    Ok(())
}

async fn configured_package_closure(configured: &[PackageRequest]) -> Result<HashSet<String>> {
    if configured.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(resolve::resolve_closure_with_taps(configured)
        .await?
        .into_iter()
        .map(|rf| formula_package_name(&rf.formula))
        .collect())
}

fn prune_plan_from_linked_formulae(keep: &HashSet<String>) -> Result<PrunePlan> {
    let mut plan = PrunePlan::default();
    for formula in linked_formulae(true)? {
        if keep.contains(&formula.package_name()) {
            continue;
        }
        let keg = file::desymlink_path(&pour::keg_path(&formula.name, &formula.version));
        if keg.is_dir() {
            plan.remove.push(PruneCandidate {
                name: formula.name,
                version: formula.version,
                keg,
            });
        }
    }
    Ok(plan)
}

fn formula_package_name(formula: &api::Formula) -> String {
    match formula.tap.as_deref().filter(|tap| *tap != "homebrew/core") {
        Some(tap) => format!("{tap}/{}", formula.name),
        None => formula.name.clone(),
    }
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
    let lifecycle_removal = lifecycle::prepare_remove_owned_state(&candidate.keg)?;
    let finalization_removal = pour::prepare_remove_finalization_state(&candidate.keg)?;
    let links = links_into_keg(&candidate.name, &candidate.keg)?;
    for link in &links {
        validate_prepared_link_removal(link)?;
    }
    let prefix_path = prefix::prefix();
    for link in links {
        validate_prepared_link_removal(&link)?;
        fs::remove_file(&link.path)
            .wrap_err_with(|| format!("failed rm: {}", file::display_path(&link.path)))?;
        let linked_dir = prefix::linked_keg_record(&candidate.name)
            .parent()
            .unwrap()
            .to_path_buf();
        let stop = if link.path.parent() == Some(linked_dir.as_path()) {
            &linked_dir
        } else {
            &prefix_path
        };
        remove_empty_parents(&link.path, stop)?;
    }
    lifecycle::remove_owned_state_prepared(lifecycle_removal)?;
    pour::remove_finalization_state_prepared(finalization_removal)?;
    file::remove_all(&candidate.keg)?;
    let rack = prefix::cellar().join(&candidate.name);
    file::remove_dir(&rack)?;
    Ok(())
}

fn links_into_keg(name: &str, keg: &Path) -> Result<Vec<PreparedLinkRemoval>> {
    let prefix_path = prefix::prefix();
    let mut paths = BTreeSet::new();
    let opt = prefix_path.join("opt").join(name);
    if symlink_points_into(&opt, keg)? {
        paths.insert(opt);
    }
    let linked = prefix::linked_keg_record(name);
    if symlink_points_into(&linked, keg)? {
        paths.insert(linked);
    }
    for dir in pour::LINK_DIRS {
        let root = prefix_path.join(dir);
        match symlink_metadata_if_exists(&root)? {
            None => continue,
            Some(metadata) if metadata.file_type().is_dir() => {}
            Some(_) => bail!("public link root is not a directory: {}", root.display()),
        }
        for entry in WalkDir::new(&root).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_symlink() && symlink_points_into(entry.path(), keg)? {
                paths.insert(entry.path().to_path_buf());
            }
        }
    }
    paths
        .into_iter()
        .map(|path| prepare_link_removal(path, keg))
        .collect::<Result<Vec<_>>>()
}

fn symlink_metadata_if_exists(path: &Path) -> Result<Option<fs::Metadata>> {
    match path.symlink_metadata() {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn symlink_points_into(link: &Path, keg: &Path) -> Result<bool> {
    let Some(metadata) = symlink_metadata_if_exists(link)? else {
        return Ok(false);
    };
    if !metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let target = fs::read_link(link)?;
    raw_symlink_points_into(link, &target, keg)
}

fn raw_symlink_points_into(link: &Path, raw_target: &Path, keg: &Path) -> Result<bool> {
    // resolve one hop only, like link_keg's ownership checks — a Cellar
    // dylib alias is itself a relative symlink, and chasing the chain
    // resolved it against the CWD, leaving such links behind as dangling
    let target = pour::lexical_normalize(
        &link
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .join(raw_target),
    );
    // canonicalize the parent only, for path spelling (/var -> /private/var);
    // the final component must stay unresolved
    let resolved = match (target.parent(), target.file_name()) {
        (Some(dir), Some(name)) => match fs::canonicalize(dir) {
            Ok(directory) => directory.join(name),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => target.clone(),
            Err(error) => return Err(error.into()),
        },
        _ => target.clone(),
    };
    Ok(resolved.starts_with(file::desymlink_path(keg)))
}

fn prepare_link_removal(path: PathBuf, keg: &Path) -> Result<PreparedLinkRemoval> {
    let metadata = path.symlink_metadata()?;
    if !metadata.file_type().is_symlink() {
        bail!(
            "public link changed during prune preflight: {}",
            path.display()
        )
    }
    let (device, inode) = node_device_inode(&metadata)?;
    let target = fs::read_link(&path)?;
    let ancestry = lifecycle::capture_directory_ancestry(
        path.parent()
            .ok_or_else(|| eyre::eyre!("public link has no parent: {}", path.display()))?,
    )?;
    let prepared = PreparedLinkRemoval {
        path,
        ancestry,
        device,
        inode,
        target,
    };
    validate_prepared_link_removal(&prepared)?;
    if !raw_symlink_points_into(&prepared.path, &prepared.target, keg)? {
        bail!(
            "public link no longer points into the pruned keg: {}",
            prepared.path.display()
        )
    }
    Ok(prepared)
}

fn validate_prepared_link_removal(prepared: &PreparedLinkRemoval) -> Result<()> {
    lifecycle::validate_directory_ancestry(&prepared.ancestry)?;
    let metadata = prepared.path.symlink_metadata().wrap_err_with(|| {
        format!(
            "public link disappeared after prune preflight: {}",
            prepared.path.display()
        )
    })?;
    if !metadata.file_type().is_symlink()
        || node_device_inode(&metadata)? != (prepared.device, prepared.inode)
        || fs::read_link(&prepared.path)? != prepared.target
    {
        bail!(
            "public link changed after prune preflight: {}",
            prepared.path.display()
        )
    }
    Ok(())
}

#[cfg(unix)]
fn node_device_inode(metadata: &fs::Metadata) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn node_device_inode(_metadata: &fs::Metadata) -> Result<(u64, u64)> {
    bail!("Homebrew prune link identity requires Unix filesystem semantics")
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
    use tokio::sync::Mutex;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    struct BrewPrefixGuard {
        previous: Option<String>,
    }

    impl BrewPrefixGuard {
        fn set(prefix: &Path) -> Self {
            let previous = crate::env::var("MISE_SYSTEM_BREW_PREFIX").ok();
            crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", file::desymlink_path(prefix));
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

    fn write_linked_record(prefix: &Path, name: &str, version: &str) -> Result<()> {
        let linked = prefix.join("var/homebrew/linked");
        file::create_dir_all(&linked)?;
        file::make_symlink(
            &Path::new("../../../Cellar").join(name).join(version),
            &linked.join(name),
        )?;
        Ok(())
    }

    fn formula(name: &str, version: &str) -> api::Formula {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "versions": {"stable": version},
            "bottle": {},
            "post_install_steps": []
        }))
        .unwrap()
    }

    fn write_formula_snapshot(keg: &Path, name: &str, contents: &str) -> Result<()> {
        let snapshot = keg.join(".brew").join(format!("{name}.rb"));
        file::create_dir_all(snapshot.parent().unwrap())?;
        file::write(snapshot, contents)
    }

    fn formula_finalization_state_path(keg: &Path) -> PathBuf {
        crate::dirs::STATE
            .join("brew-formula-finalization")
            .join(format!(
                "{}.json",
                crate::hash::hash_to_str(&(prefix::prefix(), keg))
            ))
    }

    fn assert_prune_fixture_intact(prefix: &Path, name: &str, keg: &Path, lifecycle_state: &Path) {
        assert!(prefix.join("bin").join(name).is_symlink());
        assert!(prefix.join("opt").join(name).is_symlink());
        assert!(prefix.join("var/homebrew/linked").join(name).is_symlink());
        assert!(keg.is_dir());
        assert!(lifecycle_state.is_file());
        assert!(keg.join(".brew/.mise-lifecycle-incarnation").is_file());
    }

    #[test]
    fn linked_formulae_default_keeps_only_requested_formulae() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
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
        let _lock = ENV_LOCK.blocking_lock();
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
    fn prune_plan_removes_unconfigured_linked_formulae() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
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
        let keep = HashSet::from(["keep".to_string()]);

        assert_eq!(
            prune_plan_from_linked_formulae(&keep)?,
            PrunePlan {
                remove: vec![PruneCandidate {
                    name: "remove".to_string(),
                    version: "2.0.0".to_string(),
                    keg: remove,
                }],
            }
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn prepared_public_link_removal_rejects_foreign_file_swap() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let keg = write_keg(
            tmp.path(),
            "jq",
            "1.7",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let link = prefix::prefix().join("bin/jq");
        let prepared = prepare_link_removal(link.clone(), &keg)?;
        file::remove_file(&link)?;
        file::write(&link, "foreign")?;

        assert!(validate_prepared_link_removal(&prepared).is_err());
        assert_eq!(file::read_to_string(link)?, "foreign");
        Ok(())
    }

    #[test]
    fn prune_plan_uses_tap_qualified_keep_keys() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let keep = HashSet::from(["acme/tools/widget".to_string()]);

        {
            let tmp = tempfile::tempdir()?;
            let _guard = BrewPrefixGuard::set(tmp.path());
            let core_widget = write_keg(
                tmp.path(),
                "widget",
                "1.0.0",
                r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
            )?;

            assert_eq!(
                prune_plan_from_linked_formulae(&keep)?,
                PrunePlan {
                    remove: vec![PruneCandidate {
                        name: "widget".to_string(),
                        version: "1.0.0".to_string(),
                        keg: core_widget,
                    }],
                }
            );
        }

        {
            let tmp = tempfile::tempdir()?;
            let _guard = BrewPrefixGuard::set(tmp.path());
            write_keg(
                tmp.path(),
                "widget",
                "1.0.0",
                r#"{"installed_on_request":true,"source":{"tap":"acme/tools"}}"#,
            )?;

            assert_eq!(
                prune_plan_from_linked_formulae(&keep)?,
                PrunePlan { remove: vec![] }
            );
        }

        Ok(())
    }

    #[test]
    fn prune_plan_removes_formulae_only_needed_by_unconfigured_roots() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let readline = write_keg(
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
        let external = write_keg(
            tmp.path(),
            "external",
            "2.0.0",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"},"runtime_dependencies":[{"full_name":"readline"}]}"#,
        )?;

        assert_eq!(
            prune_plan_from_linked_formulae(&HashSet::new())?,
            PrunePlan {
                remove: vec![
                    PruneCandidate {
                        name: "external".to_string(),
                        version: "2.0.0".to_string(),
                        keg: external,
                    },
                    PruneCandidate {
                        name: "readline".to_string(),
                        version: "8.2.0".to_string(),
                        keg: readline,
                    },
                    PruneCandidate {
                        name: "unused".to_string(),
                        version: "1.0.0".to_string(),
                        keg: unused,
                    }
                ],
            }
        );
        Ok(())
    }

    #[test]
    fn unlink_and_remove_keg_removes_links_and_keg() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
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
        write_linked_record(tmp.path(), "jq", "1.7")?;

        unlink_and_remove_keg(&candidate)?;

        assert!(!tmp.path().join("bin").join("jq").exists());
        assert!(!tmp.path().join("opt").join("jq").exists());
        assert!(
            tmp.path()
                .join("var/homebrew/linked/jq")
                .symlink_metadata()
                .is_err()
        );
        assert!(tmp.path().join("bin").exists());
        assert!(tmp.path().join("opt").exists());
        assert!(!keg.exists());
        assert!(!tmp.path().join("Cellar").join("jq").exists());
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prune_accepts_native_replacement_with_stale_mise_lifecycle_state() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let name = "openssl@3";
        let version = "1.0";
        let keg = write_keg(
            tmp.path(),
            name,
            version,
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1.0","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        write_formula_snapshot(&keg, name, "class OpensslAT3; end")?;
        write_linked_record(tmp.path(), name, version)?;
        let prepared = lifecycle::prepare(&formula(name, version), &keg)?;
        lifecycle::install(&prepared, None).await?;
        let lifecycle_state = lifecycle::test_state_path(&keg);
        assert!(lifecycle_state.is_file());

        // Real Homebrew replaces the exact rack/version and its receipt while
        // mise's private state survives outside the keg.
        file::remove_all(&keg)?;
        file::create_dir_all(keg.join("bin"))?;
        file::write(keg.join("bin").join(name), "native replacement")?;
        write_formula_snapshot(&keg, name, "class OpensslAT3; end # native")?;
        file::write(
            keg.join("INSTALL_RECEIPT.json"),
            r#"{"homebrew_version":"6.0.17","installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        let candidate = PruneCandidate {
            name: name.to_string(),
            version: version.to_string(),
            keg: keg.clone(),
        };

        unlink_and_remove_keg(&candidate)?;

        assert!(
            tmp.path()
                .join("bin")
                .join(name)
                .symlink_metadata()
                .is_err()
        );
        assert!(
            tmp.path()
                .join("opt")
                .join(name)
                .symlink_metadata()
                .is_err()
        );
        assert!(
            tmp.path()
                .join("var/homebrew/linked")
                .join(name)
                .symlink_metadata()
                .is_err()
        );
        assert!(!keg.exists());
        assert!(lifecycle_state.symlink_metadata().is_err());
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prune_rejects_stale_mise_identity_before_unlinking() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let name = "openssl@3";
        let version = "1.0";
        let keg = write_keg(
            tmp.path(),
            name,
            version,
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":true,"built_as_bottle":true,"poured_from_bottle":true,"time":123,"source_modified_time":100,"arch":"arm64","source":{"spec":"stable","versions":{"stable":"1.0","head":null,"version_scheme":0},"path":"/api/formula.jws.json","tap":"homebrew/core","tap_git_head":"core-head"}}"#,
        )?;
        write_formula_snapshot(&keg, name, "class OpensslAT3; end")?;
        write_linked_record(tmp.path(), name, version)?;
        let prepared = lifecycle::prepare(&formula(name, version), &keg)?;
        lifecycle::install(&prepared, None).await?;
        let lifecycle_state = lifecycle::test_state_path(&keg);
        file::remove_file(keg.join(".brew/.mise-lifecycle-incarnation"))?;
        let candidate = PruneCandidate {
            name: name.to_string(),
            version: version.to_string(),
            keg: keg.clone(),
        };

        let error = unlink_and_remove_keg(&candidate).unwrap_err();

        assert!(error.to_string().contains("lifecycle"));
        assert!(tmp.path().join("bin").join(name).is_symlink());
        assert!(tmp.path().join("opt").join(name).is_symlink());
        assert!(
            tmp.path()
                .join("var/homebrew/linked")
                .join(name)
                .is_symlink()
        );
        assert!(keg.is_dir());
        assert!(lifecycle_state.is_file());
        file::remove_file(lifecycle_state)?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prune_rejects_malformed_finalization_state_before_unlinking() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let name = "openssl@3";
        let version = "1.0";
        let keg = write_keg(
            tmp.path(),
            name,
            version,
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        write_formula_snapshot(&keg, name, "class OpensslAT3; end")?;
        write_linked_record(tmp.path(), name, version)?;
        let prepared = lifecycle::prepare(&formula(name, version), &keg)?;
        lifecycle::install(&prepared, None).await?;
        let lifecycle_state = lifecycle::test_state_path(&keg);
        let finalization_state = formula_finalization_state_path(&keg);
        file::create_dir_all(finalization_state.parent().unwrap())?;
        file::write(&finalization_state, b"{")?;
        let candidate = PruneCandidate {
            name: name.to_string(),
            version: version.to_string(),
            keg: keg.clone(),
        };

        let error = unlink_and_remove_keg(&candidate).unwrap_err();

        assert!(error.to_string().contains("finalization"));
        assert_prune_fixture_intact(tmp.path(), name, &keg, &lifecycle_state);
        assert_eq!(std::fs::read(&finalization_state)?, b"{");
        file::remove_file(finalization_state)?;
        file::remove_file(lifecycle_state)?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prune_rejects_symlink_finalization_state_before_unlinking() -> Result<()> {
        let _lock = ENV_LOCK.lock().await;
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let name = "openssl@3";
        let version = "1.0";
        let keg = write_keg(
            tmp.path(),
            name,
            version,
            r#"{"homebrew_version":"6.0.17 (mise)","installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        write_formula_snapshot(&keg, name, "class OpensslAT3; end")?;
        write_linked_record(tmp.path(), name, version)?;
        let prepared = lifecycle::prepare(&formula(name, version), &keg)?;
        lifecycle::install(&prepared, None).await?;
        let lifecycle_state = lifecycle::test_state_path(&keg);
        let finalization_state = formula_finalization_state_path(&keg);
        file::create_dir_all(finalization_state.parent().unwrap())?;
        let foreign = tmp.path().join("foreign-finalization-state");
        file::write(&foreign, "foreign")?;
        file::make_symlink(&foreign, &finalization_state)?;
        let candidate = PruneCandidate {
            name: name.to_string(),
            version: version.to_string(),
            keg: keg.clone(),
        };

        let error = unlink_and_remove_keg(&candidate).unwrap_err();

        assert!(error.to_string().contains("finalization"));
        assert_prune_fixture_intact(tmp.path(), name, &keg, &lifecycle_state);
        assert!(finalization_state.is_symlink());
        assert_eq!(file::read_to_string(&foreign)?, "foreign");
        file::remove_file(finalization_state)?;
        file::remove_file(lifecycle_state)?;
        Ok(())
    }

    /// a prefix link to a Cellar dylib alias resolves through a relative
    /// symlink chain and must still be removed, not left dangling
    #[test]
    fn unlink_and_remove_keg_removes_dylib_alias_links() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
        let tmp = tempfile::tempdir()?;
        let _guard = BrewPrefixGuard::set(tmp.path());
        let keg = write_keg(
            tmp.path(),
            "foo",
            "1.0",
            r#"{"installed_on_request":true,"source":{"tap":"homebrew/core"}}"#,
        )?;
        file::create_dir_all(keg.join("lib"))?;
        file::write(keg.join("lib").join("libfoo.1.dylib"), "")?;
        file::make_symlink(
            Path::new("libfoo.1.dylib"),
            &keg.join("lib").join("libfoo.dylib"),
        )?;
        let lib = tmp.path().join("lib");
        file::create_dir_all(&lib)?;
        for name in ["libfoo.1.dylib", "libfoo.dylib"] {
            file::make_symlink(
                &Path::new("../Cellar/foo/1.0/lib").join(name),
                &lib.join(name),
            )?;
        }
        let candidate = PruneCandidate {
            name: "foo".to_string(),
            version: "1.0".to_string(),
            keg: keg.clone(),
        };

        unlink_and_remove_keg(&candidate)?;

        assert!(lib.join("libfoo.1.dylib").symlink_metadata().is_err());
        assert!(lib.join("libfoo.dylib").symlink_metadata().is_err());
        assert!(!keg.exists());
        Ok(())
    }

    #[test]
    fn apply_prune_plan_dry_run_removes_nothing() -> Result<()> {
        let _lock = ENV_LOCK.blocking_lock();
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
        };
        write_linked_record(tmp.path(), "jq", "1.7")?;

        apply_prune_plan(&plan, true)?;

        assert!(tmp.path().join("bin").join("jq").exists());
        assert!(tmp.path().join("opt").join("jq").exists());
        assert!(tmp.path().join("var/homebrew/linked/jq").is_symlink());
        assert!(keg.exists());
        Ok(())
    }
}
