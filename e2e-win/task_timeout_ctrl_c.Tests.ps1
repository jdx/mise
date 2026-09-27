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
        # A mise that never exits fails the test instead of holding the job until its limit.
        function script:Invoke-MiseRun([string[]]$Arguments, [int]$LimitSeconds = 120) {
            $watch = [Diagnostics.Stopwatch]::StartNew()
            $mise = Start-Process -FilePath mise -ArgumentList (@('run') + $Arguments) `
                -RedirectStandardOutput out.log -RedirectStandardError err.log -NoNewWindow -PassThru
            if (-not $mise.WaitForExit($LimitSeconds * 1000)) {
                taskkill /F /T /PID $mise.Id 2>&1 | Out-Null
                throw "mise run $Arguments did not exit within $LimitSeconds seconds"
            }
            $watch.Stop()
            return [pscustomobject]@{ ExitCode = $mise.ExitCode; Seconds = $watch.Elapsed.TotalSeconds; Ended = Get-Date }
        }

        function script:Get-TaskProcesses([string]$Marker) {
            return @(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like "*$Marker*" })
        }

        # Tasks start their scripts by full path, so AfterEach finds them by $TestDrive.
        function script:Set-MiseToml {
            process { $_.Replace('<dir>', $TestDrive) | Out-File -FilePath mise.toml -Encoding utf8NoBOM }
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
run = 'pwsh -NoProfile -File "<dir>\slow.ps1"'
timeout = "20s"
'@ | Set-MiseToml

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
run = '"<dir>\stubborn.cmd"'
timeout = "3s"
'@ | Set-MiseToml

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

    It 'stops what a timed-out task started when the task itself exits on Ctrl+C' {
        if (-not [MiseTimeoutConsole]::EnsureConsole()) {
            Set-ItResult -Skipped -Because 'the test host has no console to raise Ctrl+C on'
            return
        }
        # The child ignores Ctrl+C, so only mise can stop it once the task has exited.
        @'
Add-Type -Namespace W -Name K -MemberDefinition '[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(System.IntPtr h, bool add);'
[W.K]::SetConsoleCtrlHandler([System.IntPtr]::Zero, $true) | Out-Null
Set-Content child-started.txt 1
Start-Sleep -Seconds 120
'@ | Out-File -FilePath ignore.ps1 -Encoding utf8NoBOM
        @'
Start-Process pwsh -NoNewWindow -ArgumentList '-NoProfile', '-File', (Join-Path $PWD 'ignore.ps1')
while (-not (Test-Path child-started.txt)) { Start-Sleep -Milliseconds 100 }
try { Start-Sleep -Seconds 120 } finally { Set-Content parent-stopped.txt 1 }
'@ | Out-File -FilePath parent.ps1 -Encoding utf8NoBOM
        @'
[tasks.parent]
run = 'pwsh -NoProfile -File "<dir>\parent.ps1"'
timeout = "20s"
'@ | Set-MiseToml

        $result = Invoke-MiseRun @('parent')

        $result.ExitCode | Should -Not -Be 0
        'child-started.txt' | Should -Exist -Because 'the child must start before the timeout for this test to mean anything'
        'parent-stopped.txt' | Should -Exist -Because 'the task itself should have exited on Ctrl+C'
        $deadline = (Get-Date).AddSeconds(10)
        while ((Get-TaskProcesses 'ignore.ps1').Count -gt 0 -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 200
        }
        Get-TaskProcesses 'ignore.ps1' | Should -BeNullOrEmpty -Because 'what the task started must not outlive it'
    }

    It 'stops an orphaned descendant when a task that ignores Ctrl+C is terminated' {
        if (-not [MiseTimeoutConsole]::EnsureConsole()) {
            Set-ItResult -Skipped -Because 'the test host has no console to raise Ctrl+C on'
            return
        }
        # `cmd /c start` exits at once, so the child's parent is gone and `taskkill /T`
        # cannot reach it once the task is terminated after the grace period.
        @'
Add-Type -Namespace W -Name K -MemberDefinition '[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(System.IntPtr h, bool add);'
[W.K]::SetConsoleCtrlHandler([System.IntPtr]::Zero, $true) | Out-Null
Set-Content orphan-started.txt 1
Start-Sleep -Seconds 120
'@ | Out-File -FilePath orphan.ps1 -Encoding utf8NoBOM
        @'
Add-Type -Namespace W -Name K -MemberDefinition '[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(System.IntPtr h, bool add);'
[W.K]::SetConsoleCtrlHandler([System.IntPtr]::Zero, $true) | Out-Null
cmd /c start "" /b pwsh -NoProfile -File (Join-Path $PWD 'orphan.ps1')
while (-not (Test-Path orphan-started.txt)) { Start-Sleep -Milliseconds 100 }
Start-Sleep -Seconds 120
'@ | Out-File -FilePath stubborn.ps1 -Encoding utf8NoBOM
        @'
[tasks.stubborn]
run = 'pwsh -NoProfile -File "<dir>\stubborn.ps1"'
timeout = "15s"
'@ | Set-MiseToml

        $result = Invoke-MiseRun @('stubborn')

        $result.ExitCode | Should -Not -Be 0
        'orphan-started.txt' | Should -Exist -Because 'the descendant must start before the timeout for this test to mean anything'
        $deadline = (Get-Date).AddSeconds(10)
        while ((Get-TaskProcesses 'orphan.ps1').Count -gt 0 -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 200
        }
        Get-TaskProcesses 'orphan.ps1' | Should -BeNullOrEmpty -Because 'what the task started must not outlive it'
    }

    It 'leaves running what a task that did not time out started' {
        @'
Start-Process pwsh -NoNewWindow -ArgumentList '-NoProfile', '-File', (Join-Path $PWD 'background.ps1')
while (-not (Test-Path background-started.txt)) { Start-Sleep -Milliseconds 100 }
'@ | Out-File -FilePath quick.ps1 -Encoding utf8NoBOM
        @'
Set-Content background-started.txt 1
Start-Sleep -Seconds 60
'@ | Out-File -FilePath background.ps1 -Encoding utf8NoBOM
        @'
[tasks.quick]
run = 'pwsh -NoProfile -File "<dir>\quick.ps1"'
timeout = "60s"
'@ | Set-MiseToml

        $result = Invoke-MiseRun @('quick')

        $result.ExitCode | Should -Be 0
        Start-Sleep -Seconds 2
        Get-TaskProcesses 'background.ps1' | Should -Not -BeNullOrEmpty -Because 'only a timed-out task has what it started stopped'
    }

    It 'does not report a task that exited in time as timed out while its output is drained' {
        # The task exits at once; the ping it starts inherits its output pipe and keeps it open
        # past the timeout.
        @'
[tasks.quick]
run = 'echo started> started.txt && start "" /b ping -n 9 127.0.0.1'
timeout = "3s"
'@ | Set-MiseToml

        $result = Invoke-MiseRun @('--output', 'prefix', 'quick')

        'started.txt' | Should -Exist
        Get-Content out.log -Raw | Should -Match '127\.0\.0\.1' -Because 'the ping must hold the pipe for this test to mean anything'
        Get-Content err.log -Raw | Should -Not -Match 'timed out'
        $result.ExitCode | Should -Be 0
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
run = 'pwsh -NoProfile -File "<dir>\slow.ps1"'
timeout = "20s"

[tasks.steady]
run = 'pwsh -NoProfile -File "<dir>\steady.ps1"'
'@ | Set-MiseToml

        $result = Invoke-MiseRun @('--continue-on-error', 'slow', ':::', 'steady')

        $result.ExitCode | Should -Not -Be 0
        Get-Content err.log -Raw | Should -Match 'timed out'
        'slow-stopped.txt' | Should -Exist
        Get-Content steady.txt | Should -Be 'finished' -Because 'Ctrl+C must only reach the timed-out task'
    }
}
