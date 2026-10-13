Describe 'install layout default' {
    # On Windows, experimental = true turns the identity layout on when install_layout is unset,
    # and install_layout = "legacy" keeps the legacy layout.

    BeforeAll {
        $script:OriginalLocation = Get-Location
        $script:Saved = @{}
        foreach ($name in 'MISE_DATA_DIR', 'MISE_INSTALLS_DIR', 'MISE_INSTALL_STORE_DIR', 'MISE_CONFIG_FILE', 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_EXPERIMENTAL', 'MISE_INSTALL_LAYOUT', 'MISE_YES') {
            $script:Saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

        # Installs jq 1.7.1 into a fresh data directory and returns what mise where reports.
        function script:InstallJq([string]$Name) {
            $root = Join-Path $TestDrive $Name
            New-Item -ItemType Directory -Path $root | Out-Null
            Set-Location $root
            $env:MISE_DATA_DIR = Join-Path $root 'data'
            $env:MISE_CONFIG_FILE = Join-Path $root 'mise.toml'
            $env:MISE_TRUSTED_CONFIG_PATHS = $root
            '' | Out-File -FilePath $env:MISE_CONFIG_FILE -Encoding utf8NoBOM
            mise install jq@1.7.1 | Out-Null
            $LASTEXITCODE | Should -Be 0
            (mise where jq@1.7.1 | Select-Object -Last 1).Trim()
        }

        Remove-Item -Path Env:\MISE_INSTALLS_DIR, Env:\MISE_INSTALL_STORE_DIR, Env:\MISE_INSTALL_LAYOUT -ErrorAction SilentlyContinue
        $env:MISE_YES = '1'
    }

    AfterAll {
        Set-Location $script:OriginalLocation
        foreach ($name in $script:Saved.Keys) {
            if ($null -eq $script:Saved[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:Saved[$name], 'Process')
            }
        }
    }

    AfterEach {
        Remove-Item -Path Env:\MISE_INSTALL_LAYOUT -ErrorAction SilentlyContinue
    }

    It 'uses the identity layout when experimental is on and install_layout is unset' {
        $env:MISE_EXPERIMENTAL = '1'
        $dir = InstallJq 'experimental'
        (Split-Path $dir -Leaf) | Should -Match '^jq-[a-z2-7]{8}$'
        (Split-Path $dir -Parent) | Should -Be (Join-Path $env:MISE_DATA_DIR 'i')
    }

    It 'keeps the legacy layout when install_layout is legacy' {
        $env:MISE_EXPERIMENTAL = '1'
        $env:MISE_INSTALL_LAYOUT = 'legacy'
        $dir = InstallJq 'legacy'
        $dir | Should -Be (Join-Path $env:MISE_DATA_DIR 'installs\jq\1.7.1')
        Test-Path -LiteralPath (Join-Path $env:MISE_DATA_DIR 'i') | Should -BeFalse
    }

    It 'keeps the legacy layout when experimental is off' {
        $env:MISE_EXPERIMENTAL = '0'
        $dir = InstallJq 'stable'
        $dir | Should -Be (Join-Path $env:MISE_DATA_DIR 'installs\jq\1.7.1')
    }
}
