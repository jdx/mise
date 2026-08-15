#![allow(unknown_lints)]
use crate::config::Settings;
use std::borrow::Cow;
use std::fmt::Display;

use indoc::formatdoc;

use crate::shell::{self, ActivateOptions, Shell};

#[derive(Default)]
pub struct Pwsh {}

impl Pwsh {}

impl Shell for Pwsh {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;

        // Single-quoted rather than double: PowerShell expands `$name` and honours backticks
        // inside `"..."`, so a mise installed under a directory holding either character was
        // invoked at a mangled path — `& "C:\...\a$b\mise.exe"` resolves to `C:\...\a\mise.exe`.
        // A path wants no expansion at all, which is exactly what `'...'` gives.
        let exe = escape_sq(&exe.to_string_lossy()).into_owned();
        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));
        out.push_str(&formatdoc! {r#"
            $env:MISE_SHELL = 'pwsh'
            if (-not (Test-Path -Path Env:/__MISE_ORIG_PATH)) {{
                $env:__MISE_ORIG_PATH = $env:PATH
            }}

            function mise {{
                [CmdletBinding()]
                param(
                    [Parameter(ValueFromRemainingArguments=$true)]  # Allow any number of arguments, including none
                    [string[]] $arguments = @()  # defaults to an empty array: a bare `mise` binds $null, which Set-StrictMode rejects on .count
                )

                $previous_out_encoding = $OutputEncoding
                $previous_console_out_encoding = [Console]::OutputEncoding
                $OutputEncoding = [Console]::OutputEncoding = [Text.UTF8Encoding]::UTF8

                function _reset_output_encoding {{
                    $OutputEncoding = $previous_out_encoding
                    [Console]::OutputEncoding = $previous_console_out_encoding
                }}

                if ($arguments.count -eq 0) {{
                    & '{exe}'
                    _reset_output_encoding
                    return
                }} elseif ($arguments -contains '-h' -or $arguments -contains '--help') {{
                    & '{exe}' @arguments
                    _reset_output_encoding
                    return
                }}

                $command = $arguments[0]
                if ($arguments.Length -gt 1) {{
                    $remainingArgs = $arguments[1..($arguments.Length - 1)]
                }} else {{
                    $remainingArgs = @()
                }}

                switch ($command) {{
                    {{ $_ -in 'deactivate', 'shell', 'sh' }} {{
                        & '{exe}' $command @remainingArgs | Out-String | Invoke-Expression -ErrorAction SilentlyContinue
                        _reset_output_encoding
                    }}
                    default {{
                        & '{exe}' $command @remainingArgs
                        if ($(Test-Path -Path Function:\_mise_hook)){{
                            _mise_hook
                        }}
                        _reset_output_encoding
                    }}
                }}
            }}
            "#});

        if !opts.no_hook_env {
            out.push_str(&formatdoc! {r#"

            function Global:_mise_hook {{
                if ($env:MISE_SHELL -eq "pwsh"){{
                    $status = $global:LASTEXITCODE
                    $output = & '{exe}' hook-env{flags} $args -s pwsh | Out-String
                    if ($output -and $output.Trim()) {{
                        $output | Invoke-Expression
                    }}
                    # mise hook-env will have set $LASTEXITCODE, restore previous value
                    $global:LASTEXITCODE = $status
                }}
            }}

            function __enable_mise_chpwd{{
                if ($PSVersionTable.PSVersion.Major -lt 7) {{
                    if ($env:MISE_PWSH_CHPWD_WARNING -ne '0') {{
                        Write-Warning "mise: chpwd functionality requires PowerShell version 7 or higher. Your current version is $($PSVersionTable.PSVersion). You can add `$env:MISE_PWSH_CHPWD_WARNING=0` to your environment to disable this warning."
                    }}
                    return
                }}
                if (-not (Test-Path variable:global:__mise_pwsh_chpwd)){{
                    $Global:__mise_pwsh_chpwd= $true
                    $_mise_chpwd_hook = [EventHandler[System.Management.Automation.LocationChangedEventArgs]] {{
                        param([object] $source, [System.Management.Automation.LocationChangedEventArgs] $eventArgs)
                        end {{
                            _mise_hook
                        }}
                    }};
                    $__mise_pwsh_previous_chpwd_function=$ExecutionContext.SessionState.InvokeCommand.LocationChangedAction;

                    if ($__mise_pwsh_previous_chpwd_function) {{
                        $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction = [Delegate]::Combine($__mise_pwsh_previous_chpwd_function, $_mise_chpwd_hook)
                    }}
                    else {{
                        $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction = $_mise_chpwd_hook
                    }}
                }}
            }}
            __enable_mise_chpwd
            Remove-Item -ErrorAction SilentlyContinue -Path Function:/__enable_mise_chpwd

            function __enable_mise_prompt {{
                if (-not (Test-Path variable:global:__mise_pwsh_previous_prompt_function)){{
                    $Global:__mise_pwsh_previous_prompt_function=$function:prompt
                    function global:prompt {{
                        if (Test-Path -Path Function:\_mise_hook){{
                            _mise_hook
                        }}
                        & $__mise_pwsh_previous_prompt_function
                    }}
                }}
            }}
            __enable_mise_prompt
            Remove-Item -ErrorAction SilentlyContinue -Path Function:/__enable_mise_prompt

            _mise_hook
            "#});
        }
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if (-not (Test-Path variable:global:__mise_pwsh_command_not_found)){{
                $Global:__mise_pwsh_command_not_found= $true
                function __enable_mise_command_not_found {{
                    $_mise_pwsh_cmd_not_found_hook = [EventHandler[System.Management.Automation.CommandLookupEventArgs]] {{
                        param([object] $Name, [System.Management.Automation.CommandLookupEventArgs] $eventArgs)
                        end {{
                            # Only auto-install when the missing command is what the
                            # user actually typed. PSReadLine is absent in
                            # non-interactive sessions, and even when its module is
                            # loaded GetHistoryItems() throws until the line editor
                            # initializes, so treat "cannot tell" as "not typed".
                            $lastCommand = $null
                            try {{
                                $psReadLine = 'Microsoft.PowerShell.PSConsoleReadLine' -as [type]
                                if ($psReadLine) {{
                                    $history = @($psReadLine::GetHistoryItems())
                                    if ($history.Count -gt 0) {{
                                        $lastCommand = $history[-1].CommandLine
                                    }}
                                }}
                            }} catch {{ }}
                            # compare whole tokens: a substring match would fire for
                            # `mise` when the user typed `premise`, while matching only
                            # the first token would miss `... | some-missing-tool`
                            if ($lastCommand -and (($lastCommand -split '\s+') -contains $Name)) {{
                                if (& '{exe}' hook-not-found -s pwsh -- $Name){{
                                    _mise_hook
                                    if (Get-Command $Name -ErrorAction SilentlyContinue){{
                                        $EventArgs.Command = Get-Command $Name
                                        $EventArgs.StopSearch = $true
                                    }}
                                }}
                            }}
                        }}
                    }}
                    $current_command_not_found_function = $ExecutionContext.SessionState.InvokeCommand.CommandNotFoundAction
                    if ($current_command_not_found_function) {{
                        $ExecutionContext.SessionState.InvokeCommand.CommandNotFoundAction = [Delegate]::Combine($current_command_not_found_function, $_mise_pwsh_cmd_not_found_hook)
                    }}
                    else {{
                        $ExecutionContext.SessionState.InvokeCommand.CommandNotFoundAction = $_mise_pwsh_cmd_not_found_hook
                    }}
                }}
                __enable_mise_command_not_found
                Remove-Item -ErrorAction SilentlyContinue -Path Function:/__enable_mise_command_not_found
            }}
            "#});
        }
        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
        Remove-Item -ErrorAction SilentlyContinue function:mise
        Remove-Item -ErrorAction SilentlyContinue -Path Env:/MISE_SHELL
        Remove-Item -ErrorAction SilentlyContinue -Path Env:/__MISE_DIFF
        Remove-Item -ErrorAction SilentlyContinue -Path Env:/__MISE_SESSION
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = escape_env_name(k);
        let v = escape_sq(v);
        format!("${{Env:{k}}}='{v}'\n")
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        let k = escape_env_name(k);
        let v = escape_sq(v);
        format!("${{Env:{k}}}='{v}'+[IO.Path]::PathSeparator+${{env:{k}}}\n")
    }

    fn unset_env(&self, k: &str) -> String {
        // A cmdlet argument rather than a variable reference, so this one is an ordinary
        // single-quoted string. `-LiteralPath` rather than `-Path` because quoting only settles
        // how PowerShell *parses* the argument -- `Remove-Item` still globs `*`, `?` and `[...]`
        // in a `-Path`, so removing a variable named `*` would take every other one with it.
        let k = escape_sq(k);
        format!("Remove-Item -ErrorAction SilentlyContinue -LiteralPath 'Env:/{k}'\n")
    }
}

impl Display for Pwsh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pwsh")
    }
}

/// Quote `input` for a PowerShell single-quoted string literal, without the surrounding quotes.
///
/// Inside `'...'` every character is literal except `'`, which is written by doubling it. A
/// backtick is *not* an escape there — that is only true inside `"..."` — so emitting `` `' ``
/// left the quote closing the literal, and the line failed to parse rather than carrying an
/// apostrophe. Allocates only when there is a quote to double, the way `xonsh_escape_sq` in the
/// xonsh backend does for Python's rules.
fn escape_sq(input: &str) -> Cow<'_, str> {
    if input.contains('\'') {
        Cow::Owned(input.replace('\'', "''"))
    } else {
        Cow::Borrowed(input)
    }
}

/// Quote an environment variable name for the `${Env:NAME}` form.
///
/// The braces are what let a name through that bare `$Env:NAME` cannot parse — anything holding
/// a space, say, which `[env]` in a config accepts. Inside them a backtick escapes the next
/// character, so a backtick and the closing brace are the two that have to be escaped;
/// everything else, apostrophes included, is literal.
fn escape_env_name(name: &str) -> Cow<'_, str> {
    if name.contains(['`', '}']) {
        Cow::Owned(name.replace('`', "``").replace('}', "`}"))
    } else {
        Cow::Borrowed(name)
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use std::path::Path;
    use test_log::test;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_activate() {
        // Unset __MISE_ORIG_PATH to avoid PATH restoration logic in output
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }

        let pwsh = Pwsh::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(pwsh.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Pwsh::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let pwsh = Pwsh::default();
        assert_snapshot!(replace_path(&pwsh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    /// The defect: an apostrophe closed the literal, so the line did not parse. Only `'` is
    /// special inside `'...'` -- escaping anything else would be the same mistake in reverse,
    /// turning a literal backtick or `$` into something PowerShell acts on.
    #[test]
    fn test_set_env_escapes_single_quotes_only() {
        let pwsh = Pwsh::default();
        assert_eq!(
            pwsh.set_env("HOME_ISH", r"C:\Users\O'Brien\tools"),
            "${Env:HOME_ISH}='C:\\Users\\O''Brien\\tools'\n"
        );
        // literal inside a single-quoted string, so they pass through untouched
        assert_eq!(
            pwsh.set_env("RAW", "a`b $c \"d\" e\\f"),
            "${Env:RAW}='a`b $c \"d\" e\\f'\n"
        );
        assert_eq!(pwsh.set_env("EMPTY", ""), "${Env:EMPTY}=''\n");
        // two apostrophes in one value, and one at each edge
        assert_eq!(pwsh.set_env("K", "'a'b'"), "${Env:K}='''a''b'''\n");
    }

    /// PATH is the value that matters most here: it carries the user's home directory, so an
    /// apostrophe in a Windows username reached every `hook-env`.
    #[test]
    fn test_prepend_env_escapes_single_quotes() {
        assert_eq!(
            Pwsh::default().prepend_env("PATH", r"C:\Users\O'Brien\bin"),
            "${Env:PATH}='C:\\Users\\O''Brien\\bin'+[IO.Path]::PathSeparator+${env:PATH}\n"
        );
    }

    /// `$Env:NAME` cannot parse a name with a space, which `[env]` in a config accepts; the
    /// braced form can. Inside the braces a backtick escapes the next character, so it and the
    /// closing brace are escaped and an apostrophe is left alone.
    #[test]
    fn test_env_names_use_the_braced_form() {
        let pwsh = Pwsh::default();
        assert_eq!(pwsh.set_env("MY VAR", "x"), "${Env:MY VAR}='x'\n");
        assert_eq!(pwsh.set_env("WEIRD'KEY", "x"), "${Env:WEIRD'KEY}='x'\n");
        assert_eq!(pwsh.set_env("A}B", "x"), "${Env:A`}B}='x'\n");
        assert_eq!(pwsh.set_env("A`B", "x"), "${Env:A``B}='x'\n");
    }

    /// `unset_env` builds a `-Path` argument rather than a variable reference, so it is an
    /// ordinary single-quoted string and takes the value rule, not the name rule.
    #[test]
    fn test_unset_env_quotes_the_path() {
        assert_eq!(
            Pwsh::default().unset_env("MY VAR"),
            "Remove-Item -ErrorAction SilentlyContinue -LiteralPath 'Env:/MY VAR'\n"
        );
        assert_eq!(
            Pwsh::default().unset_env("WEIRD'KEY"),
            "Remove-Item -ErrorAction SilentlyContinue -LiteralPath 'Env:/WEIRD''KEY'\n"
        );
    }

    /// Quoting settles parsing, not globbing: `Remove-Item -Path 'Env:/*'` still matches every
    /// variable and removes them all. Measured -- with `-Path` two unrelated probe variables were
    /// wiped, with `-LiteralPath` they survived and only the one named `*` went.
    #[test]
    fn test_unset_env_does_not_glob_the_name() {
        for name in ["*", "?", "PRE[FIX]"] {
            let out = Pwsh::default().unset_env(name);
            assert!(out.contains("-LiteralPath"), "{out}");
            assert!(!out.contains(" -Path "), "{out}");
            assert!(out.contains(&format!("'Env:/{name}'")), "{out}");
        }
    }

    /// A `$` is legal in a Windows directory name, and the exe path used to be interpolated
    /// into a double-quoted string, where PowerShell expanded it away.
    #[test]
    fn test_activate_invokes_the_exe_through_a_single_quoted_path() {
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }
        let out = Pwsh::default().activate(ActivateOptions {
            exe: Path::new(r"C:\Users\me\a$b\mise.exe").to_path_buf(),
            flags: "".into(),
            no_hook_env: false,
            prelude: vec![],
        });
        assert!(out.contains(r"& 'C:\Users\me\a$b\mise.exe'"), "{out}");
        assert!(!out.contains(r#"& "C:\Users\me\a$b\mise.exe""#), "{out}");
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Pwsh::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Pwsh::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
