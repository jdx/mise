
Describe 'task' {
    BeforeAll {
        $originalPath = Get-Location
        Set-Location TestDrive:
        # Saved before overwriting, restored in AfterAll: Pester runs every suite in one process,
        # so removing an inherited value here would leave the next suite without it.
        $script:originalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        # Trust the TestDrive config path - use $TestDrive for physical path, not PSDrive path
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        # Saved here, restored in AfterAll: the tests below set these and `BeforeEach` only clears
        # them between tests in this file.
        $script:originalShellEnv = @{}
        foreach ($name in 'MISE_WINDOWS_EXECUTABLE_EXTENSIONS',
            'MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS',
            'MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS') {
            $script:originalShellEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

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

        # A task file with neither an extension nor a shebang. Deliberately *not* named
        # `filetask`: that collapses to the same task name as `filetask.bat`, and the `.bat` is
        # what runs, so nothing here would be exercised.
        @'
@echo off
echo from-noext
'@ | Out-File -FilePath "tasks\noexttask" -Encoding ascii -NoNewline

        # Create testtask.ps1 for pwsh test
        @'
Write-Output "windows"
'@ | Out-File -FilePath "tasks\testtask.ps1" -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $originalPath
        if ($null -eq $script:originalTrusted) {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:originalTrusted, 'Process')
        }
        # `BeforeEach` clears these for tests *inside* this Describe, but Pester runs every suite
        # in one process, so whatever the last test left set would leak into the next suite.
        foreach ($name in $script:originalShellEnv.Keys) {
            if ($null -eq $script:originalShellEnv[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:originalShellEnv[$name], 'Process')
            }
        }
    }

    BeforeEach {
        Remove-Item -Path Env:\MISE_WINDOWS_EXECUTABLE_EXTENSIONS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS -ErrorAction SilentlyContinue
    }

    It 'executes a task' {
        mise run filetask.bat | Select -Last 1 | Should -Be 'mytask'
    }

    It 'uses native separators in task path environment variables' {
        @'
[tasks.path_env]
shell = "pwsh -NoProfile -Command"
quiet = true
run = '''
[ordered]@{
    original_cwd = $env:MISE_ORIGINAL_CWD
    config_root = $env:MISE_CONFIG_ROOT
    project_root = $env:MISE_PROJECT_ROOT
    task_dir = $env:MISE_TASK_DIR
    task_file = $env:MISE_TASK_FILE
} | ConvertTo-Json -Compress
'''
'@ | Out-File -FilePath "$TestDrive\mise.task-path-env.toml" -Encoding utf8NoBOM

        $oldConfig = $env:MISE_CONFIG_FILE
        # Preserve the slash spelling that config discovery can produce when a global config is
        # found through `.config/mise/config.toml` relative to the invocation directory (#12160).
        $env:MISE_CONFIG_FILE = "$TestDrive/mise.task-path-env.toml"
        try {
            $paths = mise run path_env | ConvertFrom-Json
            $LASTEXITCODE | Should -Be 0
            foreach ($property in $paths.PSObject.Properties) {
                $property.Value.Contains('/') | Should -BeFalse -Because (
                    "$($property.Name) used mixed separators: $($property.Value)"
                )
            }
        }
        finally {
            if ($null -eq $oldConfig) {
                Remove-Item -Path Env:\MISE_CONFIG_FILE -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CONFIG_FILE = $oldConfig
            }
            Remove-Item -Path "$TestDrive\mise.task-path-env.toml" -ErrorAction SilentlyContinue
        }
    }

    # `windows_executable_extensions` is what decides whether a file with no extension and no
    # shebang is a task at all -- there is no permission bit here for `is_executable` to consult.
    # Nothing else in this suite covers that boundary, and it is the reason the extensionless
    # fixture above is invisible by default.
    It 'does not discover an extensionless file task by default' {
        $out = mise tasks ls | Out-String
        # Both guards matter for the negative assertion below: it would also hold if the command
        # had failed outright, or listed nothing at all.
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*filetask*'
        $out | Should -Not -BeLike '*noexttask*'
    }

    It 'discovers an extensionless file task when the setting admits it' {
        # The leading comma is the empty entry, which is what matches a file with no extension.
        $env:MISE_WINDOWS_EXECUTABLE_EXTENSIONS = ",exe,bat,cmd"
        $out = mise tasks ls | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*noexttask*'
    }

    It 'executes a shebang task with bash' {
        # Create a file task with a bash shebang and no extension
        @"
#!/usr/bin/env bash
echo "from-bash"
"@ | Out-File -FilePath "tasks\shebangtask" -Encoding utf8NoBOM -NoNewline
        mise run shebangtask | Select -Last 1 | Should -Be 'from-bash'
    }

    # File tasks cannot branch on platform -- `run_windows` is a TOML-task key and is rejected in a
    # file-task header -- so writing the task twice, once per platform, is the only option. Both
    # files reduce to the same task name, and mise used to keep both and run both.
    #
    # End to end because the unit tests can only check the preference in isolation: it is the chain
    # of discovery (shebang admits the `.sh`), naming (the stem, so both become `platpair`) and
    # preference that produces the collision.
    It 'prefers the Windows script when a task exists in both forms' {
        @"
#!/usr/bin/env bash
echo "from-posix"
"@ | Out-File -FilePath "tasks\platpair.sh" -Encoding utf8NoBOM -NoNewline
        "Write-Output 'from-windows'" | Out-File -FilePath "tasks\platpair.ps1" -Encoding utf8NoBOM -NoNewline

        # One task, not two entries sharing a name.
        (mise tasks --json | ConvertFrom-Json | Where-Object { $_.name -eq 'platpair' }).Count |
            Should -Be 1

        # And the POSIX half must not run. Asserting the absence as well as the presence: with both
        # kept, this printed `from-posix` too and still ended on the Windows line.
        $out = mise run platpair 2>&1 | Out-String
        $out | Should -BeLike '*from-windows*'
        $out | Should -Not -BeLike '*from-posix*'

        Remove-Item "tasks\platpair.sh", "tasks\platpair.ps1" -ErrorAction Ignore
    }

    It 'executes a task in pwsh' {
        $env:MISE_WINDOWS_EXECUTABLE_EXTENSIONS = "ps1"
        $env:MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS = "pwsh.exe"
        $env:MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS = "true"
        mise run testtask | Select -Last 1 | Should -Be 'windows'
    }

    It 'leaves a bash task PATH that both bash and a native grandchild can use' {
        # mise used to convert PATH to MSYS Unix form before spawning bash. A shell that sets
        # MSYSTEM -- `C:\Program Files\Git\bin\bash.exe`, which is what `bash` resolves to and
        # what mise picks by default -- prepends its own entries at startup and keeps the
        # inherited POSIX string as a single opaque element. Everything mise had put on PATH
        # then reached the next native process buried inside one entry: measured on CI, 78
        # entries became 4 and a native grandchild could resolve none of them.
        #
        # A shell that leaves MSYSTEM unset re-parses the converted value and survives it, so
        # this pins itself to a shell that sets MSYSTEM rather than quietly turning into a test
        # that cannot fail on a host where `bash` resolves somewhere else.

        # Mirror the task executor's own resolution rather than accepting anything named
        # bash.exe: it refuses the WSL launcher at System32, and on a host that offers only
        # that one there is no shell for this test to run.
        $bashCandidates = @($env:MISE_BASH_PATH) + @(
            'C:\Program Files\Git\bin\bash.exe',
            'C:\Program Files (x86)\Git\bin\bash.exe',
            "$env:LOCALAPPDATA\Programs\Git\bin\bash.exe",
            'C:\msys64\usr\bin\bash.exe',
            'C:\msys32\usr\bin\bash.exe'
        ) + @(Get-Command bash.exe -All -ErrorAction SilentlyContinue | ForEach-Object { $_.Source })
        $wslLauncher = Join-Path $env:SystemRoot 'System32\bash.exe'
        $posixBash = $bashCandidates |
            Where-Object { $_ -and ($_ -ne $wslLauncher) -and (Test-Path $_) } |
            Select-Object -First 1
        if (-not $posixBash) {
            Set-ItResult -Skipped -Because "no POSIX bash (Git Bash / MSYS2) on this host; the WSL launcher does not count"
            return
        }

        # A directory only this test puts on PATH, so its arrival at the far end is evidence
        # that the whole list survived rather than that something else happened to be there.
        $markerDir = Join-Path $TestDrive 'marker dir'
        New-Item -ItemType Directory -Path $markerDir -Force | Out-Null

        @'
[tasks.shell_env]
shell = "bash -c"
run = 'echo "MSYSTEM=${MSYSTEM-<unset>}"'

[tasks.bash_sees]
shell = "bash -c"
run = 'echo "$PATH"'

[tasks.grandchild_sees]
shell = "bash -c"
run = "powershell -NoProfile -Command '$env:PATH'"
'@ | Out-File -FilePath "$TestDrive\mise.path_shape.toml" -Encoding utf8NoBOM

        $oldConfig = $env:MISE_CONFIG_FILE
        $oldPath = $env:PATH
        $env:MISE_CONFIG_FILE = "$TestDrive\mise.path_shape.toml"
        $env:PATH = "$markerDir;$oldPath"
        try {
            $shellEnv = "$(mise run shell_env 2>&1 | Select -Last 1)"
            $LASTEXITCODE | Should -Be 0
            $shellEnv | Should -Match '^MSYSTEM='
            if ($shellEnv -eq 'MSYSTEM=<unset>') {
                Set-ItResult -Skipped -Because "the resolved bash leaves MSYSTEM unset; the regression this covers only reaches a shell that sets it"
                return
            }

            # Exit code and emptiness first: `Should -Not -Match` holds for no output at all, and a
            # test that passes when the task never ran is the shape this PR is removing.
            $inBash = "$(mise run bash_sees 2>&1 | Select -Last 1)"
            $LASTEXITCODE | Should -Be 0
            $inBash | Should -Not -BeNullOrEmpty
            $inBash | Should -Not -Match ';'

            $inGrandchild = "$(mise run grandchild_sees 2>&1 | Select -Last 1)"
            $LASTEXITCODE | Should -Be 0
            $inGrandchild | Should -Not -BeNullOrEmpty
            # A trailing ';' is ordinary, so empty entries are not the subject here.
            $entries = @($inGrandchild -split ';' | Where-Object { $_ -ne '' })
            $entries.Count | Should -BeGreaterThan 1
            # Every entry a Windows path -- a drive letter or a UNC share, both of which PATH
            # takes. The defect put a whole ':'-joined list in one of them.
            foreach ($e in $entries) {
                $e | Should -Match '^(?:[A-Za-z]:|\\\\)'
                $e.Substring(2) | Should -Not -Match ':'
            }
            # The shape assertions above would still hold if the list had been truncated to the
            # shell's own entries, which is exactly what the defect did. This is the one that
            # says nothing was dropped on the way.
            $inGrandchild | Should -Match ([regex]::Escape($markerDir))
        }
        finally {
            $env:PATH = $oldPath
            if ($null -eq $oldConfig) {
                Remove-Item -Path Env:\MISE_CONFIG_FILE -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CONFIG_FILE = $oldConfig
            }
            Remove-Item -Path "$TestDrive\mise.path_shape.toml" -ErrorAction SilentlyContinue
        }
    }

    It 'forwards args to a bash subshell task without shifting $0' {
        # Repro for the Windows non-cmd POSIX-shell arg-swallow bug (#9355): with
        # shell = "bash -c", a forwarded arg used to be passed as a separate argv
        # to `bash -c`, so the user's first arg became $0. Inline TOML scripts
        # append args to the command (like Unix), so $0 stays the shell (bash) and
        # the arg is appended after it, not `using shell myarg`, where the arg had
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
            # proves the fix regardless of that form: $0 still names bash (not
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

    Context 'pwsh -NoProfile injection' {
        # A pwsh inline shell should be spawned with -NoProfile so startup
        # profiles (which can mutate PATH and shadow task tools) are skipped,
        # matching the non-interactive behavior of `sh -c`/`zsh -c`. See
        # discussion #10956. The task prints its own process argv so we can
        # assert on how pwsh was actually invoked.
        BeforeEach {
            $script:noProfileTestEnv = @{}
            foreach ($name in @('MISE_CONFIG_FILE', 'MISE_WINDOWS_POWERSHELL_NO_PROFILE')) {
                $script:noProfileTestEnv[$name] = @{
                    Exists = Test-Path "Env:\$name"
                    Value = [Environment]::GetEnvironmentVariable($name, 'Process')
                }
            }
            Remove-Item -Path Env:\MISE_WINDOWS_POWERSHELL_NO_PROFILE -ErrorAction SilentlyContinue
            @'
[task_config]
includes = ["tasks"]

[tasks.probe_argv]
shell = "pwsh -c"
run = 'Write-Output ([Environment]::GetCommandLineArgs() -join " ")'
'@ | Out-File -FilePath "$TestDrive\mise.noprofile.toml" -Encoding utf8NoBOM
            @'
Write-Output ([Environment]::GetCommandLineArgs() -join " ")
'@ | Out-File -FilePath "$TestDrive\tasks\probe_file_argv.ps1" -Encoding utf8NoBOM
            $env:MISE_CONFIG_FILE = "$TestDrive\mise.noprofile.toml"
        }

        AfterEach {
            foreach ($name in @('MISE_CONFIG_FILE', 'MISE_WINDOWS_POWERSHELL_NO_PROFILE')) {
                $original = $script:noProfileTestEnv[$name]
                if ($original.Exists) {
                    [Environment]::SetEnvironmentVariable($name, $original.Value, 'Process')
                } else {
                    [Environment]::SetEnvironmentVariable($name, $null, 'Process')
                }
            }
            $script:noProfileTestEnv = $null
            Remove-Item -Path "$TestDrive\mise.noprofile.toml" -ErrorAction SilentlyContinue
            Remove-Item -Path "$TestDrive\tasks\probe_file_argv.ps1" -ErrorAction SilentlyContinue
        }

        It 'injects -NoProfile into a pwsh task shell by default' {
            $output = mise run probe_argv 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $output | Should -BeLike '*-NoProfile*'
        }

        It 'omits -NoProfile when windows_powershell_no_profile is disabled' {
            $env:MISE_WINDOWS_POWERSHELL_NO_PROFILE = "false"
            $output = mise run probe_argv 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $output | Should -Not -BeLike '*-NoProfile*'
        }

        It 'injects -NoProfile into a PowerShell file task' {
            $output = mise run probe_file_argv 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $output | Should -BeLike '*-NoProfile*'
        }
    }

    Context 'a task file Windows cannot execute' {
        BeforeAll {
            $script:notExecDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
            New-Item -ItemType Directory -Path (Join-Path $script:notExecDir 'scripts') | Out-Null
            # No shebang and no known extension, which is what `is_executable` looks at here --
            # there is no permission bit involved on this platform.
            Set-Content (Join-Path $script:notExecDir 'scripts\thing') 'echo hi' -NoNewline
            @'
[tasks.x]
file = "scripts/thing"
'@ | Out-File -FilePath (Join-Path $script:notExecDir 'mise.toml') -Encoding utf8NoBOM
        }

        It 'does not tell the user to run chmod' {
            Push-Location $script:notExecDir
            try {
                $out = mise tasks validate 2>&1 | Out-String
                $out | Should -BeLike '*not executable*'
                # chmod does not exist here, and it is not the fix either: adding a shebang or a
                # known extension is.
                $out | Should -Not -BeLike '*chmod*'
                $out | Should -BeLike '*shebang*'
            } finally {
                Pop-Location
            }
        }

        It 'says how to fix it when the task is run' {
            Push-Location $script:notExecDir
            try {
                $out = mise run x 2>&1 | Out-String
                $LASTEXITCODE | Should -Not -Be 0
                $out | Should -BeLike '*not executable*'
                $out | Should -BeLike '*shebang*'
                $out | Should -Not -BeLike '*chmod*'
            } finally {
                Pop-Location
            }
        }
    }

    Context 'a task-directory file Windows will not run' {
        BeforeAll {
            # Deliberately outside $TestDrive: the config at the TestDrive root includes a tasks
            # directory, and a project that inherits those tasks never reaches the two diagnostics
            # that only fire when nothing resolved at all.
            $script:skipRoot = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
            $script:aloneDir = Join-Path $script:skipRoot 'alone'
            $script:besideDir = Join-Path $script:skipRoot 'beside'
            New-Item -ItemType Directory -Path (Join-Path $script:aloneDir 'mise-tasks') -Force | Out-Null
            New-Item -ItemType Directory -Path (Join-Path $script:besideDir 'mise-tasks') -Force | Out-Null

            # No shebang and no known extension. There is no permission bit for `is_executable` to
            # consult on this platform, so this file is simply not a task.
            Set-Content (Join-Path $script:aloneDir 'mise-tasks\skipped') 'echo hi' -NoNewline
            New-Item -ItemType File -Path (Join-Path $script:aloneDir 'mise.toml') | Out-Null

            # The same file with a working task beside it, which sends `mise run` down its other
            # branch -- the one that has a task list to suggest from. The warning is the only
            # thing that names the file there.
            Set-Content (Join-Path $script:besideDir 'mise-tasks\skipped') 'echo hi' -NoNewline
            Set-Content (Join-Path $script:besideDir 'mise-tasks\works.bat') "@echo off`r`necho works"
            New-Item -ItemType File -Path (Join-Path $script:besideDir 'mise.toml') | Out-Null

            # Global tasks would also keep these projects from being empty, and this suite does
            # not otherwise isolate them.
            $script:originalConfigDir = [Environment]::GetEnvironmentVariable('MISE_CONFIG_DIR', 'Process')
            $env:MISE_CONFIG_DIR = Join-Path $script:skipRoot 'config'
            New-Item -ItemType Directory -Path $env:MISE_CONFIG_DIR | Out-Null
            $script:describeTrusted = $env:MISE_TRUSTED_CONFIG_PATHS
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:skipRoot
        }

        AfterAll {
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:describeTrusted
            if ($null -eq $script:originalConfigDir) {
                Remove-Item -Path Env:\MISE_CONFIG_DIR -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CONFIG_DIR = $script:originalConfigDir
            }
            Remove-Item -Recurse -Force $script:skipRoot -ErrorAction SilentlyContinue
        }

        It 'says why `mise tasks ls` found nothing' {
            Push-Location $script:aloneDir
            try {
                $out = mise tasks ls 2>&1 | Out-String
                $LASTEXITCODE | Should -Be 0
                $out | Should -BeLike '*non-executable*'
                $out | Should -BeLike '*shebang*'
                # Being told to run chmod is why this diagnostic was switched off here at all.
                $out | Should -Not -BeLike '*chmod*'
            } finally {
                Pop-Location
            }
        }

        It 'names the file when the project has no other tasks' {
            Push-Location $script:aloneDir
            try {
                $out = mise run skipped 2>&1 | Out-String
                $LASTEXITCODE | Should -Not -Be 0
                $out | Should -BeLike '*non-executable*'
                $out | Should -BeLike '*mise-tasks*'
                $out | Should -BeLike '*shebang*'
                $out | Should -Not -BeLike '*chmod*'
            } finally {
                Pop-Location
            }
        }

        It 'names the file when the project has other tasks' {
            Push-Location $script:besideDir
            try {
                # The control: the sibling really is a task here, so the run below fails over the
                # skipped file rather than over an empty project.
                mise run works | Select -Last 1 | Should -Be 'works'
                $out = mise run skipped 2>&1 | Out-String
                $LASTEXITCODE | Should -Not -Be 0
                $out | Should -BeLike '*non-executable*'
                $out | Should -BeLike '*mise-tasks*'
                $out | Should -BeLike '*shebang*'
                $out | Should -Not -BeLike '*chmod*'
            } finally {
                Pop-Location
            }
        }
    }
}
