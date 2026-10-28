Describe 'system lazy fallback' {
    BeforeAll {
        $originalLocation = Get-Location
        $savedEnv = @{}
        foreach ($name in @('PATH', 'MISE_CONFIG_FILE', 'MISE_CONFIG_DIR', 'MISE_DATA_DIR',
                'MISE_CACHE_DIR', 'MISE_STATE_DIR', 'MISE_SYSTEM_CONFIG_FILE',
                'MISE_SYSTEM_CONFIG_DIR', 'MISE_SYSTEM_DATA_DIR', 'MISE_SYSTEM_INSTALLS_DIR',
                'MISE_SHIMS_DIR', 'MISE_SYSTEM_SHIMS_DIR', 'MISE_TRUSTED_CONFIG_PATHS',
                'MISE_SYSTEM_LAZY_FALLBACK', 'MISE_NOT_FOUND_AUTO_INSTALL',
                'MISE_NOT_FOUND_SYSTEM_FALLBACK', 'MISE_WINDOWS_SHIM_MODE',
                'MISE_WINDOWS_EXECUTABLE_EXTENSIONS')) {
            $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        Set-Location TestDrive:
        $env:MISE_CONFIG_DIR = Join-Path $TestDrive 'config'
        $env:MISE_CONFIG_FILE = Join-Path $env:MISE_CONFIG_DIR 'config.toml'
        $env:MISE_DATA_DIR = Join-Path $TestDrive 'data'
        $env:MISE_CACHE_DIR = Join-Path $TestDrive 'cache'
        $env:MISE_STATE_DIR = Join-Path $TestDrive 'state'
        $env:MISE_SYSTEM_CONFIG_DIR = Join-Path $TestDrive 'system'
        $env:MISE_SYSTEM_CONFIG_FILE = Join-Path $env:MISE_SYSTEM_CONFIG_DIR 'config.toml'
        $env:MISE_SYSTEM_DATA_DIR = Join-Path $TestDrive 'system-data'
        $env:MISE_SYSTEM_INSTALLS_DIR = Join-Path $env:MISE_DATA_DIR 'installs'
        $env:MISE_SHIMS_DIR = Join-Path $TestDrive 'shims'
        $env:MISE_SYSTEM_SHIMS_DIR = Join-Path $TestDrive 'system-shims'
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $env:MISE_SYSTEM_LAZY_FALLBACK = '1'
        $env:MISE_NOT_FOUND_AUTO_INSTALL = '0'
        $env:MISE_NOT_FOUND_SYSTEM_FALLBACK = '0'
        $env:MISE_WINDOWS_SHIM_MODE = 'exe'
        $env:MISE_WINDOWS_EXECUTABLE_EXTENSIONS = 'ps1,vbs,exe,com,bat,cmd'
        $early = Join-Path $TestDrive 'early'
        $later = Join-Path $TestDrive 'later'
        New-Item -ItemType Directory -Path $env:MISE_CONFIG_DIR, $env:MISE_SYSTEM_CONFIG_DIR, $early, $later -Force | Out-Null
        '' | Set-Content $env:MISE_CONFIG_FILE
        @'
[tools]
jq = { version = "1.7.1", lazy = true, lazy_bins = ["review-lazy"] }
'@ | Set-Content $env:MISE_SYSTEM_CONFIG_FILE
        @'
@echo off
echo spawnable-fallback:%~1
exit /b 23
'@ | Set-Content (Join-Path $later 'review-lazy.cmd')
        $env:PATH = "$env:MISE_SYSTEM_SHIMS_DIR;$early;$later;$($savedEnv['PATH'])"
        mise reshim --system
        $LASTEXITCODE | Should -Be 0
    }

    AfterAll {
        Set-Location $originalLocation
        foreach ($name in $savedEnv.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process')
        }
        . "$PSScriptRoot\fixture-executables.ps1"
        Wait-MiseFixtureExecutables -Directory $TestDrive
    }

    It 'skips an interpreter-only <Name> before a runnable fallback' -TestCases @(
        @{ Name = 'review-lazy.ps1'; Contents = "Write-Output 'wrong-powershell'" },
        @{ Name = 'review-lazy.vbs'; Contents = 'WScript.Echo "wrong-vbscript"' },
        @{ Name = 'review-lazy'; Contents = "#!/bin/sh`necho wrong-shebang" }
    ) {
        param($Name, $Contents)
        $candidate = Join-Path $early $Name
        try {
            $Contents | Set-Content $candidate
            $viaExec = mise x -- review-lazy 'one argument' 2>&1
            $LASTEXITCODE | Should -Be 23 -Because ($viaExec | Out-String)
            ($viaExec | Out-String).Trim() | Should -Be 'spawnable-fallback:one argument'
            $viaShim = & (Join-Path $env:MISE_SYSTEM_SHIMS_DIR 'review-lazy.exe') 'one argument' 2>&1
            $LASTEXITCODE | Should -Be 23 -Because ($viaShim | Out-String)
            ($viaShim | Out-String).Trim() | Should -Be 'spawnable-fallback:one argument'
            Test-Path (Join-Path $env:MISE_SYSTEM_INSTALLS_DIR 'jq') | Should -BeFalse
        } finally {
            Remove-Item $candidate -ErrorAction Ignore
        }
    }
}
