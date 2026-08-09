Describe 'exec_inactive_tool' {
    # Regression test for discussion #4407: `mise install <tool>` writes to no
    # config file, so the tool's bin dir never joins the PATH `mise exec` builds
    # and the bin fails to resolve. The Windows arm surfaced only the opaque
    # `cannot find binary path`, with no hint that mise had just installed it.

    BeforeAll {
        $script:miseExe = (Get-Command mise).Source

        # A scratch cwd so no repo-local mise.toml can activate the tool and
        # make the "not activated" path unreachable.
        $script:workdir = Join-Path -Path $env:TEMP -ChildPath "mise-4407-$PID"
        New-Item -ItemType Directory -Force -Path $script:workdir | Out-Null
        Push-Location $script:workdir

        mise install usage | Out-Null

        # Drop any host `usage` from PATH. The subject here is what mise says
        # when the bin is genuinely unreachable; a preinstalled copy would
        # resolve instead and mask the failure entirely.
        $script:savedPath = $env:PATH
        $env:PATH = ($env:PATH -split ';' | Where-Object {
                $_ -and -not (Test-Path -Path (Join-Path -Path $_ -ChildPath 'usage.exe') -ErrorAction SilentlyContinue)
            }) -join ';'
    }

    AfterAll {
        $env:PATH = $script:savedPath
        Pop-Location
        Remove-Item -Recurse -Force $script:workdir -ErrorAction Ignore
    }

    It 'names the installed-but-inactive tool' {
        $out = (& $script:miseExe exec -- usage 2>&1 | Out-String)
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -Match "installed but not activated"
        $out | Should -Match "mise use usage"
        $out | Should -Match "mise exec usage -- usage"
    }

    It 'keeps the original error for a bin no installed tool provides' {
        $out = (& $script:miseExe exec -- not-a-tool-4407 2>&1 | Out-String)
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -Match "cannot find binary path"
        $out | Should -Not -Match "installed but not activated"
    }

    It 'suggests a command that works' {
        $out = (& $script:miseExe exec usage -- usage --version 2>&1 | Out-String)
        $out | Should -Match "usage-cli"
    }
}
