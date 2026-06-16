
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

    It 'converts PATH to /cygdrive form for a Cygwin bash subshell task' {
        # Cygwin resolves drives via `/cygdrive/c/...`, not Git Bash's `/c/...`.
        # When mise detects a Cygwin bash (here pinned via MISE_BASH_PATH), the
        # task's PATH must use the `/cygdrive/` form or commands won't resolve.
        # Skipped unless Cygwin is actually installed, since CI runners lack it.

        $cygwinBash = "C:\cygwin64\bin\bash.exe"
        if (-not (Test-Path $cygwinBash)) {
            Set-ItResult -Skipped -Because "Cygwin bash not installed at $cygwinBash"
            return
        }

        @'
[tasks.cygdrive_repro]
shell = "bash -c"
run = '''
case "$PATH" in
  */cygdrive/*)
    echo "PATH-cygdrive-style"
    ;;
  *)
    echo "PATH-not-cygdrive"
    ;;
esac
'''
'@ | Out-File -FilePath "mise.cygdrive_repro.toml" -Encoding utf8NoBOM

        # Save and restore the env vars we override so a dev machine's real
        # settings (a developer may export MISE_BASH_PATH / MISE_CYGDRIVE_PREFIX)
        # and later tests are not disturbed. Pin MISE_CYGDRIVE_PREFIX to the
        # default so the `/cygdrive` assertion holds even when the caller has
        # exported a custom prefix such as `/mnt` (which the feature honors).
        $oldConfig = $env:MISE_CONFIG_FILE
        $oldBashPath = $env:MISE_BASH_PATH
        $oldCygPrefix = $env:MISE_CYGDRIVE_PREFIX
        $env:MISE_CONFIG_FILE = "$TestDrive\mise.cygdrive_repro.toml"
        $env:MISE_BASH_PATH = $cygwinBash
        $env:MISE_CYGDRIVE_PREFIX = "/cygdrive"
        try {
            $output = mise run cygdrive_repro 2>&1 | Select -Last 1
            $output | Should -Be 'PATH-cygdrive-style'
        }
        finally {
            if ($null -eq $oldConfig) {
                Remove-Item -Path Env:\MISE_CONFIG_FILE -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CONFIG_FILE = $oldConfig
            }
            if ($null -eq $oldBashPath) {
                Remove-Item -Path Env:\MISE_BASH_PATH -ErrorAction SilentlyContinue
            } else {
                $env:MISE_BASH_PATH = $oldBashPath
            }
            if ($null -eq $oldCygPrefix) {
                Remove-Item -Path Env:\MISE_CYGDRIVE_PREFIX -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CYGDRIVE_PREFIX = $oldCygPrefix
            }
            Remove-Item -Path "$TestDrive\mise.cygdrive_repro.toml" -ErrorAction SilentlyContinue
        }
    }

    It 'forwards args to a bash subshell task without shifting $0' {
        # Repro for the Windows non-cmd POSIX-shell arg-swallow bug (#9355): with
        # shell = "bash -c", a forwarded arg used to be passed as a separate argv
        # to `bash -c`, so the user's first arg became $0. Inline TOML scripts
        # append args to the command (like Unix), so $0 stays the shell (bash) and
        # the arg is appended after it — not `using shell myarg`, where the arg had
        # been swallowed into $0.
        if (-not (Get-Command bash.exe -ErrorAction SilentlyContinue)) {
            Set-ItResult -Skipped -Because "bash.exe (Git Bash / MSYS) not on PATH"
            return
        }

        @'
[tasks.args_repro]
shell = "bash -c"
run = 'echo "using shell $0"'
'@ | Out-File -FilePath "mise.args_repro.toml" -Encoding utf8NoBOM

        $oldConfig = $env:MISE_CONFIG_FILE
        $env:MISE_CONFIG_FILE = "$TestDrive\mise.args_repro.toml"
        try {
            # $0 is the shell bash was invoked as: "bash" on some setups, a full
            # path like "/usr/bin/bash" on Git Bash. Assert on the shape that
            # proves the fix regardless of that form — $0 still names bash (not
            # the forwarded arg) and "myarg" is appended as the trailing token,
            # rather than being swallowed into $0 (the old bug printed
            # "using shell myarg").
            $output = mise run args_repro -- myarg 2>&1 | Select -Last 1
            $output | Should -BeLike '*bash* myarg'
        }
        finally {
            if ($null -eq $oldConfig) {
                Remove-Item -Path Env:\MISE_CONFIG_FILE -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CONFIG_FILE = $oldConfig
            }
            Remove-Item -Path "$TestDrive\mise.args_repro.toml" -ErrorAction SilentlyContinue
        }
    }
}
