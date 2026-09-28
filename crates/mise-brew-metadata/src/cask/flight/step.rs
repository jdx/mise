use super::*;

pub fn parse_flight_step(cask: &Cask, kind: &str, value: &Value) -> Result<FlightStep> {
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
        "set_permissions" => {
            reject_unsupported_flight_fields(
                cask,
                kind,
                "set_permissions step",
                object,
                &["type", "paths", "permissions", "non_recursive"],
            )?;
            let paths = object
                .get("paths")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: unsupported {kind} set_permissions step metadata format",
                        cask.token
                    )
                })?
                .iter()
                .map(|path| parse_permissions_flight_path(cask, kind, Some(path)))
                .collect::<Result<Vec<_>>>()?;
            let permissions = object
                .get("permissions")
                .and_then(Value::as_str)
                .filter(|permissions| !permissions.is_empty())
                .ok_or_else(|| {
                    eyre!(
                        "brew-cask:{}: unsupported {kind} set_permissions step permissions",
                        cask.token
                    )
                })?;
            // Homebrew serializes the DSL's `recursive: true` default as an
            // absent `non_recursive`, so only an explicit `true` narrows it.
            let recursive =
                !parse_optional_flight_bool(cask, kind, object, "non_recursive", false)?;
            Ok(FlightStep::SetPermissions {
                paths,
                permissions: permissions.to_string(),
                recursive,
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
                        "brew-cask:{}: unsupported {kind} set_ownership step metadata format",
                        cask.token
                    )
                })?
                .iter()
                .map(|path| parse_ownership_flight_path(cask, kind, path))
                .collect::<Result<Vec<_>>>()?;
            // Homebrew omits the default `staff` group and the `recursive: true`
            // default when serializing.
            let user = parse_ownership_name(cask, kind, object, "user")?;
            let group = parse_ownership_name(cask, kind, object, "group")?
                .unwrap_or_else(|| "staff".into());
            let recursive =
                !parse_optional_flight_bool(cask, kind, object, "non_recursive", false)?;
            Ok(FlightStep::SetOwnership {
                paths,
                user,
                group,
                recursive,
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
                    "must_succeed",
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
            let guards = parse_flight_guards(cask, kind, object.get("guards"))?;
            Ok(FlightStep::Run {
                must_succeed: parse_optional_flight_bool(cask, kind, object, "must_succeed", true)?,
                command: parse_run_command(cask, kind, object.get("command"))?,
                args,
                env,
                sudo: parse_optional_flight_bool(cask, kind, object, "sudo", false)?,
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
