Describe 'backend_raw_bin_exe_extension' {
    # `bin = "docker-compose"` names a raw `.exe` download without its extension,
    # the way a cross-platform tool stub does. The file used to be installed as
    # `docker-compose` with no extension, which Windows cannot execute.
    BeforeAll {
        # `run.ps1` does not change directory, so a config written to the working
        # directory would land on the repository's own tracked `mise.toml`.
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        Set-Location $script:TestRoot

        # Saved here, restored in AfterAll: Pester runs every suite in one
        # process, so removing an inherited value would leave the next without it.
        $script:OriginalExperimental = [Environment]::GetEnvironmentVariable('MISE_EXPERIMENTAL', 'Process')
        $env:MISE_EXPERIMENTAL = "1"
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalExperimental) {
            Remove-Item -Path Env:\MISE_EXPERIMENTAL -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_EXPERIMENTAL = $script:OriginalExperimental
        }
    }

    It 'keeps .exe for bin without an extension via the <Backend> backend' -TestCases @(
        @{ Backend = 'http'; Tool = 'http:docker-compose-bin-exe'; Options = 'url = "https://github.com/docker/compose/releases/download/v{version}/docker-compose-windows-x86_64.exe"' },
        @{ Backend = 'github'; Tool = 'github:docker/compose'; Options = 'asset_pattern = "docker-compose-windows-x86_64.exe"' }
    ) {
        param($Backend, $Tool, $Options)
        @"
[tools]
"$Tool" = { version = "2.29.1", bin = "docker-compose", $Options }
"@ | Set-Content -Path (Join-Path $script:TestRoot "mise.toml")

        mise install -f $Tool
        $LASTEXITCODE | Should -Be 0

        $install = mise where "${Tool}@2.29.1"
        $LASTEXITCODE | Should -Be 0
        Test-Path (Join-Path $install 'docker-compose.exe') | Should -BeTrue
        Test-Path (Join-Path $install 'docker-compose') | Should -BeFalse

        mise exec $Tool -- docker-compose version | Should -BeLike "Docker Compose version *"
        $LASTEXITCODE | Should -Be 0
    }
}
