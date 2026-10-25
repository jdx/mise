Describe 'task fail-fast with Git Bash' {
    BeforeAll {
        $script:originalPath = Get-Location
        $script:originalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        $script:testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:testDir | Out-Null
        Set-Location $script:testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:testDir

        # `sleep` after `;` keeps bash from running it in place: it forks, and Git Bash's emulated
        # fork leaves `sleep.exe` outside the parent chain `taskkill /T` follows.
        Set-Content -LiteralPath (Join-Path $script:testDir 'mise.toml') -Value @(
            '[tasks.fail]'
            'run = "exit 1"'
            ''
            '[tasks.slow]'
            'shell = "bash -c"'
            'run = "sleep 20; echo $((40 + 2))"'
        )
    }

    AfterAll {
        Set-Location $script:originalPath
        if ($null -eq $script:originalTrusted) {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        } else {
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:originalTrusted
        }
    }

    It 'stops a sibling task whose children Git Bash forked' {
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        $out = & mise run fail ::: slow 2>&1 | Out-String
        $watch.Stop()
        $LASTEXITCODE | Should -Not -Be 0
        # The command line is echoed, so only a computed value shows the task ran to the end.
        $out | Should -Not -Match '(?m)^42\s*$'
        $watch.Elapsed.TotalSeconds | Should -BeLessThan 15
    }
}
