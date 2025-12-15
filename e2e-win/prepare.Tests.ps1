
Describe 'prepare' {
    BeforeAll {
        $script:originalPath = Get-Location
        # Set experimental since prepare requires it
        $env:MISE_EXPERIMENTAL = "1"
    }

    AfterAll {
        Set-Location $script:originalPath
        Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_EXPERIMENTAL -ErrorAction SilentlyContinue
    }

    It 'lists no providers when no lockfiles exist' {
        # Create unique test directory to avoid config inheritance from repo root
        $testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $testDir | Out-Null
        Set-Location $testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $testDir

        try {
            mise prepare --list | Should -Match 'No prepare providers found'
        } finally {
            Set-Location $script:originalPath
            Remove-Item -Path $testDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    # Note: Provider detection tests are skipped on Windows due to config discovery
    # complexities. The prepare functionality is fully tested on Linux e2e tests.
    # See e2e/cli/test_prepare for comprehensive coverage.
}
