
Describe 'task inner-quote preservation (cmd /c)' {
    # Regression tests for https://github.com/jdx/mise/discussions/9355:
    # on Windows, inner double quotes in a task's `run` string were mangled when
    # the default `cmd /c` shell was spawned, because the script was passed as an
    # ordinary std::process::Command argument and re-quoted MSVCRT-style (inner
    # `"` -> `\"`), which cmd.exe does not understand. The child then saw e.g.
    # `\"hello world\"` instead of `"hello world"`, or `"import` instead of the
    # whole `-c` argument. The fix passes the command to cmd verbatim.

    BeforeAll {
        $originalPath = Get-Location
        $originalTrustedConfigPaths = $env:MISE_TRUSTED_CONFIG_PATHS
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        @'
[tasks.echo_quoted]
run = 'echo "hello world"'

[tasks.echo_doc]
run = 'echo "My Great Document.typ"'

[tasks.node_inner_quotes]
run = 'node -e "console.log(2 + 2)"'
'@ | Out-File -FilePath "mise.toml" -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $originalPath
        if ($null -ne $originalTrustedConfigPaths) {
            $env:MISE_TRUSTED_CONFIG_PATHS = $originalTrustedConfigPaths
        } else {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
    }

    It 'does not backslash-escape inner double quotes' {
        # The smoking gun of the bug was a stray backslash reaching the program.
        $output = mise run echo_quoted | Select-Object -Last 1
        $output | Should -Not -Match '\\'
        $output | Should -Be '"hello world"'
    }

    It 'preserves a quoted filename with spaces (discussion #9355 repro)' {
        $output = mise run echo_doc | Select-Object -Last 1
        $output | Should -Not -Match '\\'
        $output | Should -Be '"My Great Document.typ"'
    }

    It 'passes an inner-quoted -c argument through as a single argument' {
        # Mirrors the `python -c "..."` / `node -e "..."` repro: without the fix
        # the quoted script is split at the first space and the interpreter
        # fails with a syntax error instead of printing the result.
        if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
            Set-ItResult -Skipped -Because "node not on PATH"
            return
        }
        $output = mise run node_inner_quotes | Select-Object -Last 1
        $output | Should -Be '4'
    }
}
