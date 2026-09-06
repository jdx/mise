Describe 'the install marker file names itself when it cannot be written' {
    # `create_install_dirs` creates three directories and then writes
    # `<CACHE>/<short>/<version>/incomplete` underneath. `std::fs::create_dir_all` answers Ok for a
    # path Windows will not take -- one ending in `nul` -- and creates nothing, so the first thing
    # to notice is that write, three calls away from the cause. Through `File::create` it was a bare
    # `(os error 3)` naming neither the file nor the operation:
    #
    #     mise ERROR Failed to install aqua:jqlang/jq@nul: The system cannot find the path
    #                specified. (os error 3)
    #
    # `file::create` wraps it with the path. This does not make `nul` a usable version, and
    # `create_dir_all` still answers Ok for it; only the report changes.

    BeforeAll {
        $script:OriginalDir = Get-Location
        # All four, every time. Pester runs every suite in one process, so an inherited value has to
        # come back afterwards rather than simply being removed.
        $script:Saved = @{}
        foreach ($v in 'MISE_DATA_DIR', 'MISE_CONFIG_DIR', 'MISE_CACHE_DIR', 'MISE_TRUSTED_CONFIG_PATHS') {
            $script:Saved[$v] = [Environment]::GetEnvironmentVariable($v, 'Process')
        }

        # Not named for a device itself: `nul` as the probe root cannot be created, which would
        # break the isolation rather than the subject.
        $script:Root = Join-Path $TestDrive 'marker'
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

        # Both must fail; the question is what they say. The control is `1.2` rather than an
        # invented string: aqua's jq entry opens with `version_constraint: semver("<= 1.2")` and
        # `no_asset: true`, so a real semver comparison selects that override. An unparseable
        # version would reach the same message here, but by a route that depends on how a
        # non-semver string is compared, which is not what this control is about.
        $script:Device = mise install jq@nul 2>&1 | Out-String
        $script:DeviceExit = $LASTEXITCODE
        $script:Control = mise install jq@1.2 2>&1 | Out-String
        $script:ControlExit = $LASTEXITCODE
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($v in $script:Saved.Keys) {
            if ($null -ne $script:Saved[$v]) { Set-Item "Env:\$v" $script:Saved[$v] }
            else { Remove-Item "Env:\$v" -ErrorAction SilentlyContinue }
        }
    }

    It 'still fails, because the file genuinely cannot be written' {
        $script:DeviceExit | Should -Not -Be 0
    }

    It 'names the file and the operation instead of only an OS error number' {
        # All three are mise's own words. The OS error text is not asserted because it is
        # localised: the machine this was measured on returns the Japanese form, not "The system
        # cannot find the path specified".
        $script:Device | Should -Match 'failed create'
        $script:Device | Should -Match 'incomplete'
        $script:Device | Should -Match 'nul'
    }

    It 'leaves an ordinary missing version reporting what it always did' {
        # The control. A change that rewrapped every install failure would pass the assertions above
        # and fail here: `1.2` writes its marker file fine and is refused by the aqua registry
        # afterwards, which is a different answer arrived at by a different route.
        $script:ControlExit | Should -Not -Be 0
        $script:Control | Should -Match 'no asset released'
        $script:Control | Should -Not -Match 'failed create'
    }
}
