Describe 'sandbox deny_env' {
    # On Windows, `--deny-env` and a task's `deny_env` used to leave the inherited environment
    # alone, so the child saw every variable mise had. It must now see only what mise computed,
    # the variables Windows needs to start a process, and anything allowed by name.
    # cmd leaves an undefined `%NAME%` as written, which is how these tests tell "unset".

    BeforeAll {
        $originalPath = Get-Location
        $originalTrustedConfigPaths = $env:MISE_TRUSTED_CONFIG_PATHS
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $env:MISE_DENY_ENV_PROBE = 'leaked'

        $script:binDir = Join-Path $TestDrive 'deny-env-bin'
        New-Item -ItemType Directory -Path $script:binDir | Out-Null

        @"
[env]
_.path = ['$($script:binDir)']

[tasks.denied]
run = 'echo [%MISE_DENY_ENV_PROBE%] [%SystemRoot%]'
deny_env = true

# A timeout starts the task through a Ctrl+C group leader, which must not hand on mise's
# environment either.
[tasks.denied_timeout]
run = 'echo [%MISE_DENY_ENV_PROBE%] [%SystemRoot%]'
deny_env = true
timeout = "1m"

[tasks.allowed]
run = 'echo [%MISE_DENY_ENV_PROBE%]'
allow_env = ['MISE_DENY_ENV_PROBE']

[tasks.open]
run = 'echo [%MISE_DENY_ENV_PROBE%]'
"@ | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $originalPath
        Remove-Item -Path Env:\MISE_DENY_ENV_PROBE -ErrorAction SilentlyContinue
        if ($null -ne $originalTrustedConfigPaths) {
            $env:MISE_TRUSTED_CONFIG_PATHS = $originalTrustedConfigPaths
        } else {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
    }

    It 'passes inherited variables through without deny-env' {
        $output = mise x -- cmd /c 'echo [%MISE_DENY_ENV_PROBE%]' | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be '[leaked]'
    }

    It 'clears inherited variables for mise x --deny-env' {
        $output = mise x --deny-env -- cmd /c 'echo [%MISE_DENY_ENV_PROBE%]' | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be '[%MISE_DENY_ENV_PROBE%]'
    }

    It 'keeps what a Windows process needs under mise x --deny-env' {
        $output = mise x --deny-env -- cmd /c 'echo [%SystemRoot%] [%ComSpec%]' | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be "[$env:SystemRoot] [$env:ComSpec]"
    }

    It 'keeps the PATH mise built under mise x --deny-env' {
        $output = mise x --deny-env -- cmd /c 'echo %PATH%' | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output.ToLowerInvariant().Contains($script:binDir.ToLowerInvariant()) | Should -BeTrue
    }

    It 'passes a variable named by mise x --allow-env' {
        $output = mise x --allow-env MISE_DENY_ENV_PROBE -- cmd /c 'echo [%MISE_DENY_ENV_PROBE%]' | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be '[leaked]'
    }

    It 'passes inherited variables to a task without deny_env' {
        $output = mise run open | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be '[leaked]'
    }

    It 'clears inherited variables for a deny_env task' {
        $output = mise run denied | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be "[%MISE_DENY_ENV_PROBE%] [$env:SystemRoot]"
    }

    It 'clears inherited variables for a deny_env task with a timeout' {
        $output = mise run denied_timeout | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be "[%MISE_DENY_ENV_PROBE%] [$env:SystemRoot]"
    }

    It 'passes a variable named by a task allow_env' {
        $output = mise run allowed | Select-Object -Last 1
        $LASTEXITCODE | Should -Be 0
        $output | Should -Be '[leaked]'
    }
}
