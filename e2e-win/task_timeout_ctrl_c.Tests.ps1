Describe 'task timeout on Windows' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        # Ctrl+C can only be raised on a console. mise shares this one, and raises Ctrl+C on the
        # timed-out task's own process group, so neither this host nor other tasks receive it.
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class MiseTimeoutConsole
{
    [DllImport("kernel32.dll")]
    private static extern uint GetConsoleCP();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AllocConsole();

    public static bool EnsureConsole()
    {
        // A console without a window (CREATE_NO_WINDOW) has no GetConsoleWindow, but has a code page.
        return GetConsoleCP() != 0 || AllocConsole();
    }
}
'@

        # Output goes to files, never a pipe: a leaked task would hold a pipe open.
        function script:Invoke-MiseRun([string[]]$Arguments) {
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $mise = Start-Process -FilePath mise -ArgumentList (@('run') + $Arguments) `
                -RedirectStandardOutput out.log -RedirectStandardError err.log -NoNewWindow -PassThru
            $mise.WaitForExit()
            $watch.Stop()
            return [pscustomobject]@{ ExitCode = $mise.ExitCode; Seconds = $watch.Elapsed.TotalSeconds; Ended = Get-Date }
        }

        function script:Get-TaskProcesses([string]$Marker) {
            return @(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like "*$Marker*" })
        }
    }

    BeforeEach {
        Remove-Item -Path * -Force -ErrorAction SilentlyContinue
    }

    AfterEach {
        foreach ($process in (Get-TaskProcesses $TestDrive)) {
            Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'asks a timed-out task to stop with Ctrl+C before terminating it' {
        if (-not [MiseTimeoutConsole]::EnsureConsole()) {
            Set-ItResult -Skipped -Because 'the test host has no console to raise Ctrl+C on'
            return
        }
        # `finally` only runs when the task is interrupted; `taskkill /F` skips it.
        @'
try { Set-Content started.txt 1; Start-Sleep -Seconds 120 } finally { Set-Content stopped.txt 1 }
'@ | Out-File -FilePath slow.ps1 -Encoding utf8NoBOM
        @'
[tasks.slow]
run = "pwsh -NoProfile -File slow.ps1"
timeout = "20s"
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $result = Invoke-MiseRun @('slow')

        $result.ExitCode | Should -Not -Be 0
        Get-Content err.log -Raw | Should -Match 'timed out'
        'started.txt' | Should -Exist -Because 'the task must start before the timeout for this test to mean anything'
        'stopped.txt' | Should -Exist -Because 'the task should have been interrupted, not killed'
        $result.Seconds | Should -BeLessThan 60
    }

    It 'terminates a task that does not exit on Ctrl+C after the grace period' {
        if (-not [MiseTimeoutConsole]::EnsureConsole()) {
            Set-ItResult -Skipped -Because 'the test host has no console to raise Ctrl+C on'
            return
        }
        # cmd.exe answers Ctrl+C in a batch file with "Terminate batch job (Y/N)?" and keeps going.
        @'
@echo off
echo started> started.txt
:loop
ping -n 2 127.0.0.1 >nul
goto loop
'@ | Out-File -FilePath stubborn.cmd -Encoding ascii
        @'
[tasks.stubborn]
run = '.\stubborn.cmd'
timeout = "3s"
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $result = Invoke-MiseRun @('stubborn')

        $result.ExitCode | Should -Not -Be 0
        Get-Content err.log -Raw | Should -Match 'timed out'
        'started.txt' | Should -Exist
        # Measured from the task's start, since mise's own startup varies: at most the 3s timeout
        # when killed at once, more than the 5s grace period when given it.
        $ran = ($result.Ended - (Get-Item started.txt).LastWriteTime).TotalSeconds
        $ran | Should -BeGreaterThan 4.5 -Because 'the task should get the grace period before it is killed'
        Start-Sleep -Seconds 1
        Get-TaskProcesses 'stubborn.cmd' | Should -BeNullOrEmpty -Because 'the batch file must not outlive mise'
    }

    It 'does not interrupt a task that has not timed out' {
        if (-not [MiseTimeoutConsole]::EnsureConsole()) {
            Set-ItResult -Skipped -Because 'the test host has no console to raise Ctrl+C on'
            return
        }
        @'
try { Start-Sleep -Seconds 120 } finally { Set-Content slow-stopped.txt 1 }
'@ | Out-File -FilePath slow.ps1 -Encoding utf8NoBOM
        @'
try { Start-Sleep -Seconds 40; Set-Content steady.txt finished } finally { if (-not (Test-Path steady.txt)) { Set-Content steady.txt interrupted } }
'@ | Out-File -FilePath steady.ps1 -Encoding utf8NoBOM
        @'
[tasks.slow]
run = "pwsh -NoProfile -File slow.ps1"
timeout = "20s"

[tasks.steady]
run = "pwsh -NoProfile -File steady.ps1"
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $result = Invoke-MiseRun @('--continue-on-error', 'slow', ':::', 'steady')

        $result.ExitCode | Should -Not -Be 0
        Get-Content err.log -Raw | Should -Match 'timed out'
        'slow-stopped.txt' | Should -Exist
        Get-Content steady.txt | Should -Be 'finished' -Because 'Ctrl+C must only reach the timed-out task'
    }
}
