
Describe 'prepare' {

    It 'lists no providers when no lockfiles exist' {
        mise prepare --list | Should -Match 'No prepare providers found'
    }

    It 'detects npm provider with package-lock.json' {
        @'
{
  "name": "test-project",
  "lockfileVersion": 3,
  "packages": {}
}
'@ | Set-Content -Path 'package-lock.json'

        mise prepare --list | Should -Match 'npm'
    }

    It 'prep alias works' {
        mise prep --list | Should -Match 'npm'
    }

    It 'dry-run shows what would run' {
        mise prepare --dry-run | Should -Match 'npm'
    }

    AfterAll {
        Remove-Item -Path 'package-lock.json' -ErrorAction SilentlyContinue
    }
}
