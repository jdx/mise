Describe 'command wrappers' {
    # `exe`- and `file`-mode shims do not run mise as the wrapped command. They run
    # `mise x -- <name>` with __MISE_SHIM_PATH naming themselves, which used to skip the
    # wrapper's own shim as the active one and run the real tool instead (#13671).

    BeforeAll {
        $originalLocation = Get-Location
        $originalPath = $env:PATH
        $originalDataDir = [Environment]::GetEnvironmentVariable('MISE_DATA_DIR', 'Process')
        $originalConfigFile = [Environment]::GetEnvironmentVariable('MISE_CONFIG_FILE', 'Process')
        $originalTrustedConfigPaths = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $originalShimMode = [Environment]::GetEnvironmentVariable('MISE_WINDOWS_SHIM_MODE', 'Process')

        Set-Location TestDrive:
        $env:MISE_DATA_DIR = Join-Path $TestDrive 'data'
        $env:MISE_CONFIG_FILE = Join-Path $TestDrive 'mise.toml'
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $wrapperDir = Join-Path $env:MISE_DATA_DIR 'command-wrappers\bin'

        $toolDir = Join-Path $TestDrive 'toolbin'
        New-Item -ItemType Directory -Path $toolDir -Force | Out-Null
        "@echo off`r`necho REAL_TOOL:%*`r`n" |
            Out-File -FilePath (Join-Path $toolDir 'wraptool.cmd') -Encoding ascii -NoNewline
        # The wrapper delegates to the real tool, as mbx does to cargo. Reaching it proves the
        # wrapper's PATH no longer leads back to the wrapper's own shim.
        "@echo off`r`necho WRAPPED:%WRAP_TEST%`r`nwraptool %*`r`n" |
            Out-File -FilePath (Join-Path $toolDir 'wrapcmd.cmd') -Encoding ascii -NoNewline
        $env:PATH = "$wrapperDir;$toolDir;$originalPath"

        @'
[wrappers.wraptool]
command = "wrapcmd"
args = ["--fixed"]
env = { WRAP_TEST = "1" }
'@ | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $originalLocation
        $env:PATH = $originalPath
        foreach ($saved in @(
            @{ Name = 'MISE_DATA_DIR'; Value = $originalDataDir },
            @{ Name = 'MISE_CONFIG_FILE'; Value = $originalConfigFile },
            @{ Name = 'MISE_TRUSTED_CONFIG_PATHS'; Value = $originalTrustedConfigPaths },
            @{ Name = 'MISE_WINDOWS_SHIM_MODE'; Value = $originalShimMode }
        )) {
            if ($null -eq $saved.Value) {
                Remove-Item -Path "Env:\$($saved.Name)" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($saved.Name, $saved.Value, 'Process')
            }
        }
        . "$PSScriptRoot\fixture-executables.ps1"
        Wait-MiseFixtureExecutables -Directory $TestDrive
    }

    It 'applies the wrapper through <mode>-mode shims' -ForEach @(
        @{ Mode = 'exe'; Shim = 'wraptool.exe' },
        @{ Mode = 'file'; Shim = 'wraptool.cmd' }
    ) {
        $env:MISE_WINDOWS_SHIM_MODE = $Mode
        mise reshim --force
        $LASTEXITCODE | Should -Be 0
        $shim = Join-Path $wrapperDir $Shim
        Test-Path $shim -PathType Leaf | Should -BeTrue

        $output = mise x -- wraptool check 2>&1
        $LASTEXITCODE | Should -Be 0 -Because "mise x output: $($output | Out-String)"
        $output | Should -Contain 'WRAPPED:1'
        $output | Should -Contain 'REAL_TOOL:--fixed check'

        # Windows command names ignore case, so another spelling reaches the wrapper too.
        $output = mise x -- WrapTool check 2>&1
        $LASTEXITCODE | Should -Be 0 -Because "mise x output: $($output | Out-String)"
        $output | Should -Contain 'WRAPPED:1'
        $output | Should -Contain 'REAL_TOOL:--fixed check'

        $output = & $shim check 2>&1
        $LASTEXITCODE | Should -Be 0 -Because "shim output: $($output | Out-String)"
        $output | Should -Contain 'WRAPPED:1'
        $output | Should -Contain 'REAL_TOOL:--fixed check'
    }

    It 'passes a wrapper shim another project left on to a task launcher' {
        # A native task launcher is a copy of mise-shim.exe, the file an exe-mode shim is. It runs
        # its task, so a leftover wrapper shim for that name must run it like any other command
        # and not look for a tool.
        $config = Get-Content $env:MISE_CONFIG_FILE -Raw
        try {
            $env:MISE_WINDOWS_SHIM_MODE = 'hardlink'
            $binDir = Join-Path $env:MISE_DATA_DIR 'bin'
            $hardlinkMise = Join-Path $binDir 'mise.exe'
            New-Item -ItemType Directory -Path $binDir -Force | Out-Null
            $misePath = (Get-Command -Type Application mise -All | Select-Object -First 1).Source
            Copy-Item $misePath $hardlinkMise

            # Another project's wrapper leaves a shim for `greet`.
            "[wrappers.greet]`ncommand = `"wrapcmd`"`n" |
                Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM
            & $hardlinkMise reshim --force
            $LASTEXITCODE | Should -Be 0
            $leftover = Join-Path $wrapperDir 'greet.exe'
            (Get-Item -Path $leftover).LinkType | Should -Be 'HardLink'

            # The config wraps another command again. A project has a task of that name, with its
            # launcher in a directory only mise adds to PATH. Stubs are for a project's own tasks,
            # so the task is not in the global config.
            $config | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM -NoNewline
            $project = Join-Path $TestDrive 'project'
            New-Item -ItemType Directory -Path $project -Force | Out-Null
            @'
[env]
_.path = "taskbin"

[tasks.greet]
run = "echo TASK_GREET"
'@ | Out-File -FilePath (Join-Path $project 'mise.toml') -Encoding utf8NoBOM
            Set-Location $project
            $output = mise generate task-stubs --dir taskbin --windows-launcher exe 2>&1
            $LASTEXITCODE | Should -Be 0 -Because "generate output: $($output | Out-String)"
            Test-Path 'taskbin\greet.exe' -PathType Leaf | Should -BeTrue
            Test-Path $leftover -PathType Leaf | Should -BeTrue

            $output = & $hardlinkMise x -- cmd /c greet 2>&1
            $LASTEXITCODE | Should -Be 0 -Because "mise x output: $($output | Out-String)"
            $output | Should -Contain 'TASK_GREET'
        } finally {
            Set-Location TestDrive:
            $config | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM -NoNewline
        }
    }
}
