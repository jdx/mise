Describe 'mise run --timeout' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:
        $script:OriginalEnv = @{}
        foreach ($name in 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_FRIENDLY_ERROR') {
            $script:OriginalEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        # A friendly error exits through `exit::kill_all`, which hid the leak in release builds
        # (debug builds, which CI tests, never take that path). Test the run's own cleanup.
        $env:MISE_FRIENDLY_ERROR = '0'
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($name in $script:OriginalEnv.Keys) {
            if ($null -eq $script:OriginalEnv[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:OriginalEnv[$name], 'Process')
            }
        }
    }

    AfterEach {
        if ($script:TaskPid) {
            Stop-Process -Id $script:TaskPid -Force -ErrorAction SilentlyContinue
        }
    }

    It 'stops the running task when the whole run times out' {
        # The task records the PID of a grandchild of mise (mise -> cmd -> powershell), so the
        # check below covers the process tree, not just the shell mise started.
        @'
[tasks.stuck]
run = "powershell -NoProfile -Command \"Set-Content -Path task.pid -Value $PID; Start-Sleep -Seconds 120\""
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        # Output goes to files, never a pipe: a leaked task inherits mise's handles, and a pipe it
        # held open would keep this test waiting until the task ended on its own.
        $mise = Start-Process -FilePath mise -ArgumentList 'run', '--timeout', '10s', 'stuck' `
            -RedirectStandardOutput out.log -RedirectStandardError err.log -NoNewWindow -PassThru
        $mise.WaitForExit()
        Get-Content err.log -Raw | Should -Match 'timed out'
        'task.pid' | Should -Exist -Because 'the task must start before the timeout for this test to mean anything'

        $script:TaskPid = [int](Get-Content task.pid)
        Wait-Process -Id $script:TaskPid -Timeout 5 -ErrorAction SilentlyContinue
        Get-Process -Id $script:TaskPid -ErrorAction SilentlyContinue | Should -BeNullOrEmpty
    }
}
