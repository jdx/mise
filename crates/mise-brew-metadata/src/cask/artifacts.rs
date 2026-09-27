use std::collections::BTreeMap;
use std::path::Path;

use eyre::{Result, bail, eyre};
use serde_json::Value;

use super::{flight::parse_flight_steps, helpers::*, model::*, types::*};

mod ordinary;

pub use ordinary::{
    parse_command_wrapper_artifact, parse_font_artifact, parse_generated_completion_artifact,
    parse_generic_artifact, parse_pkg_artifact,
};
use ordinary::{parse_completion_artifact, parse_installer_artifact};

pub fn cask_artifacts(cask: &Cask) -> Result<CaskArtifacts> {
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
        && artifacts.installers.is_empty()
        && artifacts.generic.is_empty()
        && artifacts.fonts.is_empty()
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

fn declared_target(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|o| o.get("target"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn artifact_target(value: &Value, values: &[Value]) -> Option<String> {
    values
        .get(1)
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("target"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| declared_target(value))
}

/// The `source` and `target` of an artifact declared either as a bare string or
/// as a `[source, {target: ...}]` pair. A bare string carries no target of its
/// own; artifacts that accept a sibling `target` key add it themselves.
fn artifact_source_target(value: &Value, artifact: &Value) -> Option<(String, Option<String>)> {
    match artifact {
        Value::String(source) => Some((source.clone(), None)),
        Value::Array(values) => Some((
            values.first()?.as_str()?.to_string(),
            artifact_target(value, values),
        )),
        _ => None,
    }
}

pub fn parse_app_artifact(value: &Value) -> Option<AppArtifact> {
    let (source, target) = artifact_source_target(value, value.as_object()?.get("app")?)?;
    Some(AppArtifact { source, target })
}

pub fn parse_binary_artifact(value: &Value) -> Option<BinaryArtifact> {
    let (source, target) = artifact_source_target(value, value.as_object()?.get("binary")?)?;
    Some(BinaryArtifact {
        source,
        target: target.or_else(|| declared_target(value)),
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
            Value::String(id) if !id.trim().is_empty() => pkg_ids.push(id.clone()),
            Value::Array(ids) => pkg_ids.extend(
                ids.iter()
                    .filter_map(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_string),
            ),
            _ => {}
        }
    }
}
