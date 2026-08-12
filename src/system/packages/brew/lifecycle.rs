//! Persistent formula state and typed post-install operations.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use eyre::{WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::api::Formula;
use super::prefix;
use crate::result::Result;

#[derive(Debug, Serialize, Deserialize)]
struct LifecycleState {
    complete: bool,
    #[serde(default)]
    symlinks: Vec<LifecycleSymlink>,
    #[serde(default)]
    required_paths: Vec<PathBuf>,
    #[serde(default)]
    absent_patterns: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LifecycleSymlink {
    source: PathBuf,
    target: PathBuf,
}

/// Reject lifecycle metadata the native engine cannot execute before any pour mutates state.
pub(super) fn validate(formula: &Formula) -> Result<()> {
    if formula.post_install_defined {
        bail!(
            "brew:{} uses legacy Ruby post_install, which the native backend cannot execute truthfully",
            formula.name
        );
    }
    for step in &formula.post_install_steps {
        let kind = step.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "mkdir_p" => {
                reject_unknown_keys(step, &["type", "path"])?;
                path_spec(step, "path")?;
            }
            "remove" => {
                reject_unknown_keys(step, &["type", "paths", "recursive", "guards"])?;
                path_specs(step, "paths")?;
                validate_guards(step)?;
            }
            "copy" => {
                reject_unknown_keys(
                    step,
                    &["type", "source", "target", "recursive", "overwrite"],
                )?;
                path_spec(step, "source")?;
                path_spec(step, "target")?;
            }
            "run" => {
                reject_unknown_keys(step, &["type", "command", "args"])?;
                path_spec(step, "command")?;
                string_array(step, "args")?;
            }
            "symlink" => {
                reject_unknown_keys(
                    step,
                    &[
                        "type",
                        "source",
                        "target",
                        "force",
                        "overwrite",
                        "source_glob",
                        "recursive",
                    ],
                )?;
                path_spec(step, "source")?;
                path_spec(step, "target")?;
            }
            _ => bail!(
                "brew:{} requires unsupported post_install_steps type {:?}; no package state was changed",
                formula.name,
                kind
            ),
        }
    }
    Ok(())
}

fn validate_guards(step: &Value) -> Result<()> {
    for guard in step
        .get("guards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        reject_unknown_keys(guard, &["path", "condition", "id"])?;
        path_spec_value(guard)?;
        if guard.get("condition").and_then(Value::as_str) != Some("if_exists") {
            bail!("unsupported post-install guard condition");
        }
    }
    Ok(())
}

fn reject_unknown_keys(step: &Value, allowed: &[&str]) -> Result<()> {
    let object = step
        .as_object()
        .ok_or_else(|| eyre!("post-install step must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!("unsupported post-install step field {key:?}");
    }
    Ok(())
}

pub(super) fn needs_repair(keg: &Path) -> bool {
    let state_path = state_path(keg);
    if state_path.exists() {
        let Ok(contents) = crate::file::read_to_string(&state_path) else {
            return true;
        };
        let Ok(state) = serde_json::from_str::<LifecycleState>(&contents) else {
            return true;
        };
        if !state.complete
            || state
                .symlinks
                .iter()
                .any(|link| resolved_symlink_target(&link.target).as_ref() != Some(&link.source))
            || state.required_paths.iter().any(|path| !path.exists())
            || state.absent_patterns.iter().any(|pattern| {
                glob::glob(pattern)
                    .ok()
                    .into_iter()
                    .flatten()
                    .any(|path| path.is_ok())
            })
        {
            return true;
        }
    }
    ["etc", "var"].into_iter().any(|root| {
        let source = keg.join(".bottle").join(root);
        source.exists() && shared_tree_missing(&source, &prefix::prefix().join(root))
    })
}

pub(super) fn install(formula: &Formula, keg: &Path) -> Result<()> {
    validate(formula)?;
    let state_path = state_path(keg);
    write_state(
        &state_path,
        &LifecycleState {
            complete: false,
            symlinks: vec![],
            required_paths: vec![],
            absent_patterns: vec![],
        },
    )?;
    let mut symlinks = vec![];
    let mut required_paths = vec![];
    let mut absent_patterns = vec![];
    let result = (|| {
        for root in ["etc", "var"] {
            required_paths.extend(install_shared_tree(
                formula,
                keg,
                root,
                &keg.join(".bottle").join(root),
                &prefix::prefix().join(root),
            )?);
        }
        for step in &formula.post_install_steps {
            let effects = execute_step(formula, keg, step)?;
            symlinks.extend(effects.symlinks);
            required_paths.extend(effects.required_paths);
            absent_patterns.extend(effects.absent_patterns);
        }
        Ok(())
    })();
    if result.is_ok() {
        write_state(
            &state_path,
            &LifecycleState {
                complete: true,
                symlinks,
                required_paths,
                absent_patterns,
            },
        )?;
    }
    result
}

fn state_path(keg: &Path) -> PathBuf {
    let identity = crate::hash::hash_to_str(&(
        prefix::prefix(),
        keg.parent().and_then(Path::file_name),
        keg.file_name(),
    ));
    crate::dirs::STATE
        .join("brew-formula-lifecycle")
        .join(format!("{identity}.json"))
}

pub(super) fn remove_owned_state(keg: &Path) -> Result<()> {
    let path = state_path(keg);
    if path.exists() {
        let state: LifecycleState = serde_json::from_str(&crate::file::read_to_string(&path)?)?;
        remove_lifecycle_symlinks(&state)?;
        crate::file::remove_file(path)?;
    }
    Ok(())
}

fn remove_lifecycle_symlinks(state: &LifecycleState) -> Result<()> {
    for link in &state.symlinks {
        if resolved_symlink_target(&link.target).as_ref() == Some(&link.source) {
            crate::file::remove_file(&link.target)?;
        }
    }
    Ok(())
}

fn write_state(path: &Path, state: &LifecycleState) -> Result<()> {
    crate::file::create_dir_all(path.parent().unwrap())?;
    crate::file::write(path, serde_json::to_string_pretty(state)?)
}

fn install_shared_tree(
    formula: &Formula,
    keg: &Path,
    root: &str,
    source_root: &Path,
    destination_root: &Path,
) -> Result<Vec<PathBuf>> {
    if !source_root.is_dir() {
        return Ok(vec![]);
    }
    let mut installed_paths = vec![];
    for entry in walkdir::WalkDir::new(source_root).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source_root)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = destination_root.join(relative);
        if entry.file_type().is_dir() {
            crate::file::create_dir_all(&destination)?;
            continue;
        }
        let destination =
            install_destination(formula, keg, root, entry.path(), relative, &destination)?;
        atomic_copy(entry.path(), &destination)?;
        installed_paths.push(destination);
    }
    Ok(installed_paths)
}

fn install_destination(
    formula: &Formula,
    keg: &Path,
    root: &str,
    source: &Path,
    relative: &Path,
    destination: &Path,
) -> Result<PathBuf> {
    if destination.symlink_metadata().is_err() || files_equal(source, destination) {
        return Ok(destination.to_path_buf());
    }
    let rack = keg
        .parent()
        .ok_or_else(|| eyre!("keg has no formula rack"))?;
    for old_keg in crate::file::ls(rack).unwrap_or_default() {
        if old_keg == keg || !old_keg.is_dir() {
            continue;
        }
        let old_default = old_keg.join(".bottle").join(root).join(relative);
        if old_default.symlink_metadata().is_ok() && files_equal(&old_default, destination) {
            return Ok(destination.to_path_buf());
        }
    }
    let default = PathBuf::from(format!("{}.default", destination.display()));
    debug!(
        "brew:{} preserving modified {}; writing new default to {}",
        formula.name,
        destination.display(),
        default.display()
    );
    Ok(default)
}

fn atomic_copy(source: &Path, destination: &Path) -> Result<()> {
    crate::file::create_dir_all(destination.parent().unwrap())?;
    let temp = destination.with_file_name(format!(
        ".{}.mise-new",
        destination.file_name().unwrap().to_string_lossy()
    ));
    if temp.symlink_metadata().is_ok() {
        crate::file::remove_file(&temp)?;
    }
    let metadata = source.symlink_metadata()?;
    if metadata.file_type().is_symlink() {
        crate::file::make_symlink(&fs::read_link(source)?, &temp)?;
    } else {
        fs::copy(source, &temp)?;
        fs::set_permissions(&temp, metadata.permissions())?;
    }
    if destination.symlink_metadata().is_ok() {
        crate::file::remove_file(destination)?;
    }
    crate::file::rename(&temp, destination)?;
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> bool {
    match (left.symlink_metadata(), right.symlink_metadata()) {
        (Ok(a), Ok(b)) if a.file_type().is_symlink() && b.file_type().is_symlink() => {
            fs::read_link(left).ok() == fs::read_link(right).ok()
        }
        (Ok(a), Ok(b)) if a.is_file() && b.is_file() && a.len() == b.len() => {
            fs::read(left).ok() == fs::read(right).ok()
        }
        _ => false,
    }
}

fn shared_tree_missing(source_root: &Path, destination_root: &Path) -> bool {
    walkdir::WalkDir::new(source_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| !entry.file_type().is_dir())
        .any(|entry| {
            entry
                .path()
                .strip_prefix(source_root)
                .ok()
                .is_some_and(|relative| destination_root.join(relative).symlink_metadata().is_err())
        })
}

#[derive(Default)]
struct StepEffects {
    symlinks: Vec<LifecycleSymlink>,
    required_paths: Vec<PathBuf>,
    absent_patterns: Vec<String>,
}

fn execute_step(formula: &Formula, keg: &Path, step: &Value) -> Result<StepEffects> {
    if !guards_match(formula, keg, step)? {
        return Ok(StepEffects::default());
    }
    match step.get("type").and_then(Value::as_str) {
        Some("mkdir_p") => {
            let path = resolve_path(formula, keg, path_spec(step, "path")?)?;
            crate::file::create_dir_all(&path)?;
            Ok(StepEffects {
                required_paths: vec![path],
                ..Default::default()
            })
        }
        Some("remove") => {
            let mut absent_patterns = vec![];
            for spec in path_specs(step, "paths")? {
                let pattern = resolve_path(formula, keg, spec)?;
                absent_patterns.extend(expand_braces(&pattern.to_string_lossy()));
                for path in expand_path_spec(formula, keg, spec)? {
                    if step.get("recursive").and_then(Value::as_bool) == Some(true) {
                        if path.exists() {
                            crate::file::remove_all(&path)?;
                        }
                    } else if path.symlink_metadata().is_ok() {
                        crate::file::remove_file(&path)?;
                    }
                }
            }
            Ok(StepEffects {
                absent_patterns,
                ..Default::default()
            })
        }
        Some("copy") => {
            let source = resolve_path(formula, keg, path_spec(step, "source")?)?;
            let target = resolve_path(formula, keg, path_spec(step, "target")?)?;
            let required_paths = if step.get("recursive").and_then(Value::as_bool) == Some(true) {
                copy_recursive(&source, &target)?
            } else {
                let destination = if target.is_dir() {
                    target.join(source.file_name().unwrap())
                } else {
                    target
                };
                atomic_copy(&source, &destination)?;
                vec![destination]
            };
            Ok(StepEffects {
                required_paths,
                ..Default::default()
            })
        }
        Some("symlink") => {
            let target = resolve_path(formula, keg, path_spec(step, "target")?)?;
            let sources = if step.get("source_glob").and_then(Value::as_bool) == Some(true) {
                expand_path_spec(formula, keg, path_spec(step, "source")?)?
            } else {
                vec![resolve_path(formula, keg, path_spec(step, "source")?)?]
            };
            if sources.is_empty() {
                return Ok(StepEffects::default());
            }
            let force = step.get("force").and_then(Value::as_bool).unwrap_or(false)
                || step
                    .get("overwrite")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            let directory_target = sources.len() > 1 || target.is_dir();
            if directory_target {
                crate::file::create_dir_all(&target)?;
            }
            let mut links = vec![];
            for source in sources {
                let destination = if directory_target {
                    target.join(source.file_name().unwrap())
                } else {
                    target.clone()
                };
                crate::file::create_dir_all(destination.parent().unwrap())?;
                if destination.symlink_metadata().is_ok() {
                    if !force && !files_equal(&source, &destination) {
                        bail!(
                            "post-install target already exists: {}",
                            destination.display()
                        );
                    }
                    crate::file::remove_file(&destination)?;
                }
                let relative = super::pour::relative_target(&source, &destination);
                crate::file::make_symlink(&relative, &destination)?;
                links.push(LifecycleSymlink {
                    source: super::pour::lexical_normalize(&source),
                    target: destination,
                });
            }
            Ok(StepEffects {
                symlinks: links,
                ..Default::default()
            })
        }
        Some("run") => {
            let executable = resolve_path(formula, keg, path_spec(step, "command")?)?;
            let args = string_array(step, "args")?
                .iter()
                .map(|arg| expand_templates(formula, keg, arg))
                .collect::<Result<Vec<_>>>()?;
            let status = Command::new(&executable)
                .args(&args)
                .stdin(Stdio::null())
                .env("HOMEBREW_PREFIX", prefix::prefix())
                .env("HOMEBREW_CELLAR", prefix::cellar())
                .status()
                .wrap_err_with(|| format!("failed to run {}", executable.display()))?;
            if !status.success() {
                bail!(
                    "post-install command {} exited {status}",
                    executable.display()
                );
            }
            let required_paths = args
                .iter()
                .map(PathBuf::from)
                .filter(|path| path.starts_with(prefix::prefix()) && path.exists())
                .collect();
            Ok(StepEffects {
                required_paths,
                ..Default::default()
            })
        }
        _ => unreachable!("validated before mutation"),
    }
}

fn guards_match(formula: &Formula, keg: &Path, step: &Value) -> Result<bool> {
    for guard in step
        .get("guards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !resolve_path(formula, keg, guard)?.exists() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn copy_recursive(source: &Path, target: &Path) -> Result<Vec<PathBuf>> {
    let destination = if target.is_dir() {
        target.join(source.file_name().unwrap())
    } else {
        target.to_path_buf()
    };
    if destination.exists() {
        crate::file::remove_all(&destination)?;
    }
    let mut outputs = vec![destination.clone()];
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let output = destination.join(relative);
        if entry.file_type().is_dir() {
            crate::file::create_dir_all(&output)?;
        } else {
            atomic_copy(entry.path(), &output)?;
            outputs.push(output);
        }
    }
    Ok(outputs)
}

fn resolved_symlink_target(path: &Path) -> Option<PathBuf> {
    let target = fs::read_link(path).ok()?;
    let target = if target.is_absolute() {
        target
    } else {
        path.parent()?.join(target)
    };
    Some(super::pour::lexical_normalize(&target))
}

fn path_spec<'a>(step: &'a Value, key: &str) -> Result<&'a Value> {
    let spec = step
        .get(key)
        .ok_or_else(|| eyre!("post-install step missing {key}"))?;
    if spec.get("path").and_then(Value::as_str).is_none() {
        bail!("post-install {key} must contain a string path");
    }
    Ok(spec)
}

fn path_spec_value(spec: &Value) -> Result<&Value> {
    if spec.get("path").and_then(Value::as_str).is_none() {
        bail!("post-install path spec must contain a string path");
    }
    Ok(spec)
}

fn path_specs<'a>(step: &'a Value, key: &str) -> Result<Vec<&'a Value>> {
    step.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("post-install step missing path list {key}"))?
        .iter()
        .map(path_spec_value)
        .collect()
}

fn expand_path_spec(formula: &Formula, keg: &Path, spec: &Value) -> Result<Vec<PathBuf>> {
    let pattern = resolve_path(formula, keg, spec)?;
    let patterns = expand_braces(&pattern.to_string_lossy());
    let mut paths = vec![];
    for pattern in patterns {
        for path in glob::glob(&pattern)? {
            paths.push(path?);
        }
    }
    Ok(paths)
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_offset) = pattern[open + 1..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + 1 + close_offset;
    pattern[open + 1..close]
        .split(',')
        .map(|choice| format!("{}{}{}", &pattern[..open], choice, &pattern[close + 1..]))
        .collect()
}

fn string_array<'a>(step: &'a Value, key: &str) -> Result<Vec<&'a str>> {
    step.get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| eyre!("{key} must contain strings"))
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(vec![]))
}

fn resolve_path(formula: &Formula, keg: &Path, spec: &Value) -> Result<PathBuf> {
    let path = spec.get("path").and_then(Value::as_str).unwrap();
    if path.starts_with("{{") {
        return Ok(PathBuf::from(expand_templates(formula, keg, path)?));
    }
    let base = spec.get("base").and_then(Value::as_str).unwrap_or("prefix");
    Ok(template_base(formula, keg, base)?.join(path))
}

fn expand_templates(formula: &Formula, keg: &Path, value: &str) -> Result<String> {
    let mut output = value.to_string();
    for token in [
        "HOMEBREW_PREFIX",
        "HOMEBREW_CELLAR",
        "prefix",
        "opt_prefix",
        "bin",
        "sbin",
        "lib",
        "libexec",
        "share",
        "pkgshare",
        "var",
        "etc",
        "pkgetc",
    ] {
        let replacement = template_base(formula, keg, token)?;
        output = output.replace(&format!("{{{{{token}}}}}"), &replacement.to_string_lossy());
    }
    if output.contains("{{") {
        bail!("unsupported post-install template in {value:?}");
    }
    Ok(output)
}

fn template_base(formula: &Formula, keg: &Path, base: &str) -> Result<PathBuf> {
    let shared = prefix::prefix();
    match base {
        "HOMEBREW_PREFIX" => Ok(shared),
        "HOMEBREW_CELLAR" => Ok(prefix::cellar()),
        "prefix" => Ok(keg.to_path_buf()),
        "opt_prefix" => Ok(shared.join("opt").join(&formula.name)),
        "bin" | "sbin" | "lib" | "libexec" | "share" => Ok(keg.join(base)),
        "pkgshare" => Ok(keg.join("share").join(&formula.name)),
        "var" | "etc" => Ok(shared.join(base)),
        "pkgetc" => Ok(shared.join("etc").join(&formula.name)),
        _ => bail!("unsupported post-install path base {base:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formula(steps: Vec<Value>) -> Formula {
        serde_json::from_value(serde_json::json!({
            "name": "openssl@3",
            "versions": {"stable": "1"},
            "bottle": {},
            "post_install_steps": steps
        }))
        .unwrap()
    }

    #[test]
    fn rejects_unsupported_steps_before_install() {
        let error = validate(&formula(vec![serde_json::json!({"type": "touch"})])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported post_install_steps type")
        );
    }

    #[test]
    fn accepts_ca_certificates_and_openssl_steps() {
        let ca = formula(vec![serde_json::json!({
            "command": {"base": "libexec", "path": "post-install"},
            "type": "run",
            "args": ["{{pkgshare}}/cacert.pem", "{{pkgetc}}/cert.pem"]
        })]);
        let openssl = formula(vec![serde_json::json!({
            "source": {"path": "{{etc}}/ca-certificates/cert.pem"},
            "target": {"path": "{{pkgetc}}/cert.pem"},
            "force": true,
            "type": "symlink"
        })]);
        validate(&ca).unwrap();
        validate(&openssl).unwrap();
    }

    #[test]
    fn preserves_modified_config_as_default() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = tmp.path().join("Cellar/foo/2");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        crate::file::create_dir_all(source.parent().unwrap())?;
        crate::file::create_dir_all(destination.parent().unwrap())?;
        crate::file::write(&source, "new")?;
        crate::file::write(&destination, "user")?;
        let installed = install_shared_tree(
            &formula(vec![]),
            &keg,
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "user");
        let default = PathBuf::from(format!("{}.default", destination.display()));
        assert_eq!(crate::file::read_to_string(&default)?, "new");
        assert_eq!(installed, vec![default]);
        Ok(())
    }

    #[test]
    fn upgrades_untouched_old_default() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rack = tmp.path().join("Cellar/foo");
        let old = rack.join("1/.bottle/etc/foo/config");
        let keg = rack.join("2");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        for path in [&old, &source, &destination] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&old, "old")?;
        crate::file::write(&destination, "old")?;
        crate::file::write(&source, "new")?;
        let installed = install_shared_tree(
            &formula(vec![]),
            &keg,
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "new");
        assert_eq!(installed, vec![destination.clone()]);
        assert!(!PathBuf::from(format!("{}.default", destination.display())).exists());
        Ok(())
    }

    #[test]
    fn old_default_comparison_stays_within_shared_root() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let rack = tmp.path().join("Cellar/foo");
        let old_var = rack.join("1/.bottle/var/foo/config");
        let keg = rack.join("2");
        let source = keg.join(".bottle/etc/foo/config");
        let destination = tmp.path().join("etc/foo/config");
        for path in [&old_var, &source, &destination] {
            crate::file::create_dir_all(path.parent().unwrap())?;
        }
        crate::file::write(&old_var, "user")?;
        crate::file::write(&destination, "user")?;
        crate::file::write(&source, "new")?;
        let installed = install_shared_tree(
            &formula(vec![]),
            &keg,
            "etc",
            &keg.join(".bottle/etc"),
            &tmp.path().join("etc"),
        )?;
        assert_eq!(crate::file::read_to_string(&destination)?, "user");
        assert_eq!(
            installed,
            vec![PathBuf::from(format!("{}.default", destination.display()))]
        );
        Ok(())
    }

    #[test]
    fn records_typed_step_health_effects() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let keg = tmp.path().join("Cellar/foo/1");
        crate::file::create_dir_all(keg.join("share/source"))?;
        crate::file::write(keg.join("share/source/file"), "value")?;
        crate::file::write(keg.join("obsolete"), "old")?;
        let formula = formula(vec![]);

        let mkdir = execute_step(
            &formula,
            &keg,
            &serde_json::json!({
                "type": "mkdir_p",
                "path": {"base": "prefix", "path": "generated"}
            }),
        )?;
        assert_eq!(mkdir.required_paths, vec![keg.join("generated")]);

        let copy = execute_step(
            &formula,
            &keg,
            &serde_json::json!({
                "type": "copy",
                "source": {"base": "prefix", "path": "share/source"},
                "target": {"base": "prefix", "path": "copied"},
                "recursive": true
            }),
        )?;
        assert!(copy.required_paths.contains(&keg.join("copied/file")));

        let remove = execute_step(
            &formula,
            &keg,
            &serde_json::json!({
                "type": "remove",
                "paths": [{"base": "prefix", "path": "obsolete"}]
            }),
        )?;
        assert_eq!(
            remove.absent_patterns,
            vec![keg.join("obsolete").to_string_lossy()]
        );
        assert!(!keg.join("obsolete").exists());
        Ok(())
    }

    #[test]
    fn prune_removes_only_unchanged_lifecycle_symlinks() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let replacement = tmp.path().join("replacement");
        let target = tmp.path().join("target");
        crate::file::write(&source, "source")?;
        crate::file::write(&replacement, "replacement")?;
        crate::file::make_symlink(&source, &target)?;
        let state = LifecycleState {
            complete: true,
            symlinks: vec![LifecycleSymlink {
                source: source.clone(),
                target: target.clone(),
            }],
            required_paths: vec![],
            absent_patterns: vec![],
        };
        remove_lifecycle_symlinks(&state)?;
        assert!(!target.exists());

        crate::file::make_symlink(&replacement, &target)?;
        remove_lifecycle_symlinks(&state)?;
        assert_eq!(fs::read_link(target)?, replacement);
        Ok(())
    }
}
