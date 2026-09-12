Describe 'mise trust --ignore on Windows' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved and restored in AfterAll: Pester runs every suite in one process, so a state dir
        # left pointing into $TestDrive would follow the suites that run after this one.
        $script:OriginalState = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $script:StateDir = Join-Path $script:TestRoot 'state'
        $script:Store = Join-Path $script:StateDir 'ignored-configs'
        $script:Project = Join-Path $script:TestRoot 'project'
        New-Item -ItemType Directory -Path $script:Project | Out-Null
        @"
[env]
IGNORE_PROBE = "loaded"
"@ | Out-File (Join-Path $script:Project 'mise.toml')

        $env:MISE_STATE_DIR = $script:StateDir
        # `trusted_config_paths` overrides the persisted ignore list by design, so the setting CI
        # sets globally would mask exactly what this suite is testing.
        Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore

        function script:StoreCount {
            @(Get-ChildItem $script:Store -Force -ErrorAction SilentlyContinue).Count
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

    It 'keeps a config out of later processes' {
        Set-Location $script:TestRoot
        # Trusted first, so the control below holds off CI too: `is_trusted` short-circuits to true
        # under `ci_info::is_ci()`, but nothing grants trust on a developer's machine. The ignore
        # list is consulted *before* the persisted trust store, so trusting here does not weaken
        # what the last assertion tests.
        mise trust $script:Project | Out-Null
        $LASTEXITCODE | Should -Be 0

        Set-Location $script:Project
        # Control: the config loads before anything is ignored. Without it a fix that broke config
        # loading outright would pass the assertion at the end.
        $out = mise env | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'IGNORE_PROBE'

        # Named explicitly rather than relying on the no-argument form, which searches for the
        # first untrusted config from the cwd upward and finds different things in different
        # environments.
        Set-Location $script:TestRoot
        $out = mise trust --ignore $script:Project 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'ignored'

        # Control, asserted on its own so a setup failure cannot be read as the defect: the entry
        # really is written. On Windows it is a plain file holding the path rather than a symlink,
        # because symlinks need a privilege mise does not require -- and resolving it was the part
        # that was wrong.
        (script:StoreCount) | Should -Be 1

        # The point of the fix: a *new process* has to honour it. The ignore list is loaded once
        # per process, so this is the only place the loading path is exercised.
        Set-Location $script:Project
        $out = mise env | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Not -Match 'IGNORE_PROBE'
    }
}
