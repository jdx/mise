Describe 'lazy shim' {
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
        $env:MISE_WINDOWS_SHIM_MODE = 'exe'
        $shimDir = Join-Path $env:MISE_DATA_DIR 'shims'
        $env:PATH = "$shimDir;$originalPath"

        @'
[tools]
jq = { version = "1.7.1", lazy = true, lazy_bins = ["JQ.EXE"] }
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

    It 'installs and runs a lazy tool through native and hardlink shims' {
        mise reshim --force
        $LASTEXITCODE | Should -Be 0
        $shim = Join-Path $env:MISE_DATA_DIR 'shims\jq.exe'
        Test-Path $shim -PathType Leaf | Should -BeTrue
        Test-Path (Join-Path $env:MISE_DATA_DIR 'installs\jq\1.7.1') | Should -BeFalse

        $output = & $shim --version 2>&1
        $LASTEXITCODE | Should -Be 0 -Because "native shim output: $($output | Out-String)"
        ($output | Out-String) | Should -Match 'jq-1\.7.1'
        Test-Path (Join-Path $env:MISE_DATA_DIR 'installs\jq\1.7.1') | Should -BeTrue

        mise uninstall --all jq
        $LASTEXITCODE | Should -Be 0
        $env:MISE_WINDOWS_SHIM_MODE = 'hardlink'
        $binDir = Join-Path $env:MISE_DATA_DIR 'bin'
        $hardlinkMise = Join-Path $binDir 'mise.exe'
        New-Item -ItemType Directory -Path $binDir -Force | Out-Null
        $misePath = (Get-Command -Type Application mise -All | Select-Object -First 1).Source
        Copy-Item $misePath $hardlinkMise
        & $hardlinkMise reshim --force
        $LASTEXITCODE | Should -Be 0
        (Get-Item -Path $shim).LinkType | Should -Be 'HardLink'
        Test-Path (Join-Path $env:MISE_DATA_DIR 'installs\jq\1.7.1') | Should -BeFalse

        $output = & $shim --version 2>&1
        $LASTEXITCODE | Should -Be 0 -Because "hardlink shim output: $($output | Out-String)"
        ($output | Out-String) | Should -Match 'jq-1\.7\.1'
        Test-Path (Join-Path $env:MISE_DATA_DIR 'installs\jq\1.7.1') | Should -BeTrue
    }
}
