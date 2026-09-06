Describe 'MISE_TRUSTED_CONFIG_PATHS' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it. The tests below set this var and
        # AfterEach clears it between them.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Create two separate project directories with mise.toml files
        $script:DirA = Join-Path $script:TestRoot "project_a"
        $script:DirB = Join-Path $script:TestRoot "project_b"
        New-Item -ItemType Directory -Path $script:DirA | Out-Null
        New-Item -ItemType Directory -Path $script:DirB | Out-Null

        @"
[env]
PROJECT = "a"
"@ | Out-File (Join-Path $script:DirA ".mise.toml")

        @"
[env]
PROJECT = "b"
"@ | Out-File (Join-Path $script:DirB ".mise.toml")
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

    # `-s bash` throughout: these assert *whether the config was trusted*, so they should not also
    # be asserting which shell `mise env` picks when nothing is named. Left bare, they were pinning
    # the Windows default as bash and broke when it changed.
    It 'trusts a single path set via env var' {
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:DirA
        Set-Location $script:DirA
        $output = mise env -s bash | Out-String
        $output | Should -Match "export PROJECT=a"
    }

    It 'trusts multiple Windows paths separated by semicolon' {
        # On Windows, paths are separated by ; (not :) because absolute paths
        # contain : in the drive letter (e.g. C:\foo). Using ; avoids ambiguity.
        $env:MISE_TRUSTED_CONFIG_PATHS = "$($script:DirA);$($script:DirB)"
        Set-Location $script:DirA
        $output = mise env -s bash | Out-String
        $output | Should -Match "export PROJECT=a"
        Set-Location $script:DirB
        $output = mise env -s bash | Out-String
        $output | Should -Match "export PROJECT=b"
    }
}
