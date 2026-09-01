Describe 'a tool whose backends are all ruled out on this platform' {
    # `mise install libsql-server@latest` on Windows used to exit 0 and unpack a source checkout --
    # the release builds for linux and darwin only, and its `source.tar.gz` was the last asset left
    # above zero in scoring.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:Saved = @{}
        foreach ($v in 'MISE_DATA_DIR', 'MISE_CONFIG_DIR', 'MISE_CACHE_DIR', 'MISE_TRUSTED_CONFIG_PATHS') {
            $script:Saved[$v] = [Environment]::GetEnvironmentVariable($v, 'Process')
        }

        $script:Root = Join-Path $TestDrive 'nobackend'
        $cfg = Join-Path $script:Root 'cfg'
        $cache = Join-Path $script:Root 'cache'
        $script:Data = Join-Path $script:Root 'data'
        $proj = Join-Path $script:Root 'proj'
        New-Item -ItemType Directory -Path $cfg, $cache, $script:Data, $proj -Force | Out-Null

        $env:MISE_DATA_DIR = $script:Data
        $env:MISE_CONFIG_DIR = $cfg
        $env:MISE_CACHE_DIR = $cache
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:Root
        Set-Location $proj
        '' | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM

        $script:Out = mise install libsql-server@latest 2>&1 | Out-String
        $script:Exit = $LASTEXITCODE
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($v in $script:Saved.Keys) {
            if ($null -ne $script:Saved[$v]) { Set-Item "Env:\$v" $script:Saved[$v] }
            else { Remove-Item "Env:\$v" -ErrorAction SilentlyContinue }
        }
    }

    It 'refuses instead of reporting a successful install' {
        $script:Exit | Should -Not -Be 0
    }

    It 'says the tool is registered but has no backend usable here' {
        $script:Out | Should -Match 'none of its backends'
    }

    It 'leaves no source checkout behind' {
        # The assertion that would have caught the original defect on its own: whatever mise did or
        # did not report, a repository must not appear under installs/.
        $installed = Join-Path $script:Data 'installs\libsql-server'
        if (Test-Path $installed) {
            @(Get-ChildItem -Path $installed -Recurse -Filter 'Cargo.toml') | Should -HaveCount 0
        }
    }
}
