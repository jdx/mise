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

        # Create the custom system directory with a config file that sets an env var
        New-Item -ItemType Directory -Path $customSystemDir -Force | Out-Null
        $systemConfig = Join-Path $customSystemDir "config.toml"
        Set-Content -Path $systemConfig -Value "[env]`nTEST_SYSTEM_VAR_1 = 'from_system_config'"

        # Verify mise can load and use the system config by checking the env var
        $output = mise env --json | ConvertFrom-Json
        $output.TEST_SYSTEM_VAR_1 | Should -Be "from_system_config"
    }

    It 'can change MISE_SYSTEM_DIR to different locations' {
        # First system dir with TEST_VAR_A
        $systemDir1 = Join-Path $TestDrive "system1"
        $env:MISE_SYSTEM_DIR = $systemDir1
        New-Item -ItemType Directory -Path $systemDir1 -Force | Out-Null
        Set-Content -Path (Join-Path $systemDir1 "config.toml") -Value "[env]`nTEST_VAR_A = 'value_from_system1'"

        $output1 = mise env --json | ConvertFrom-Json
        $output1.TEST_VAR_A | Should -Be "value_from_system1"

        # Second system dir with TEST_VAR_B (different variable name to avoid caching)
        $systemDir2 = Join-Path $TestDrive "system2"
        $env:MISE_SYSTEM_DIR = $systemDir2
        New-Item -ItemType Directory -Path $systemDir2 -Force | Out-Null
        Set-Content -Path (Join-Path $systemDir2 "config.toml") -Value "[env]`nTEST_VAR_B = 'value_from_system2'"

        $output2 = mise env --json | ConvertFrom-Json
        $output2.TEST_VAR_B | Should -Be "value_from_system2"
        # TEST_VAR_A should not exist in this context
        $output2.PSObject.Properties.Name | Should -Not -Contain "TEST_VAR_A"
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
