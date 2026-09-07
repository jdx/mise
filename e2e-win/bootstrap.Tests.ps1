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

    It 'rejects services selected through bootstrap config roots' {
        $serviceRoot = Join-Path $TestDrive 'service-root'
        New-Item -ItemType Directory -Path $serviceRoot | Out-Null
        @"
[bootstrap.services.example]
state = "stopped"
enabled = false
"@ | Out-File -FilePath (Join-Path $serviceRoot 'mise.toml') -Encoding utf8NoBOM

        @"
[bootstrap]
config_roots = ["service-root"]
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $out = mise bootstrap services apply --yes 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*bootstrap system services are only supported on Linux*'
    }

    It 'reports an installed WinGet package' -Skip:(-not (Get-Command winget -ErrorAction Ignore)) {
        @"
[bootstrap.packages]
"winget:Microsoft.AppInstaller" = "latest"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $out = mise bootstrap packages status 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*winget*Microsoft.AppInstaller*installed*'
    }

    It 'dry-runs WinGet source refresh and an exact package install' -Skip:(-not (Get-Command winget -ErrorAction Ignore)) {
        @"
[bootstrap.packages]
"winget:Mise.Does.Not.Exist.0123456789" = "latest"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $out = mise bootstrap packages apply --manager winget --update --dry-run --yes 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*winget source update --accept-source-agreements --disable-interactivity*'
        $out | Should -BeLike '*winget install --id Mise.Does.Not.Exist.0123456789 --exact --silent --accept-source-agreements --accept-package-agreements --disable-interactivity*'
    }

    It 'passes WinGet version pins without interpreting them' -Skip:(-not (Get-Command winget -ErrorAction Ignore)) {
        @"
[bootstrap.packages]
"winget:Microsoft.AppInstaller" = "0.0-preview"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $out = mise bootstrap packages apply --manager winget --dry-run --yes 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*winget install --id Microsoft.AppInstaller --exact --version 0.0-preview --silent*'
    }
}
