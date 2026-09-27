use super::*;

pub(super) fn parse_flight_sudo(
    cask: &Cask,
    kind: &str,
    value: Option<&Value>,
) -> Result<FlightSudo> {
    match value {
        None | Some(Value::Bool(false)) => Ok(FlightSudo::Never),
        Some(Value::Bool(true)) => Ok(FlightSudo::Always),
        Some(Value::String(value)) if value == "if_needed" => Ok(FlightSudo::IfNeeded),
        _ => bail!("brew-cask:{}: unsupported {kind} sudo setting", cask.token),
    }
}

pub(super) fn parse_flight_guards(
    cask: &Cask,
    kind: &str,
    value: Option<&Value>,
) -> Result<Vec<FlightGuard>> {
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

/// Homebrew leaves `base` off only for paths that are already absolute: `/`,
/// `~`, or an absolute template such as `{{appdir}}`. Globs are expanded only
/// inside `staged_path`, like `set_permissions`.
pub(super) fn parse_ownership_flight_path(
    cask: &Cask,
    kind: &str,
    value: &Value,
) -> Result<FlightPath> {
    let path = parse_context_flight_path_value(cask, kind, "set_ownership path", Some(value))?;
    let valid = match path.base {
        FlightPathBase::Literal => {
            path.path.starts_with('/') || path.path.starts_with("~/") || path.path.starts_with("{{")
        }
        _ => validate_flight_relative_path(&path.path).is_ok(),
    };
    if !valid {
        bail!(
            "brew-cask:{}: invalid {kind} set_ownership path {}",
            cask.token,
            path.path
        );
    }
    if path.base != FlightPathBase::StagedPath && path.path.contains(['*', '?', '[']) {
        bail!(
            "brew-cask:{}: unsupported {kind} set_ownership glob outside staged_path {}",
            cask.token,
            path.path
        );
    }
    Ok(path)
}

/// A user or group becomes one half of chown's `user:group` operand, so a
/// separator or whitespace would change what it names.
pub(super) fn parse_ownership_name(
    cask: &Cask,
    kind: &str,
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name))
            if !name.is_empty()
                && !name.starts_with('-')
                && !name
                    .chars()
                    .any(|c| c == ':' || c.is_whitespace() || c.is_control()) =>
        {
            Ok(Some(name.clone()))
        }
        Some(_) => bail!(
            "brew-cask:{}: unsupported {kind} set_ownership {field}",
            cask.token
        ),
    }
}

pub(super) fn parse_optional_flight_bool(
    cask: &Cask,
    kind: &str,
    object: &serde_json::Map<String, Value>,
    field: &str,
    default: bool,
) -> Result<bool> {
    match object.get(field) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!("brew-cask:{}: {kind} {field} must be a boolean", cask.token),
    }
}

pub fn parse_run_command(cask: &Cask, kind: &str, value: Option<&Value>) -> Result<FlightPath> {
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

pub(super) fn parse_flight_guard(cask: &Cask, kind: &str, value: &Value) -> Result<FlightGuard> {
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

pub(super) fn parse_context_flight_path(
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

pub(super) fn parse_context_flight_path_value(
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

pub(super) fn reject_unsupported_flight_fields(
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

pub(super) fn parse_flight_path(
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

/// `set_permissions` paths follow Homebrew's `remove` shape but may also
/// name the installed app for `postflight_steps`, so `appdir` is accepted
/// beside `staged_path`.
pub(super) fn parse_permissions_flight_path(
    cask: &Cask,
    kind: &str,
    value: Option<&Value>,
) -> Result<FlightPath> {
    let field = "paths";
    let object = value.and_then(Value::as_object).ok_or_else(|| {
        eyre!(
            "brew-cask:{}: unsupported {kind} {field} metadata format",
            cask.token
        )
    })?;
    let base = match object.get("base").and_then(Value::as_str) {
        Some("staged_path") => FlightPathBase::StagedPath,
        Some("appdir") => FlightPathBase::AppDir,
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
    if base == FlightPathBase::AppDir && is_flight_glob(path) {
        bail!(
            "brew-cask:{}: unsupported {kind} {field} glob outside staged_path {}",
            cask.token,
            path
        )
    }
    Ok(FlightPath {
        base,
        path: path.to_string(),
    })
}
