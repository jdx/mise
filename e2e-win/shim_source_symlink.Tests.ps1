Describe 'exe shim source lookup through a symlinked mise' {
    # Single-link layout: the PATH-visible mise.exe is a symlink while mise-shim.exe
    # ships only beside the real binary. The lookup used to check only beside the
    # link and on PATH, so "exe" shim mode silently fell back to "file" mode.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalPath = $env:PATH
        $script:OriginalDataDir = [Environment]::GetEnvironmentVariable('MISE_DATA_DIR', 'Process')
        $script:OriginalConfigFile = [Environment]::GetEnvironmentVariable('MISE_CONFIG_FILE', 'Process')
        $script:OriginalTrustedConfigPaths = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:OriginalShimMode = [Environment]::GetEnvironmentVariable('MISE_WINDOWS_SHIM_MODE', 'Process')

        # The single-link layout: real/bin/{mise.exe, mise-shim.exe} plus links/mise.exe -> real/bin/mise.exe
        $script:RealBin = Join-Path $TestDrive 'real\bin'
        $script:Links = Join-Path $TestDrive 'links'
        New-Item -ItemType Directory -Path $script:RealBin -Force | Out-Null
        New-Item -ItemType Directory -Path $script:Links -Force | Out-Null
        $miseOnPath = (Get-Command -Type Application mise -All | Select-Object -First 1).Source
        Copy-Item $miseOnPath (Join-Path $script:RealBin 'mise.exe')
        Copy-Item (Join-Path (Split-Path $miseOnPath) 'mise-shim.exe') (Join-Path $script:RealBin 'mise-shim.exe')
        $script:LinkedMise = Join-Path $script:Links 'mise.exe'
        # Hosts without Developer Mode / elevation cannot create file symlinks;
        # record that so the tests can skip instead of failing the whole suite.
        $script:SymlinkUnavailable = $false
        try {
            New-Item -ItemType SymbolicLink -Path $script:LinkedMise -Target (Join-Path $script:RealBin 'mise.exe') -ErrorAction Stop | Out-Null
        } catch {
            $script:SymlinkUnavailable = $true
        }

        # Strip PATH of any directory that can reach a mise-shim.exe and prepend the
        # links directory: the linked mise is now the only mise on PATH.
        $stripped = ($env:PATH -split ';' | Where-Object {
            $_ -and -not (Test-Path -LiteralPath (Join-Path $_ 'mise-shim.exe'))
        }) -join ';'
        $env:PATH = "$($script:Links);$stripped"

        # Isolated data dir and config; the lazy bin stages a jq shim without installing it.
        $env:MISE_DATA_DIR = Join-Path $TestDrive 'data'
        $env:MISE_CONFIG_FILE = Join-Path $TestDrive 'mise.toml'
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        Set-Location $TestDrive
        @'
[tools]
jq = { version = "1.7.1", lazy = true, lazy_bins = ["jq.exe"] }
'@ | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM

        $env:MISE_WINDOWS_SHIM_MODE = 'exe'
        if (-not $script:SymlinkUnavailable) {
            & $script:LinkedMise reshim --force
            $script:ReshimExit = $LASTEXITCODE
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        $env:PATH = $script:OriginalPath
        foreach ($saved in @(
            @{ Name = 'MISE_DATA_DIR'; Value = $script:OriginalDataDir },
            @{ Name = 'MISE_CONFIG_FILE'; Value = $script:OriginalConfigFile },
            @{ Name = 'MISE_TRUSTED_CONFIG_PATHS'; Value = $script:OriginalTrustedConfigPaths },
            @{ Name = 'MISE_WINDOWS_SHIM_MODE'; Value = $script:OriginalShimMode }
        )) {
            if ($null -eq $saved.Value) {
                Remove-Item -Path "Env:\$($saved.Name)" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($saved.Name, $saved.Value, 'Process')
            }
        }
    }

    It 'reshim succeeds with the symlinked mise as the only mise on PATH' {
        if ($script:SymlinkUnavailable) {
            Set-ItResult -Skipped -Because 'this host cannot create file symlinks (enable Developer Mode or run elevated)'
            return
        }
        $script:ReshimExit | Should -Be 0
    }

    It 'writes native exe shims from the mise-shim.exe beside the real binary' {
        if ($script:SymlinkUnavailable) {
            Set-ItResult -Skipped -Because 'this host cannot create file symlinks (enable Developer Mode or run elevated)'
            return
        }
        # Before the fix this was jq.cmd + an extension-less script (the "file" fallback).
        $shim = Join-Path $env:MISE_DATA_DIR 'shims\jq.exe'
        Test-Path $shim -PathType Leaf | Should -BeTrue
        (Get-FileHash $shim).Hash | Should -Be (Get-FileHash (Join-Path $script:RealBin 'mise-shim.exe')).Hash
    }

    It 'does not leave file-mode shims behind' {
        if ($script:SymlinkUnavailable) {
            Set-ItResult -Skipped -Because 'this host cannot create file symlinks (enable Developer Mode or run elevated)'
            return
        }
        Test-Path (Join-Path $env:MISE_DATA_DIR 'shims\jq.cmd') | Should -BeFalse
        Test-Path (Join-Path $env:MISE_DATA_DIR 'shims\jq') | Should -BeFalse
    }
}
