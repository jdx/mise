Describe 'file tasks whose shebang names pwsh' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "mise-tasks") | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        "[tools]" | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        function Write-Task {
            param([string]$Name, [string]$Body)
            $path = Join-Path $script:TestRoot "mise-tasks\$Name"
            # LF, because this file's own line endings are CRLF: a stray `r would ride along
            # inside the bash body below and reach the interpreter as part of the command.
            $lf = $Body -replace "`r`n", "`n"
            [System.IO.File]::WriteAllText($path, $lf, (New-Object System.Text.UTF8Encoding $false))
        }

        # Extensionless plus a pwsh shebang -- the shape docs/tasks/file-tasks.md shows under
        # "Shebang". Windows PowerShell rejects `-File` on anything not named `*.ps1`, so mise
        # has to hand it a name pwsh will accept. The Linux build has no such rule, which is why
        # this only ever failed here.
        Write-Task -Name "pwsh_task" -Body @'
#!/usr/bin/env pwsh
Write-Output "pwsh ran: $($args[0])"
'@

        # Control: found by extension, so it never needed the shim. It pins that the path which
        # already worked still works.
        Write-Task -Name "pwsh_ext.ps1" -Body @'
#!/usr/bin/env pwsh
Write-Output "ext ran: $($args[0])"
'@

        # An explicit `shell` overrides the shebang, and PowerShell's *command* resolution wants
        # `.ps1` exactly as its `-File` handling does. Left out, this one exits 0 having printed
        # nothing at all.
        Write-Task -Name "pwsh_command_mode" -Body @'
#!/usr/bin/env pwsh
#MISE shell="pwsh -c"
Write-Output "command mode ran: $($args[0])"
'@

        # Control: nothing outside PowerShell should change. `printf` rather than `echo` so this
        # can only pass through bash -- cmd has no `printf`.
        Write-Task -Name "bash_task" -Body @'
#!/usr/bin/env bash
printf 'bash ran: %s\n' "$1"
'@

        # A batch file, for the case where the file shell resolves to PowerShell but the task
        # needs no help: PowerShell runs `.cmd` as a native command. Copying it under a `.ps1`
        # name would hand batch syntax to the PowerShell parser instead.
        Write-Task -Name "cmd_task.cmd" -Body @'
@echo off
echo cmd task ran: %1
'@

        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'runs an extensionless task whose shebang names pwsh' {
        $output = mise run pwsh_task ARG1 | Out-String
        $output | Should -Match "pwsh ran: ARG1"
    }

    It 'runs a pwsh task whose shell is in -Command mode' {
        # Asserted on what the task printed, not on the exit code: this one failed by exiting 0
        # and doing nothing, so an exit-code check passes against the bug.
        $output = mise run pwsh_command_mode ARG1 | Out-String
        $output | Should -Match "command mode ran: ARG1"
    }

    It 'still runs the .ps1 sibling' {
        $output = mise run pwsh_ext ARG1 | Out-String
        $output | Should -Match "ext ran: ARG1"
    }

    It 'still runs a bash shebang task' {
        $output = mise run bash_task ARG1 | Out-String
        $output | Should -Match "bash ran: ARG1"
    }

    It 'leaves a batch task alone when the file shell is PowerShell' {
        # `use_file_shell_for_executable_tasks` sends a `.cmd` through the file shell, which here
        # is PowerShell -- and PowerShell runs a `.cmd` as a native command, so it needs no
        # rename. Staging it as `.ps1` would give the parser `@echo off`, which fails while still
        # exiting 0, so this asserts on the output.
        $saved = @{
            UseShell = [Environment]::GetEnvironmentVariable('MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS', 'Process')
            FileArgs = [Environment]::GetEnvironmentVariable('MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS', 'Process')
        }
        try {
            $env:MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS = '1'
            $env:MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS = 'pwsh -c'
            $output = mise run cmd_task ARG1 | Out-String
            $output | Should -Match "cmd task ran: ARG1"
        } finally {
            foreach ($pair in @(
                    @('MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS', $saved.UseShell),
                    @('MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS', $saved.FileArgs))) {
                if ($null -eq $pair[1]) {
                    Remove-Item ("Env:" + $pair[0]) -ErrorAction Ignore
                } else {
                    [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
                }
            }
        }
    }

    It 'leaves no copy behind in the temp directory' {
        # The copy exists only for the length of the run. A leak would deposit one file per
        # invocation, in a directory nothing else cleans.
        $pattern = 'mise-task-*.ps1'
        $before = @(Get-ChildItem -Path $env:TEMP -Filter $pattern -ErrorAction Ignore).Count
        mise run pwsh_task ARG1 | Out-Null
        $after = @(Get-ChildItem -Path $env:TEMP -Filter $pattern -ErrorAction Ignore).Count
        $after | Should -Be $before
    }
}
