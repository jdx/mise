Describe 'backend_http_raw_bin_path' {
    # A raw binary opting into shared extraction with `bin_path` is the shape
    # that `create_install_symlink` links file-to-file. On Windows that used to
    # go through `junction::create`, which builds a *directory* reparse point:
    # it succeeds, and leaves a link that cannot be resolved.
    #
    # Exercise both the default independent files and opt-in sharing with a
    # nested bin_path, including command resolution and execution in each mode.
    #
    # `bin` is set so the installed filename is decided outright rather than by
    # the name-cleaning heuristics, which keeps a failure here pointing at the
    # link rather than at what the file ended up called.
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

    It 'installs and runs a raw binary using <Mode> extraction' -TestCases @(
        @{ Mode = 'independent'; SharingOption = '' },
        @{ Mode = 'shared'; SharingOption = ', shared_extraction = true' }
    ) {
        param($Mode, $SharingOption)
        $tool = "http:docker-compose-binpath-$Mode"
        @"
[tools]
"$tool" = { version = "2.29.1", url = "https://github.com/docker/compose/releases/download/v{version}/docker-compose-windows-x86_64.exe", bin = "docker-compose.exe", bin_path = "nested/bin"$SharingOption }
"@ | Set-Content -Path (Join-Path $script:TestRoot "mise.toml")

        mise install -f $tool
        $LASTEXITCODE | Should -Be 0

        $install = mise where "${tool}@2.29.1"
        $LASTEXITCODE | Should -Be 0
        $binary = Join-Path $install 'nested\bin\docker-compose.exe'
        $item = Get-Item $binary
        $item.PSIsContainer | Should -BeFalse
        ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) | Should -Be 0

        # Docker ships on the CI images. Confirm the executable resolves from
        # this installation before checking that it actually runs.
        $resolved = (mise exec $tool -- where.exe docker-compose | Select-Object -First 1)
        $LASTEXITCODE | Should -Be 0
        $resolved | Should -Be $binary
        mise exec $tool -- docker-compose version | Should -BeLike "Docker Compose version *"
        $LASTEXITCODE | Should -Be 0
    }
}
