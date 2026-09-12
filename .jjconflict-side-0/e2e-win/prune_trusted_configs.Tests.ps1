Describe 'mise prune --configs on Windows' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved and restored in AfterAll: Pester runs every suite in one process, so a state dir
        # left pointing into $TestDrive would follow the suites that run after this one.
        $script:OriginalState = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $script:StateDir = Join-Path $script:TestRoot 'state'
        $script:Store = Join-Path $script:StateDir 'trusted-configs'

        $env:MISE_STATE_DIR = $script:StateDir
        # Trust has to come from the store, not from the setting: CI sets the setting globally, and
        # a config already covered by it would leave `mise trust` with nothing to record.
        Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore

        # Counting a directory that does not exist yet is 0, not an error -- an entry never written
        # has to fail the assertion that wanted it, not blow up the line before.
        function script:StoreCount {
            @(Get-ChildItem $script:Store -Force -ErrorAction SilentlyContinue).Count
        }

        function script:NewProject([string]$name) {
            $p = Join-Path $script:TestRoot $name
            New-Item -ItemType Directory -Path $p -Force | Out-Null
            @"
[env]
$($name.ToUpper()) = "1"
"@ | Out-File (Join-Path $p 'mise.toml')
            $p
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalState) {
            Remove-Item Env:MISE_STATE_DIR -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_STATE_DIR', $script:OriginalState, 'Process')
        }
        if ($null -ne $script:OriginalTrusted) {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'removes a trusted config link whose project is gone' {
        $project = script:NewProject 'gone'
        $before = script:StoreCount

        # Named explicitly rather than relying on `mise trust` with no argument: that form searches
        # for the first *untrusted* config from the cwd upward, and what it finds depends on the
        # environment the suite happens to run in.
        Set-Location $script:TestRoot
        mise trust $project | Out-Null
        $LASTEXITCODE | Should -Be 0

        # Control, and its own assertion so a setup failure cannot be mistaken for the defect: the
        # entry really is written before anything is pruned. It is a plain file rather than a
        # symlink here -- Windows symlinks need a privilege mise does not require, so
        # `file::make_symlink_or_file` writes the target path into a file instead, and a file
        # holding a dead path still exists. That is exactly what prune used to ask about.
        (script:StoreCount) | Should -Be ($before + 1)

        Remove-Item -LiteralPath $project -Recurse -Force
        Test-Path -LiteralPath $project | Should -BeFalse

        mise prune --configs | Out-Null
        $LASTEXITCODE | Should -Be 0

        (script:StoreCount) | Should -Be $before
    }

    It 'keeps a trusted config link whose project is still there' {
        # The other half: prune must not empty the store just because the entries are files. A fix
        # that removed everything would pass the test above on its own.
        $project = script:NewProject 'kept'
        $before = script:StoreCount

        Set-Location $script:TestRoot
        mise trust $project | Out-Null
        $LASTEXITCODE | Should -Be 0
        (script:StoreCount) | Should -Be ($before + 1)

        mise prune --configs | Out-Null
        $LASTEXITCODE | Should -Be 0

        (script:StoreCount) | Should -Be ($before + 1)
    }
}
