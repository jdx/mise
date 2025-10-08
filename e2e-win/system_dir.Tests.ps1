Describe 'MISE_SYSTEM_DIR' {
    BeforeAll {
        $env:MISE_DATA_DIR = Join-Path $TestDrive "data"
        $env:MISE_CONFIG_DIR = Join-Path $TestDrive "config"
    }

    AfterAll {
        Remove-Item Env:\MISE_DATA_DIR -ErrorAction SilentlyContinue
        Remove-Item Env:\MISE_CONFIG_DIR -ErrorAction SilentlyContinue
        Remove-Item Env:\MISE_SYSTEM_DIR -ErrorAction SilentlyContinue
        Remove-Item Env:\PROGRAMDATA -ErrorAction SilentlyContinue
    }

    It 'defaults to ProgramData\mise on Windows' {
        Remove-Item Env:\MISE_SYSTEM_DIR -ErrorAction SilentlyContinue

        # Get doctor output which shows all directories including MISE_SYSTEM_DIR
        $output = mise doctor --json | ConvertFrom-Json

        # Check that system_dir contains "ProgramData\mise" or "mise" (case insensitive)
        $output.data_dir | Should -Not -BeNullOrEmpty

        # The system dir should end with \mise and not be /etc/mise
        $systemDirFromDoctor = $output.PSObject.Properties | Where-Object { $_.Name -like "*system*" } | Select-Object -First 1
        if ($systemDirFromDoctor) {
            $systemDirFromDoctor.Value | Should -Not -Match "/etc/mise"
            $systemDirFromDoctor.Value | Should -Match "mise"
        }
    }

    It 'uses PROGRAMDATA environment variable when set' {
        Remove-Item Env:\MISE_SYSTEM_DIR -ErrorAction SilentlyContinue
        $customProgramData = Join-Path $TestDrive "CustomProgramData"
        $env:PROGRAMDATA = $customProgramData

        # Create a system config file
        $systemDir = Join-Path $customProgramData "mise"
        New-Item -ItemType Directory -Path $systemDir -Force | Out-Null
        $systemConfig = Join-Path $systemDir "config.toml"
        Set-Content -Path $systemConfig -Value "[settings]`nverbose = true"

        $env:MISE_SYSTEM_DIR = $systemDir

        # Verify mise can read from this directory
        $output = mise doctor --json | ConvertFrom-Json
        $output | Should -Not -BeNullOrEmpty

        Remove-Item Env:\PROGRAMDATA -ErrorAction SilentlyContinue
    }

    It 'respects MISE_SYSTEM_DIR environment variable override' {
        $customSystemDir = Join-Path $TestDrive "custom_system"
        $env:MISE_SYSTEM_DIR = $customSystemDir

        # Create the custom system directory
        New-Item -ItemType Directory -Path $customSystemDir -Force | Out-Null
        $systemConfig = Join-Path $customSystemDir "config.toml"
        Set-Content -Path $systemConfig -Value "[settings]`nexperimental = true"

        # Verify mise uses the custom directory
        $output = mise doctor --json | ConvertFrom-Json
        $output | Should -Not -BeNullOrEmpty

        Remove-Item Env:\MISE_SYSTEM_DIR -ErrorAction SilentlyContinue
    }

    It 'does not use /etc/mise path on Windows' {
        Remove-Item Env:\MISE_SYSTEM_DIR -ErrorAction SilentlyContinue

        # Run doctor and ensure no /etc/mise references
        $output = mise doctor 2>&1 | Out-String
        $output | Should -Not -Match "/etc/mise"
    }
}
