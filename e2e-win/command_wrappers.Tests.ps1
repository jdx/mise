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

        $output = & $shim check 2>&1
        $LASTEXITCODE | Should -Be 0 -Because "shim output: $($output | Out-String)"
        $output | Should -Contain 'WRAPPED:1'
        $output | Should -Contain 'REAL_TOOL:--fixed check'
    }
}
