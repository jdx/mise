
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
        # Clean up files in current directory
        Remove-Item -Path 'package-lock.json' -ErrorAction SilentlyContinue
        Remove-Item -Path 'mise.toml' -ErrorAction SilentlyContinue
    }

    It 'lists no providers when no lockfiles exist' {
        mise prepare --list | Should -Match 'No prepare providers found'
    }

    It 'detects npm provider when configured with package-lock.json' {
        # Create files in current directory (we're in $TestDrive from BeforeAll)
        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Out-File -FilePath 'package-lock.json' -Encoding utf8NoBOM

        @'
[prepare.npm]
'@ | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM

        # Verify files exist in current directory
        'package-lock.json' | Should -Exist
        'mise.toml' | Should -Exist

        mise prepare --list | Should -Match 'npm'
    }

    It 'prep alias works' {
        # Create files in current directory (we're in $TestDrive from BeforeAll)
        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Out-File -FilePath 'package-lock.json' -Encoding utf8NoBOM

        @'
[prepare.npm]
'@ | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM

        mise prep --list | Should -Match 'npm'
    }

    It 'dry-run shows what would run' {
        # Create files in current directory (we're in $TestDrive from BeforeAll)
        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Out-File -FilePath 'package-lock.json' -Encoding utf8NoBOM

        @'
[prepare.npm]
'@ | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM

        mise prepare --dry-run | Should -Match 'npm'
    }
}
