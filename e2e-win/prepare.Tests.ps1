
Describe 'prepare' {
    BeforeAll {
        $originalPath = Get-Location
        Set-Location TestDrive:
    }

    AfterAll {
        Set-Location $originalPath
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
'@ | Set-Content -Path 'package-lock.json'

        @'
[prepare.npm]
'@ | Set-Content -Path 'mise.toml'

        mise prepare --list | Should -Match 'npm'
    }

    It 'prep alias works' {
        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Set-Content -Path 'package-lock.json'

        @'
[prepare.npm]
'@ | Set-Content -Path 'mise.toml'

        mise prep --list | Should -Match 'npm'
    }

    It 'dry-run shows what would run' {
        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Set-Content -Path 'package-lock.json'

        @'
[prepare.npm]
'@ | Set-Content -Path 'mise.toml'

        mise prepare --dry-run | Should -Match 'npm'
    }
}
