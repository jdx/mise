Describe 'backend_http_raw_bin_path' {
    # A raw binary declared with `bin_path` is the one shape of `http:` install
    # that `create_install_symlink` links file-to-file. On Windows that used to
    # go through `junction::create`, which builds a *directory* reparse point:
    # it succeeds, and leaves a link that cannot be resolved.
    #
    # `http_binary_clean.Tests.ps1` covers the same binary without `bin_path`,
    # which takes the other branch and links the install directory instead - so
    # the broken combination was the one thing untested here.
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

        @"
[tools]
"http:docker-compose-binpath" = { version = "2.29.1", url = "https://github.com/docker/compose/releases/download/v{version}/docker-compose-windows-x86_64.exe", bin = "docker-compose.exe", bin_path = "bin" }
"@ | Set-Content -Path (Join-Path $script:TestRoot "mise.toml")
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

    It 'reports the install as successful' {
        # Asserted on purpose: the defect said nothing at install time. Without
        # this line a reader could assume the install used to fail loudly.
        mise install -f http:docker-compose-binpath
        $LASTEXITCODE | Should -Be 0
    }

    It 'resolves the binary from the tool it installed' {
        # Checking the version alone is not enough. Docker ships on the GitHub
        # Windows images, so an unrelated `docker-compose.exe` further along PATH
        # could satisfy a version check while the installed link stayed broken -
        # the test would pass on unfixed code. Assert where the command actually
        # resolves from first.
        $resolved = (mise exec http:docker-compose-binpath -- where.exe docker-compose | Select-Object -First 1)
        $resolved | Should -BeLike "*http-docker-compose-binpath*"
        $resolved | Should -BeLike "*bin*docker-compose.exe"
    }

    It 'installs a binary that actually runs' {
        mise exec http:docker-compose-binpath -- docker-compose version | Should -BeLike "Docker Compose version *"
    }
}
