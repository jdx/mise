Describe 'project diagnostics' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = $env:MISE_TRUSTED_CONFIG_PATHS
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        Set-Location $TestDrive
    }

    AfterAll {
        Set-Location $script:OriginalDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:OriginalTrusted
    }

    It 'collects failures and runs quoted cmd and PowerShell checks with the project environment' {
        @'
[env]
DOCTOR_VALUE = "project-value"
[doctor.checks.a_fail]
run = "exit /b 7"
shell = "cmd.exe /c"
hint = "Start the services"
[doctor.checks.b_cmd]
run = 'if "%DOCTOR_VALUE%" == "project-value" (exit /b 0) else (exit /b 1)'
shell = "cmd.exe /c"
[doctor.checks.c_pwsh]
os = "win"
run = 'if ($env:DOCTOR_VALUE -eq "project-value") { exit 0 } else { exit 1 }'
shell = "pwsh -Command"
[doctor.checks.d_skip]
run = "exit 1"
os = ["linux", "macos"]
'@ | Set-Content mise.toml
        $output = mise doctor project --json | Out-String
        $LASTEXITCODE | Should -Be 1
        $report = $output | ConvertFrom-Json
        ($report.checks.status -join ',') | Should -Be 'fail,pass,pass,skipped'
        $report.checks[0].hint | Should -Be 'Start the services'
    }

    It 'times out a check and continues to the next check' {
        @'
[doctor.checks.a_timeout]
run = "Start-Sleep -Seconds 60"
shell = "pwsh -Command"
timeout = "500ms"
[doctor.checks.b_pass]
run = "exit 0"
shell = "pwsh -Command"
'@ | Set-Content mise.toml
        $output = mise doctor project --json | Out-String
        $LASTEXITCODE | Should -Be 1
        $report = $output | ConvertFrom-Json
        ($report.checks.status -join ',') | Should -Be 'error,pass'
        $report.checks[0].message | Should -Match 'timed out'
    }
}
