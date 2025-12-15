
Describe 'prepare' {
    BeforeAll {
        $originalPath = Get-Location
        # Use physical path ($TestDrive) not PSDrive path (TestDrive:) - mise can't understand PSDrive paths
        Set-Location $TestDrive
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        # Also set experimental since prepare requires it
        $env:MISE_EXPERIMENTAL = "1"
    }

    AfterAll {
        Set-Location $originalPath
        Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_EXPERIMENTAL -ErrorAction SilentlyContinue
    }

    AfterEach {
        Remove-Item -Path (Join-Path $TestDrive 'package-lock.json') -ErrorAction SilentlyContinue
        Remove-Item -Path (Join-Path $TestDrive 'mise.toml') -ErrorAction SilentlyContinue
    }

    It 'lists no providers when no lockfiles exist' {
        mise prepare --list | Should -Match 'No prepare providers found'
    }

    It 'detects npm provider when configured with package-lock.json' {
        # Create files using full physical path to ensure they're in the right location
        $lockfile = Join-Path $TestDrive 'package-lock.json'
        $configfile = Join-Path $TestDrive 'mise.toml'

        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Out-File -FilePath $lockfile -Encoding utf8NoBOM

        @'
[prepare.npm]
'@ | Out-File -FilePath $configfile -Encoding utf8NoBOM

        # Verify files exist
        $lockfile | Should -Exist
        $configfile | Should -Exist

        mise prepare --list | Should -Match 'npm'
    }

    It 'prep alias works' {
        $lockfile = Join-Path $TestDrive 'package-lock.json'
        $configfile = Join-Path $TestDrive 'mise.toml'

        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Out-File -FilePath $lockfile -Encoding utf8NoBOM

        @'
[prepare.npm]
'@ | Out-File -FilePath $configfile -Encoding utf8NoBOM

        mise prep --list | Should -Match 'npm'
    }

    It 'dry-run shows what would run' {
        $lockfile = Join-Path $TestDrive 'package-lock.json'
        $configfile = Join-Path $TestDrive 'mise.toml'

        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Out-File -FilePath $lockfile -Encoding utf8NoBOM

        @'
[prepare.npm]
'@ | Out-File -FilePath $configfile -Encoding utf8NoBOM

        mise prepare --dry-run | Should -Match 'npm'
    }
}
