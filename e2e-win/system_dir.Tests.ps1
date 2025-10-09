Describe 'MISE_SYSTEM_DIR' {
    BeforeAll {
        $env:MISE_DATA_DIR = Join-Path $TestDrive "data"
        $env:MISE_CONFIG_DIR = Join-Path $TestDrive "config"
    }

    AfterAll {
        Remove-Item Env:\MISE_DATA_DIR -ErrorAction SilentlyContinue
        Remove-Item Env:\MISE_CONFIG_DIR -ErrorAction SilentlyContinue
        Remove-Item Env:\MISE_SYSTEM_DIR -ErrorAction SilentlyContinue
    }

    It 'respects MISE_SYSTEM_DIR environment variable override' {
        $customSystemDir = Join-Path $TestDrive "custom_system"
        $env:MISE_SYSTEM_DIR = $customSystemDir

        # Create the custom system directory with a config file
        # Use 'experimental' setting instead of 'verbose' because MISE_DEBUG=1 forces verbose=true
        New-Item -ItemType Directory -Path $customSystemDir -Force | Out-Null
        $systemConfig = Join-Path $customSystemDir "config.toml"
        Set-Content -Path $systemConfig -Value "[settings]`nexperimental = true"

        # Verify mise can load and use the system config
        $experimentalSetting = mise settings get experimental
        $experimentalSetting | Should -Be "true"
    }

    It 'can change MISE_SYSTEM_DIR to different locations' {
        # First system dir with experimental=true
        $systemDir1 = Join-Path $TestDrive "system1"
        $env:MISE_SYSTEM_DIR = $systemDir1
        New-Item -ItemType Directory -Path $systemDir1 -Force | Out-Null
        Set-Content -Path (Join-Path $systemDir1 "config.toml") -Value "[settings]`nexperimental = true"

        $setting1 = mise settings get experimental
        $setting1 | Should -Be "true"

        # Second system dir with experimental=false
        $systemDir2 = Join-Path $TestDrive "system2"
        $env:MISE_SYSTEM_DIR = $systemDir2
        New-Item -ItemType Directory -Path $systemDir2 -Force | Out-Null
        Set-Content -Path (Join-Path $systemDir2 "config.toml") -Value "[settings]`nexperimental = false"

        $setting2 = mise settings get experimental
        $setting2 | Should -Be "false"
    }

    It 'does not use Unix /etc/mise path on Windows' {
        # Create a temporary system dir to avoid using default
        $tempSystemDir = Join-Path $TestDrive "temp_system"
        $env:MISE_SYSTEM_DIR = $tempSystemDir
        New-Item -ItemType Directory -Path $tempSystemDir -Force | Out-Null

        # Run doctor and ensure no /etc/mise references in output
        $output = mise doctor 2>&1 | Out-String
        $output | Should -Not -Match "/etc/mise"
    }
}
