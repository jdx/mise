Describe 'mise settings set with a path list' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $script:ConfigPath = Join-Path $script:TestRoot "mise.toml"

        # Counting `C:\` distinguishes the two behaviours without depending on how toml_edit
        # quotes a string: the colon split severed every drive letter from its path, so the
        # broken output held a bare `C` entry and no `C:\` at all.
        function Get-DriveRootedCount {
            $written = Get-Content $script:ConfigPath | Out-String
            ([regex]::Matches($written, [regex]::Escape('C:\'))).Count
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    BeforeEach {
        # `task.disable_paths` rather than `trusted_config_paths`, which is global_only: --local
        # would not apply to it and the write would land in the real user config.
        Set-Location $script:TestRoot
        "[tools]" | Out-File -Encoding ascii $script:ConfigPath
    }

    It 'keeps a lone path whole' {
        # No list separator anywhere, and it still came apart before: the drive letter's colon
        # is not a separator.
        mise settings set --local task.disable_paths 'C:\one'
        Get-DriveRootedCount | Should -Be 1
        (mise settings get task.disable_paths | Out-String) | Should -Match ([regex]::Escape('C:\one'))
    }

    It 'splits on the semicolon rather than the drive letter' {
        mise settings set --local task.disable_paths 'C:\one;C:\two'
        Get-DriveRootedCount | Should -Be 2
        $got = mise settings get task.disable_paths | Out-String
        $got | Should -Match ([regex]::Escape('C:\one'))
        $got | Should -Match ([regex]::Escape('C:\two'))
    }

    It 'appends without severing the path when adding' {
        # `mise settings add` reaches the same parser, so it carried the same defect.
        mise settings set --local task.disable_paths 'C:\one'
        mise settings add --local task.disable_paths 'C:\two'
        Get-DriveRootedCount | Should -Be 2
        $got = mise settings get task.disable_paths | Out-String
        $got | Should -Match ([regex]::Escape('C:\one'))
        $got | Should -Match ([regex]::Escape('C:\two'))
    }
}
