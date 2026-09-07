Describe 'history watch' {
    BeforeAll {
        $script:OriginalExperimental = [Environment]::GetEnvironmentVariable('MISE_EXPERIMENTAL', 'Process')
        $env:MISE_EXPERIMENTAL = '0'
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:

        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $script:OriginalConfigDir = [Environment]::GetEnvironmentVariable('MISE_CONFIG_DIR', 'Process')
        $script:OriginalStateDir = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')
        $env:MISE_CONFIG_DIR = Join-Path $TestDrive 'config'
        $env:MISE_STATE_DIR = Join-Path $TestDrive 'state'
        New-Item -ItemType Directory -Force -Path $env:MISE_CONFIG_DIR | Out-Null
        $script:Tracked = Join-Path $TestDrive 'tracked'
        New-Item -ItemType Directory -Force -Path $script:Tracked | Out-Null
        'one' | Out-File -FilePath (Join-Path $script:Tracked 'file.txt') -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($pair in @(
                @('MISE_EXPERIMENTAL', $script:OriginalExperimental),
                @('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted),
                @('MISE_CONFIG_DIR', $script:OriginalConfigDir),
                @('MISE_STATE_DIR', $script:OriginalStateDir))) {
            if ($null -eq $pair[1]) {
                Remove-Item ("Env:" + $pair[0]) -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
            }
        }
    }

    It 'tracks without experimental opt-in' {
        $env:MISE_EXPERIMENTAL = '0'
        try {
            $output = mise bootstrap dotfiles track $script:Tracked 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $output | Should -Not -Match 'dotfile tracking is experimental'
        } finally {
            $env:MISE_EXPERIMENTAL = '0'
        }
    }

    It 'reconciles once and reports capture health' {
        $tracked = $script:Tracked -replace '\\', '/'
        mise bootstrap dotfiles track $tracked 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        $status = mise bootstrap dotfiles status --json | Out-String | ConvertFrom-Json
        $status.history.watcher | Should -Be 'not-declared'

        'two' | Out-File -FilePath (Join-Path $script:Tracked 'file.txt') -Encoding utf8NoBOM
        mise bootstrap dotfiles watch --once 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $entries = mise bootstrap dotfiles history --json | Out-String | ConvertFrom-Json
        $entries.Count | Should -Be 2
        $entries[0].trigger | Should -Be 'edit'

        @"
[bootstrap.services.mise-history]
builtin = "history-watch"
"@ | Out-File -FilePath (Join-Path $env:MISE_CONFIG_DIR 'config.toml') -Encoding utf8NoBOM
        $status = mise bootstrap dotfiles status --json | Out-String | ConvertFrom-Json
        $status.history.watcher | Should -Be 'declared-not-running'
    }
}
