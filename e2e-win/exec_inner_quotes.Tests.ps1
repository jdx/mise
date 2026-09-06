Describe 'exec / exec() inner-quote preservation (cmd /c)' {
    # Regression tests for https://github.com/jdx/mise/discussions/9355, extended
    # beyond inline tasks/hooks to `mise exec -c` and the tera `exec()` template
    # function. On Windows these spawn the default `cmd /c` shell; the command
    # must reach cmd verbatim so inner double quotes survive, instead of being
    # MSVCRT-requoted (inner `"` -> `\"`), which cmd.exe does not understand.

    BeforeAll {
        $originalPath = Get-Location
        $originalTrustedConfigPaths = $env:MISE_TRUSTED_CONFIG_PATHS
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
    }

    AfterAll {
        Set-Location $originalPath
        if ($null -ne $originalTrustedConfigPaths) {
            $env:MISE_TRUSTED_CONFIG_PATHS = $originalTrustedConfigPaths
        } else {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
    }

    It 'preserves inner double quotes in mise exec -c' {
        # cmd's echo prints the quotes literally; the bug produced `\"...\"`.
        $output = mise exec -c 'echo "hello world"' | Select-Object -Last 1
        $output | Should -Not -Match '\\'
        $output | Should -Be '"hello world"'
    }

    It 'passes an inner-quoted -c argument through to a program (mise exec -c)' {
        # Without the fix the quoted script is split at the first space and node
        # fails with a syntax error instead of printing the result.
        if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
            Set-ItResult -Skipped -Because "node not on PATH"
            return
        }
        $output = mise exec -c 'node -e "console.log(2 + 2)"' | Select-Object -Last 1
        $output | Should -Be '4'
    }

    It 'preserves inner double quotes in the exec() template function' {
        @'
[env]
EXEC_QUOTED = "{{ exec(command='echo \"a b\"') }}"
'@ | Out-File -FilePath "mise.toml" -Encoding utf8NoBOM
        # Parse JSON so the assertion sees the real value, not shell-quoted text.
        # cmd's echo keeps the quotes, so the preserved value is exactly `"a b"`;
        # a dropped-quote regression would yield `a b` and fail this.
        $value = (mise env --json | ConvertFrom-Json).EXEC_QUOTED
        $value | Should -Be '"a b"'
        $value | Should -Not -Match '\\'
    }
}
