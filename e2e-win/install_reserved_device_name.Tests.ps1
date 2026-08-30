Describe 'an install directory Windows will not create' {
    # `std::fs::create_dir_all` answers Ok for a path ending in `nul` and creates nothing --
    # measured with a standalone rustc probe:
    #
    #     installs/jq/zzz   create_dir_all=Ok   after: is_dir()=true
    #     installs/jq/aux   create_dir_all=Ok   after: is_dir()=true
    #     installs/jq/nul   create_dir_all=Ok   after: is_dir()=false
    #
    # `create_install_dirs` made three such calls and then wrote a marker file underneath, so the
    # failure surfaced from `File::create` -- three calls away from the cause, with no path and no
    # operation, as a bare `(os error 3)`.
    #
    # This does not make `nul` a usable version. It makes the refusal say which path and why.

    BeforeAll {
        $script:OriginalDir = Get-Location
        # All four, every time. A probe that isolated only some of these once wrote junctions into
        # the real installs directory.
        $script:Saved = @{}
        foreach ($v in 'MISE_DATA_DIR', 'MISE_CONFIG_DIR', 'MISE_CACHE_DIR', 'MISE_TRUSTED_CONFIG_PATHS') {
            $script:Saved[$v] = [Environment]::GetEnvironmentVariable($v, 'Process')
        }

        # Not named for a device itself: an earlier attempt at this used `nul` for the probe root
        # and Windows refused to create it, which broke the isolation rather than the subject.
        $script:Root = Join-Path $TestDrive 'devname'
        $cfg = Join-Path $script:Root 'cfg'
        $cache = Join-Path $script:Root 'cache'
        $data = Join-Path $script:Root 'data'
        $proj = Join-Path $script:Root 'proj'
        New-Item -ItemType Directory -Path $cfg, $cache, $data, $proj -Force | Out-Null

        $env:MISE_DATA_DIR = $data
        $env:MISE_CONFIG_DIR = $cfg
        $env:MISE_CACHE_DIR = $cache
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:Root
        Set-Location $proj
        '' | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM

        # Neither version exists, so both must fail. The question is what they say.
        $script:Device = mise install jq@nul 2>&1 | Out-String
        $script:DeviceExit = $LASTEXITCODE
        $script:Control = mise install jq@zzz 2>&1 | Out-String
        $script:ControlExit = $LASTEXITCODE
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($v in $script:Saved.Keys) {
            if ($null -ne $script:Saved[$v]) { Set-Item "Env:\$v" $script:Saved[$v] }
            else { Remove-Item "Env:\$v" -ErrorAction SilentlyContinue }
        }
    }

    It 'still fails, because the directory genuinely cannot be created' {
        $script:DeviceExit | Should -Not -Be 0
    }

    It 'names the path it could not create instead of only an OS error number' {
        $script:Device | Should -Match 'created nothing'
        $script:Device | Should -Match 'installs'
    }

    It 'names the component Windows objects to' {
        # Without this the message says a directory is missing and leaves the reader to work out
        # which part of the path Windows refuses.
        $script:Device | Should -Match 'reserves for a device'
        $script:Device | Should -Match 'nul'
    }

    It 'leaves an ordinary missing version reporting what it always did' {
        # The control. A change that rewrote every install failure would pass the assertions above
        # and fail here: `zzz` creates its directories fine and is refused by the aqua registry,
        # which is a different answer arrived at by a different route.
        $script:ControlExit | Should -Not -Be 0
        $script:Control | Should -Match 'no asset released'
        $script:Control | Should -Not -Match 'created nothing'
    }
}
