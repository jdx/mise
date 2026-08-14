Describe 'bootstrap' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:

        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        $script:RepoTarget = (Join-Path $TestDrive 'repo') -replace '\\', '/'
        $script:FileTarget = (Join-Path $TestDrive 'managed') -replace '\\', '/'
    }

    BeforeEach {
        @"
[bootstrap.repos]
"$script:RepoTarget" = { url = "https://example.invalid/repo.git" }
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'allows aggregate commands when no system files are configured' {
        mise bootstrap --dry-run 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        mise bootstrap status 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        mise bootstrap plan 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
    }

    It 'still rejects configured system files' {
        @"

[bootstrap.files."$script:FileTarget"]
content = "managed"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM -Append

        $out = mise bootstrap plan 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*managed system files are only supported on Unix*'
    }
}
