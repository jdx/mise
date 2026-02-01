
Describe 'task' {
    BeforeAll {
        $originalPath = Get-Location
        Set-Location TestDrive:
        # Trust the TestDrive config path - use $TestDrive for physical path, not PSDrive path
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        # Create mise.toml that includes tasks directory
        @'
[task_config]
includes = ["tasks"]
'@ | Out-File -FilePath "mise.toml" -Encoding utf8NoBOM

        # Create tasks directory
        New-Item -ItemType Directory -Path "tasks" -Force | Out-Null

        # Create filetask.bat
        @'
@echo off
echo mytask
'@ | Out-File -FilePath "tasks\filetask.bat" -Encoding ascii -NoNewline

        # Create filetask (no extension) for MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS test
        @'
@echo off
echo mytask
'@ | Out-File -FilePath "tasks\filetask" -Encoding ascii -NoNewline

        # Create testtask.ps1 for pwsh test
        @'
Write-Output "windows"
'@ | Out-File -FilePath "tasks\testtask.ps1" -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $originalPath
        Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
    }

    BeforeEach {
        Remove-Item -Path Env:\MISE_WINDOWS_EXECUTABLE_EXTENSIONS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS -ErrorAction SilentlyContinue
    }

    It 'executes a task' {
        mise run filetask.bat | Select -Last 1 | Should -Be 'mytask'
    }

    It 'executes a task without extension' {
        $env:MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS = "bat"
        mise run filetask | Select -Last 1 | Should -Be 'mytask'
    }

    It 'executes a shebang task with bash' {
        # Create a file task with a bash shebang and no extension
        @"
#!/usr/bin/env bash
echo "from-bash"
"@ | Out-File -FilePath "tasks\shebangtask" -Encoding utf8NoBOM -NoNewline
        mise run shebangtask | Select -Last 1 | Should -Be 'from-bash'
    }

    It 'executes a task in pwsh' {
        $env:MISE_WINDOWS_EXECUTABLE_EXTENSIONS = "ps1"
        $env:MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS = "pwsh.exe"
        $env:MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS = "true"
        mise run testtask | Select -Last 1 | Should -Be 'windows'
    }
}
