//! Register lazy completion loaders in activated shells. Reading a manifest here
//! never executes a publisher's generator; that remains a tab-completion action.

use std::collections::BTreeSet;
use std::sync::Arc;

use usage_rs::complete::Shell;

use crate::config::Config;
use crate::toolset::Toolset;

/// Keep native paths intact while passing them through shell source as ASCII.
pub(crate) fn encode_spec_path(path: &std::path::Path) -> String {
    use base64::Engine;
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(windows)]
    let bytes: Vec<_> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn decode_spec_path(encoded: &str) -> eyre::Result<std::path::PathBuf> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded)?;
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes)
    };
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt;
        if !bytes.len().is_multiple_of(2) {
            eyre::bail!("invalid completion specification path");
        }
        let wide: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        std::ffi::OsString::from_wide(&wide)
    };
    Ok(path.into())
}

pub(crate) fn clear(shell: &str) -> &'static str {
    match shell {
        "bash" | "zsh" => {
            "if typeset -f __mise_clear_completions >/dev/null; then __mise_clear_completions; unset -f __mise_clear_completions; fi\n"
        }
        "fish" => {
            "if functions -q __mise_clear_completions; __mise_clear_completions; functions -e __mise_clear_completions; end\n"
        }
        "pwsh" => {
            "if (Test-Path Function:__mise_clear_completions) { __mise_clear_completions; Remove-Item Function:__mise_clear_completions }\n"
        }
        _ => "",
    }
}

pub(crate) fn activate(config: &Arc<Config>, ts: &Toolset, shell: &str) -> String {
    let Some(completion_shell) = Shell::from_name(shell) else {
        return String::new();
    };
    if !matches!(
        completion_shell,
        Shell::Bash | Shell::Zsh | Shell::Fish | Shell::PowerShell
    ) {
        return String::new();
    }
    let mut commands = BTreeSet::new();
    for (_, tv) in ts.list_current_installed_versions(config) {
        let install = tv.install_path();
        let statement = match super::statement(&install) {
            Ok(Some(statement)) => statement,
            Ok(None) => continue,
            Err(err) => {
                debug!("cannot load completions for {}: {err:#}", tv.short());
                continue;
            }
        };
        let artifact = crate::backend::packslip::selected_artifact(
            &statement,
            tv.request.options().get_string("variant").as_deref(),
        );
        for bin in statement
            .predicate
            .artifacts
            .iter()
            .filter(|candidate| {
                artifact
                    .as_ref()
                    .is_none_or(|selected| candidate.name == selected.name)
            })
            .flat_map(|a| &a.bin)
        {
            let name = &bin.name;
            if safe_command(name)
                && !super::completion_sources(
                    &statement,
                    &install,
                    completion_shell.as_str(),
                    artifact.as_ref(),
                    Some(name),
                )
                .is_empty()
            {
                commands.insert(name.clone());
            }
        }
    }
    registration(shell, completion_shell, &commands)
}

fn safe_command(command: &str) -> bool {
    command
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        && command
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.+".contains(&c))
}

fn registration(shell_name: &str, shell: Shell, commands: &BTreeSet<String>) -> String {
    let mut output = clear(shell_name).to_string();
    let mut cleanup = String::new();
    if commands.is_empty() {
        return output;
    }
    if shell == Shell::Zsh {
        output.push_str(
            "if ! (( $+functions[compdef] )); then autoload -Uz compinit; compinit -i; fi\n",
        );
    }
    if shell == Shell::PowerShell {
        // PowerShell exposes registration but no public lookup. Preserve the
        // existing entry when its completion table is available; if a host
        // hides it, leave its registrations alone.
        output.push_str(r#"$__mise_completion_table_ready = $false
try {
    $__mise_context = $ExecutionContext.GetType().GetField('_context', [System.Reflection.BindingFlags]'Instance,NonPublic').GetValue($ExecutionContext)
    $__mise_completion_table = $__mise_context.GetType().GetProperty('NativeArgumentCompleters', [System.Reflection.BindingFlags]'Instance,NonPublic').GetValue($__mise_context)
    $__mise_completion_table_ready = $true
} catch {}
if ($__mise_completion_table_ready) {
"#);
    }
    for command in commands {
        let Ok(stub) = super::stub(command, shell) else {
            continue;
        };
        let saved = format!("__mise_completion_{}", crate::hash::hash_to_str(command));
        match shell {
            Shell::Bash => {
                output.push_str(&format!(
                    "{saved}=$(complete -p '{command}' 2>/dev/null || :)\n{stub}"
                ));
                let ident = super::completion_ident(command);
                cleanup.push_str(&format!(
                    "if typeset -f __mise_complete_{ident}_restub >/dev/null; then __mise_complete_{ident}_restub; unset -f __mise_complete_{ident}_restub; fi\ncomplete -r '{command}' 2>/dev/null || :\nif [[ -n ${{{saved}:-}} ]]; then eval \"${{{saved}}}\"; fi\nunset {saved}\n"
                ));
            }
            Shell::Zsh => {
                output.push_str(&format!(
                    "typeset -g {saved}=\"${{_comps[{command}]-}}\"\ntypeset -g {saved}_function=\"${{functions[_{command}]-}}\"\n_{command}() {{\n{stub}\n}}\ncompdef _{command} '{command}'\n"
                ));
                cleanup.push_str(&format!(
                    "if [[ -n ${{{saved}:-}} ]]; then compdef \"${{{saved}}}\" '{command}'; else compdef -d '{command}'; fi\nif [[ -n ${{{saved}_function:-}} ]]; then functions[_{command}]=\"${{{saved}_function}}\"; else unfunction _{command}; fi\nunset {saved} {saved}_function\n"
                ));
            }
            Shell::Fish => {
                output.push_str(&format!(
                    "set -g {saved} (complete -c '{command}')\ncomplete -e -c '{command}'\n{stub}"
                ));
                cleanup.push_str(&format!("complete -e -c '{command}'\nfor rule in ${saved}; eval $rule; end\nset -e {saved}\n"));
            }
            Shell::PowerShell => {
                output.push_str(&format!("$global:{saved} = if ($null -ne $__mise_completion_table) {{ $__mise_completion_table['{command}'] }} else {{ $null }}\n{stub}"));
                cleanup.push_str(&format!("Register-ArgumentCompleter -Native -CommandName '{command}' -ScriptBlock $global:{saved}\nRemove-Variable -Scope Global -Name '{saved}' -ErrorAction Ignore\n"));
            }
            _ => {}
        }
    }
    match shell {
        Shell::Fish => output.push_str(&format!(
            "function __mise_clear_completions\n{cleanup}end\n"
        )),
        Shell::PowerShell => output.push_str(&format!(
            "function global:__mise_clear_completions {{\n{cleanup}}}\n"
        )),
        _ => output.push_str(&format!("__mise_clear_completions() {{\n{cleanup}}}\n")),
    }
    if shell == Shell::PowerShell {
        output.push_str("}\nRemove-Variable __mise_context, __mise_completion_table, __mise_completion_table_ready -ErrorAction Ignore\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn specification_paths_preserve_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/spec-\xff.kdl"));
        assert_eq!(decode_spec_path(&encode_spec_path(path)).unwrap(), path);
    }

    #[test]
    fn different_commands_have_distinct_loader_names() {
        assert_ne!(
            super::super::completion_ident("my-tool"),
            super::super::completion_ident("my_tool")
        );
    }

    #[test]
    fn manifest_command_names_cannot_inject_shell_code() {
        for command in ["hk", "my-tool", "g++", "tool.exe"] {
            assert!(safe_command(command));
        }
        for command in [
            "",
            "tool;touch injected",
            "tool'",
            "$(id)",
            "bin/tool",
            "tool\n",
            "-tool",
        ] {
            assert!(!safe_command(command));
        }
    }
}
