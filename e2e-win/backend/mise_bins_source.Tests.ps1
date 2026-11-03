Describe '.mise-bins source' {
    # On Windows a .mise-bins entry is a copy. A program that finds its DLLs or data through its
    # own path would look in .mise-bins, so mise x and shims run the file it was copied from.
    # Deno.execPath() reports the path Windows started deno from.
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

        @'
[tools]
"github:denoland/deno" = { version = "2.5.6", asset_pattern = "deno-x86_64-pc-windows-msvc.zip", filter_bins = "deno.exe" }
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

    It 'runs the installed file, not its .mise-bins copy' {
        mise install
        $LASTEXITCODE | Should -Be 0
        mise reshim --force
        $LASTEXITCODE | Should -Be 0

        $installDir = (mise where github:denoland/deno | Select-Object -Last 1).Trim() -replace '/', '\'
        $entry = Join-Path $installDir '.mise-bins\deno.exe'
        Test-Path $entry -PathType Leaf | Should -BeTrue
        (mise which deno) -replace '/', '\' | Should -Be $entry

        $code = 'console.log(Deno.execPath())'
        $viaExec = (mise x -- deno eval $code | Select-Object -Last 1).Trim()
        $LASTEXITCODE | Should -Be 0
        $viaShim = (& (Join-Path $env:MISE_DATA_DIR 'shims\deno.exe') eval $code | Select-Object -Last 1).Trim()
        $LASTEXITCODE | Should -Be 0
        foreach ($path in @($viaExec, $viaShim)) {
            $path | Should -Not -Match '\\\.mise-bins\\'
            Split-Path $path -Leaf | Should -Be 'deno.exe'
            $path.StartsWith($installDir, [System.StringComparison]::OrdinalIgnoreCase) | Should -BeTrue
        }
    }
}
