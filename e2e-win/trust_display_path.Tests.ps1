Describe 'mise trust path output' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $script:ConfigPath = Join-Path $script:TestRoot ".mise.toml"
        @"
[env]
PROJECT = "a"
"@ | Out-File $script:ConfigPath
        Set-Location $script:TestRoot

        # `-BeLike` would read `?` as a single-character wildcard, so the prefix is matched as a
        # literal instead.
        function Get-PrefixCount {
            param([string]$Text)
            ([regex]::Matches($Text, [regex]::Escape('\\?\'))).Count
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    AfterEach {
        Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
    }

    # `mise trust` canonicalizes before reporting, and on Windows that yields an extended-length
    # path. mise refuses `\\?\` as input elsewhere, so it must not hand one back either. Each test
    # names the config file so it does not depend on what an earlier one left trusted.
    It 'reports a trusted path without the extended-length prefix' {
        $output = mise trust $script:ConfigPath 2>&1 | Out-String
        Get-PrefixCount $output | Should -Be 0
        # and the path is still reported -- simplified, not dropped
        $output.Contains($script:TestRoot) | Should -BeTrue
    }

    It 'reports an untrusted path without the prefix' {
        mise trust $script:ConfigPath | Out-Null
        $output = mise trust --untrust $script:ConfigPath 2>&1 | Out-String
        Get-PrefixCount $output | Should -Be 0
        $output.Contains($script:TestRoot) | Should -BeTrue
    }

    It 'reports the still-trusted-via-settings warning without the prefix' {
        # This branch printed the path through `{:?}`, which doubled every backslash on top of the
        # prefix, so it also asserts no escaped-backslash run survives.
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $output = mise trust --untrust $script:ConfigPath 2>&1 | Out-String
        $output.Contains('is trusted via settings') | Should -BeTrue
        Get-PrefixCount $output | Should -Be 0
        $output.Contains('\\\\') | Should -BeFalse
    }
}
