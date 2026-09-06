Describe 'mise env default shell' {
    BeforeAll {
        $script:originalPath = Get-Location
        # The workflow sets these for the whole job and Pester runs every suite in one process, so
        # put back what was there rather than dropping it.
        $script:originalEnv = @{}
        foreach ($name in 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_SHELL', 'SHELL') {
            $script:originalEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

        $script:testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:testDir | Out-Null
        Set-Location $script:testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:testDir
        Set-Content -LiteralPath (Join-Path $script:testDir 'mise.toml') -Value @(
            '[env]'
            "MISE_E2E_DEFAULT_SHELL = 'probe-value'"
        )

        # This is about what mise emits with nothing to detect from. `MISE_SHELL` is what an
        # activated session sets and `SHELL` is what Git Bash sets, so either one inherited from
        # the runner would make the case below pass vacuously.
        Remove-Item -Path Env:\MISE_SHELL -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\SHELL -ErrorAction SilentlyContinue
    }

    AfterAll {
        Set-Location $script:originalPath
        foreach ($name in $script:originalEnv.Keys) {
            if ($null -eq $script:originalEnv[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:originalEnv[$name], 'Process')
            }
        }
    }

    It 'emits PowerShell syntax rather than bash' {
        # `COMSPEC` is all mise reads on Windows without `SHELL`, and it names cmd.exe, which mise
        # has no implementation for. The fallback used to be bash, so a PowerShell session running
        # `mise env` was handed `export MISE_E2E_DEFAULT_SHELL='probe-value'`.
        $out = & mise env 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'MISE_E2E_DEFAULT_SHELL'
        $out | Should -Not -Match 'export '
        $out | Should -Match '\$\{Env:MISE_E2E_DEFAULT_SHELL\}'
    }

    It 'still emits bash syntax when bash is named' {
        # The control. Without it the case above would not distinguish "the default changed" from
        # "mise stopped being able to emit bash at all".
        $out = & mise env -s bash 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'export MISE_E2E_DEFAULT_SHELL'
    }

    It 'is only the fallback: a detected shell still wins' {
        # The fallback must not override detection, or an activated session would get the wrong
        # syntax back.
        $env:MISE_SHELL = 'bash'
        try {
            $out = & mise env 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $out | Should -Match 'export MISE_E2E_DEFAULT_SHELL'
        } finally {
            Remove-Item -Path Env:\MISE_SHELL -ErrorAction SilentlyContinue
        }
    }
}
