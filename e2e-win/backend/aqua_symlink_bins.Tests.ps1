Describe 'aqua symlink_bins' {
    BeforeAll {
        $originalLocation = Get-Location
        $originalPath = $env:PATH
        $originalDataDir = [Environment]::GetEnvironmentVariable('MISE_DATA_DIR', 'Process')
        $originalConfigFile = [Environment]::GetEnvironmentVariable('MISE_CONFIG_FILE', 'Process')
        $originalTrustedConfigPaths = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        Set-Location TestDrive:
        $env:MISE_DATA_DIR = Join-Path $TestDrive 'data'
        $env:MISE_CONFIG_FILE = Join-Path $TestDrive 'mise.toml'
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $shimDir = Join-Path $env:MISE_DATA_DIR 'shims'
        $env:PATH = "$shimDir;$originalPath"

        # uv's Windows override lists its files by name only (no `src`).
        @'
[tools]
"aqua:astral-sh/uv" = { version = "0.8.22", symlink_bins = true }
'@ | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $originalLocation
        $env:PATH = $originalPath
        foreach ($saved in @(
            @{ Name = 'MISE_DATA_DIR'; Value = $originalDataDir },
            @{ Name = 'MISE_CONFIG_FILE'; Value = $originalConfigFile },
            @{ Name = 'MISE_TRUSTED_CONFIG_PATHS'; Value = $originalTrustedConfigPaths }
        )) {
            if ($null -eq $saved.Value) {
                Remove-Item -Path "Env:\$($saved.Name)" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($saved.Name, $saved.Value, 'Process')
            }
        }
    }

    It 'links name-only files into .mise-bins and shims them' {
        mise install
        $LASTEXITCODE | Should -Be 0
        mise reshim --force
        $LASTEXITCODE | Should -Be 0

        $binDir = Join-Path $env:MISE_DATA_DIR 'installs\aqua-astral-sh-uv\0.8.22\.mise-bins'
        foreach ($bin in @('uv', 'uvx')) {
            Test-Path (Join-Path $binDir "$bin.exe") -PathType Leaf | Should -BeTrue
            (mise which $bin) -replace '/', '\' | Should -Be (Join-Path $binDir "$bin.exe")

            $shim = Join-Path $env:MISE_DATA_DIR "shims\$bin.exe"
            Test-Path $shim -PathType Leaf | Should -BeTrue
            $output = & $shim --version 2>&1
            $LASTEXITCODE | Should -Be 0 -Because "shim output: $($output | Out-String)"
            ($output | Out-String) | Should -Match '0\.8\.22'
        }
    }
}
