
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

    It 'converts PATH to MSYS Unix form for bash subshell tasks' {
        # Repro for the per-task `tools = {...}` + `shell = "bash -c"` case.
        # When mise on Windows spawns bash for a task, PATH must be `:`-separated
        # `/c/...` form, not `;`-separated `C:\...` form, or bash cannot resolve
        # any command — including the one mise just installed for the task.
        #
        # We assert on the PATH the task observes, not on a tool install, so the
        # test runs without depending on rust/cargo or any toolchain backend.

        if (-not (Get-Command bash.exe -ErrorAction SilentlyContinue)) {
            Set-ItResult -Skipped -Because "bash.exe (Git Bash / MSYS) not on PATH"
            return
        }

        @'
[tasks.path_repro]
shell = "bash -c"
run = '''
case "$PATH" in
  *\;*)
    echo "PATH-still-windows-style"
    ;;
  *)
    echo "PATH-unix-style"
    ;;
esac
'''
'@ | Out-File -FilePath "mise.path_repro.toml" -Encoding utf8NoBOM

        $env:MISE_CONFIG_FILE = "$TestDrive\mise.path_repro.toml"
        try {
            $output = mise run path_repro 2>&1 | Select -Last 1
            $output | Should -Be 'PATH-unix-style'
        }
        finally {
            Remove-Item -Path Env:\MISE_CONFIG_FILE -ErrorAction SilentlyContinue
            Remove-Item -Path "$TestDrive\mise.path_repro.toml" -ErrorAction SilentlyContinue
        }
    }
}
