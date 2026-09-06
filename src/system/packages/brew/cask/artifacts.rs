use super::*;

pub(super) fn cask_artifacts(cask: &Cask) -> Result<CaskArtifacts> {
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

pub(super) fn validate_platform_support(cask: &Cask, artifacts: &CaskArtifacts) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let font_only = !artifacts.fonts.is_empty()
            && artifacts.apps.is_empty()
            && artifacts.binaries.is_empty()
            && artifacts.command_wrappers.is_empty()
            && artifacts.pkgs.is_empty()
            && artifacts.installers.is_empty()
            && artifacts.generic.is_empty()
            && artifacts.completions.is_empty()
            && artifacts.generated_completions.is_empty()
            && artifacts.preflight_steps.is_empty()
            && artifacts.postflight_steps.is_empty()
            && !has_lifecycle_hook(cask, "preflight")
            && !has_lifecycle_hook(cask, "postflight");
        if !font_only {
            bail!(
                "brew-cask:{}: only font-only casks without lifecycle hooks are supported on linux",
                cask.token
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (cask, artifacts);
    Ok(())
}

pub(super) fn platform_unavailable_state(
    cask: &Cask,
    artifacts: &CaskArtifacts,
) -> Option<PackageState> {
    validate_platform_support(cask, artifacts)
        .err()
        .map(|err| PackageState::unavailable(err.to_string()))
}

pub(super) fn declared_target(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|o| o.get("target"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn artifact_target(value: &Value, values: &[Value]) -> Option<String> {
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
pub(super) fn artifact_source_target(
    value: &Value,
    artifact: &Value,
) -> Option<(String, Option<String>)> {
    match artifact {
        Value::String(source) => Some((source.clone(), None)),
        Value::Array(values) => Some((
            values.first()?.as_str()?.to_string(),
            artifact_target(value, values),
        )),
        _ => None,
    }
}

pub(super) fn parse_app_artifact(value: &Value) -> Option<AppArtifact> {
    let (source, target) = artifact_source_target(value, value.as_object()?.get("app")?)?;
    Some(AppArtifact { source, target })
}

pub(super) fn parse_binary_artifact(value: &Value) -> Option<BinaryArtifact> {
    let (source, target) = artifact_source_target(value, value.as_object()?.get("binary")?)?;
    Some(BinaryArtifact {
        source,
        target: target.or_else(|| declared_target(value)),
    })
}

pub(super) fn parse_command_wrapper_artifact(
    value: &Value,
) -> Result<Option<CommandWrapperArtifact>> {
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
    let args = string_args(options, "command_wrapper")?;
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
                .collect::<Result<BTreeMap<_, _>>>()
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

pub(super) fn parse_pkg_artifact(value: &Value) -> Result<Option<PkgArtifact>> {
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

pub(super) fn parse_installer_artifact(value: &Value) -> Result<Option<InstallerArtifact>> {
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
    let args = string_args(script, "installer script")?;
    Ok(Some(InstallerArtifact {
        executable: executable.to_string(),
        args,
    }))
}

/// `kind` names the declaring artifact, so errors read e.g. "installer script
/// args must be an array".
pub(super) fn string_args(
    object: &serde_json::Map<String, Value>,
    kind: &str,
) -> Result<Vec<String>> {
    let Some(args) = object.get("args") else {
        return Ok(Vec::new());
    };
    args.as_array()
        .ok_or_else(|| eyre!("brew-cask: {kind} args must be an array"))?
        .iter()
        .map(|arg| {
            arg.as_str()
                .map(str::to_string)
                .ok_or_else(|| eyre!("brew-cask: {kind} args must be strings"))
        })
        .collect()
}

pub(super) fn reject_unsupported_artifact_fields(
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

pub(super) fn parse_generic_artifact(value: &Value) -> Result<Option<GenericArtifact>> {
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

pub(super) fn parse_font_artifact(value: &Value) -> Option<FontArtifact> {
    let (source, target) = artifact_source_target(value, value.as_object()?.get("font")?)?;
    Some(FontArtifact { source, target })
}

pub(super) fn parse_completion_artifact(value: &Value) -> Result<Option<CompletionArtifact>> {
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

pub(super) fn parse_declared_completion_artifact(
    value: &Value,
    completion: &Value,
    shell: CompletionShell,
) -> Result<Option<CompletionArtifact>> {
    let Some((source, target)) = artifact_source_target(value, completion) else {
        return Ok(None);
    };
    Ok(Some(CompletionArtifact {
        shell,
        source,
        target: target.or_else(|| declared_target(value)),
    }))
}

pub(super) fn parse_generated_completion_artifact(
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
    if let Some(options) = options {
        reject_unsupported_artifact_fields(
            "generate_completions_from_executable",
            options,
            &["base_name", "shell_parameter_format", "shells"],
        )?;
    }
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

pub(super) fn default_generated_completion_shells(format: Option<&str>) -> Vec<CompletionShell> {
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

pub(super) fn parse_flight_steps(
    cask: &Cask,
    value: &Value,
    kind: &str,
) -> Result<Option<Vec<FlightStep>>> {
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

pub(super) fn parse_flight_step(cask: &Cask, kind: &str, value: &Value) -> Result<FlightStep> {
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
            let guards = parse_flight_guards(cask, kind, object.get("guards"))?;
            Ok(FlightStep::Run {
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

pub(super) fn parse_run_command(
    cask: &Cask,
    kind: &str,
    value: Option<&Value>,
) -> Result<FlightPath> {
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

pub(super) fn collect_pkg_receipt_ids(value: &Value, pkg_ids: &mut Vec<String>) {
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
