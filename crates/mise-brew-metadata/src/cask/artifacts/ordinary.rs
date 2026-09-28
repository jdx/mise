use super::*;

pub fn parse_command_wrapper_artifact(value: &Value) -> Result<Option<CommandWrapperArtifact>> {
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

pub fn parse_pkg_artifact(value: &Value) -> Result<Option<PkgArtifact>> {
    let Some(pkg) = value.as_object().and_then(|o| o.get("pkg")) else {
        return Ok(None);
    };
    match pkg {
        Value::String(source) => Ok(Some(PkgArtifact {
            source: source.clone(),
            choices: Vec::new(),
        })),
        Value::Array(values) => {
            if values.len() > 2 {
                bail!("brew-cask: pkg artifact metadata has unexpected entries");
            }
            let Some(source) = values.first().and_then(Value::as_str) else {
                return Ok(None);
            };
            let choices = match values.get(1) {
                Some(options) => parse_pkg_choices(options)?,
                None => Vec::new(),
            };
            Ok(Some(PkgArtifact {
                source: source.to_string(),
                choices,
            }))
        }
        _ => Ok(None),
    }
}

/// Homebrew's pkg stanza accepts only `choices` besides the deprecated
/// `allow_untrusted`. Homebrew writes each choice verbatim into the plist for
/// `installer -applyChoiceChangesXML`, which needs all three keys and a setting
/// that suits the attribute, so an incomplete, unknown, or mismatched choice is
/// rejected here rather than by `installer`.
fn parse_pkg_choices(options: &Value) -> Result<Vec<PkgChoice>> {
    let options = options
        .as_object()
        .ok_or_else(|| eyre!("brew-cask: pkg options must be an object"))?;
    reject_unsupported_artifact_fields("pkg", options, &["choices"])?;
    let Some(choices) = options.get("choices") else {
        return Ok(Vec::new());
    };
    choices
        .as_array()
        .ok_or_else(|| eyre!("brew-cask: pkg choices must be an array"))?
        .iter()
        .map(|choice| {
            let choice = choice
                .as_object()
                .ok_or_else(|| eyre!("brew-cask: pkg choices must be objects"))?;
            reject_unsupported_artifact_fields(
                "pkg choice",
                choice,
                &["choiceIdentifier", "choiceAttribute", "attributeSetting"],
            )?;
            let string_field = |field: &str| {
                choice
                    .get(field)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| eyre!("brew-cask: pkg choice {field} must be a string"))
            };
            let identifier = string_field("choiceIdentifier")?.to_string();
            let attribute = string_field("choiceAttribute")?;
            let setting = choice.get("attributeSetting");
            let flag = || match setting.and_then(Value::as_i64) {
                Some(0) => Ok(false),
                Some(1) => Ok(true),
                _ => bail!("brew-cask: pkg choice {attribute} setting must be 0 or 1"),
            };
            let change = match attribute {
                "selected" => PkgChoiceChange::Selected(flag()?),
                "enabled" => PkgChoiceChange::Enabled(flag()?),
                "visible" => PkgChoiceChange::Visible(flag()?),
                "customLocation" => PkgChoiceChange::CustomLocation(
                    setting
                        .and_then(Value::as_str)
                        .filter(|path| !path.is_empty())
                        .ok_or_else(|| {
                            eyre!("brew-cask: pkg choice customLocation setting must be a path")
                        })?
                        .to_string(),
                ),
                _ => bail!("brew-cask: unsupported pkg choice attribute {attribute}"),
            };
            Ok(PkgChoice { identifier, change })
        })
        .collect()
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
    reject_unsupported_artifact_fields(
        "installer script",
        script,
        &["executable", "args", "sudo", "print_stderr"],
    )?;
    let executable = script
        .get("executable")
        .and_then(Value::as_str)
        .ok_or_else(|| eyre!("brew-cask: installer script requires an executable"))?;
    let args = string_args(script, "installer script")?;
    let sudo = optional_installer_bool(script, "sudo")?;
    // Homebrew only uses `print_stderr: false` to hide a noisy installer's
    // stderr. Validate it, but leave the output visible.
    optional_installer_bool(script, "print_stderr")?;
    Ok(Some(InstallerArtifact {
        executable: executable.to_string(),
        args,
        sudo,
    }))
}

fn optional_installer_bool(object: &serde_json::Map<String, Value>, field: &str) -> Result<bool> {
    match object.get(field) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!("brew-cask: installer script {field} must be a boolean"),
    }
}

/// `kind` names the declaring artifact, so errors read e.g. "installer script
/// args must be an array".
fn string_args(object: &serde_json::Map<String, Value>, kind: &str) -> Result<Vec<String>> {
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

fn reject_unsupported_artifact_fields(
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

pub fn parse_generic_artifact(value: &Value) -> Result<Option<GenericArtifact>> {
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

pub fn parse_font_artifact(value: &Value) -> Option<FontArtifact> {
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

fn parse_declared_completion_artifact(
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

pub fn parse_generated_completion_artifact(
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

fn default_generated_completion_shells(format: Option<&str>) -> Vec<CompletionShell> {
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
