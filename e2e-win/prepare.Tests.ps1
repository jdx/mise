
Describe 'prepare' {
    BeforeAll {
        $script:originalPath = Get-Location
        # Set experimental since prepare requires it
        $env:MISE_EXPERIMENTAL = "1"
    }

    AfterAll {
        Set-Location $script:originalPath
        Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        # Don't remove MISE_EXPERIMENTAL - other tests (like vfox) need it for custom backends
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

    It 'detects npm provider when configured with package-lock.json' {
        # Create unique test directory to avoid config inheritance from repo root
        $testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $testDir | Out-Null
        Set-Location $testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $testDir

        try {
            # Create test files
            '{"name": "test-project", "lockfileVersion": 3, "packages": {}}' | Set-Content -Path 'package-lock.json'
            '[prepare.npm]' | Set-Content -Path 'mise.toml'

            # Verify files exist
            'package-lock.json' | Should -Exist
            'mise.toml' | Should -Exist

            mise prepare --list | Should -Match 'npm'
        } finally {
            Set-Location $script:originalPath
            Remove-Item -Path $testDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'prep alias works' {
        # Create unique test directory to avoid config inheritance from repo root
        $testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $testDir | Out-Null
        Set-Location $testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $testDir

        try {
            # Create test files
            '{"name": "test-project", "lockfileVersion": 3, "packages": {}}' | Set-Content -Path 'package-lock.json'
            '[prepare.npm]' | Set-Content -Path 'mise.toml'

            mise prep --list | Should -Match 'npm'
        } finally {
            Set-Location $script:originalPath
            Remove-Item -Path $testDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'dry-run shows what would run' {
        # Create unique test directory to avoid config inheritance from repo root
        $testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $testDir | Out-Null
        Set-Location $testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $testDir

        try {
            # Create test files
            '{"name": "test-project", "lockfileVersion": 3, "packages": {}}' | Set-Content -Path 'package-lock.json'
            '[prepare.npm]' | Set-Content -Path 'mise.toml'

            mise prepare --dry-run | Should -Match 'npm'
        } finally {
            Set-Location $script:originalPath
            Remove-Item -Path $testDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
