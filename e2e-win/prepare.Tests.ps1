
Describe 'prepare' {
    BeforeAll {
        $originalPath = Get-Location
        Set-Location TestDrive:
        # Trust the TestDrive config path - use $TestDrive for physical path, not PSDrive path
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
        Remove-Item -Path 'package-lock.json' -ErrorAction SilentlyContinue
        Remove-Item -Path 'mise.toml' -ErrorAction SilentlyContinue
    }

    It 'lists no providers when no lockfiles exist' {
        mise prepare --list | Should -Match 'No prepare providers found'
    }

    It 'detects npm provider when configured with package-lock.json' {
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

        mise prepare --list | Should -Match 'npm'
    }

    It 'prep alias works' {
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
