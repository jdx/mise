Describe 'history watch' {
    BeforeAll {
        $script:OriginalExperimental = [Environment]::GetEnvironmentVariable('MISE_EXPERIMENTAL', 'Process')
        $env:MISE_EXPERIMENTAL = '0'
        $script:OriginalDir = Get-Location
        # what AfterAll counts as started by this file
        $script:Began = Get-Date
        Set-Location TestDrive:

        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $script:OriginalConfigDir = [Environment]::GetEnvironmentVariable('MISE_CONFIG_DIR', 'Process')
        $script:OriginalStateDir = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')
        $env:MISE_CONFIG_DIR = Join-Path $TestDrive 'config'
        $env:MISE_STATE_DIR = Join-Path $TestDrive 'state'
        New-Item -ItemType Directory -Force -Path $env:MISE_CONFIG_DIR | Out-Null
        $script:Tracked = Join-Path $env:MISE_CONFIG_DIR 'tracked'
        New-Item -ItemType Directory -Force -Path $script:Tracked | Out-Null
        'one' | Out-File -FilePath (Join-Path $script:Tracked 'file.txt') -Encoding utf8NoBOM

        # The watcher keeps a hidden or windowless console, so owning one does
        # not tell a service from an interactive watcher; visibility does.
        # AttachConsole joins the target's console, and only a caller owning
        # none may, so the probe runs in a process of its own where giving up
        # a console cannot reach Pester.
        #
        # Under Windows Terminal, GetConsoleWindow returns only a message window;
        # inspect the real desktop window instead. Exit 3: visible. Exit 6: hidden.
        # Exit 5: a valid windowless console. Exit 4: no console.
        # Exit 7: a private Terminal console without windowless-console support.
        $script:Probe = Join-Path $TestDrive 'console-probe.ps1'
        @'
param([int]$TargetPid, [int]$RequireWindowless = 0)
Add-Type -Namespace W -Name C -MemberDefinition @"
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool AttachConsole(uint dwProcessId);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool FreeConsole();
[DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
[DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr GetModuleHandleW(string name);
[DllImport("kernel32.dll", CharSet=CharSet.Ansi, ExactSpelling=true)] public static extern IntPtr GetProcAddress(IntPtr module, string name);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern uint GetConsoleTitle(System.Text.StringBuilder title, int length);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, System.Text.StringBuilder name, int length);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, System.Text.StringBuilder title, int length);
public delegate bool WindowCallback(IntPtr window, IntPtr unused);
[DllImport("user32.dll")] public static extern bool EnumWindows(WindowCallback callback, IntPtr unused);
public static bool Visible(IntPtr window) {
    var name = new System.Text.StringBuilder(128);
    GetClassName(window, name, name.Capacity);
    if (name.ToString() != "PseudoConsoleWindow") return IsWindowVisible(window);
    var title = new System.Text.StringBuilder(2048);
    if (GetConsoleTitle(title, title.Capacity) == 0) throw new InvalidOperationException("Console title unavailable");
    bool visible = false;
    EnumWindows(delegate(IntPtr candidate, IntPtr unused) {
        if (!IsWindowVisible(candidate)) return true;
        var kind = new System.Text.StringBuilder(128);
        GetClassName(candidate, kind, kind.Capacity);
        if (kind.ToString() != "CASCADIA_HOSTING_WINDOW_CLASS") return true;
        var text = new System.Text.StringBuilder(2048);
        GetWindowText(candidate, text, text.Capacity);
        if (text.ToString().Contains(title.ToString())) visible = true;
        return true;
    }, IntPtr.Zero);
    return visible;
}
"@
[W.C]::FreeConsole() | Out-Null
if (-not [W.C]::AttachConsole($TargetPid)) { exit 4 }
$window = [W.C]::GetConsoleWindow()
if ($window -eq [IntPtr]::Zero) { exit 5 }
if ($RequireWindowless) {
    $kind = [System.Text.StringBuilder]::new(128)
    [W.C]::GetClassName($window, $kind, $kind.Capacity) | Out-Null
    if ($kind.ToString() -eq 'PseudoConsoleWindow' -and
        [W.C]::GetProcAddress([W.C]::GetModuleHandleW('kernel32.dll'), 'AllocConsoleWithOptions') -eq [IntPtr]::Zero) {
        exit 7
    }
}
if ([W.C]::Visible($window)) { exit 3 } else { exit 6 }
'@ | Out-File -FilePath $script:Probe -Encoding utf8NoBOM
        $script:PsHost = (Get-Process -Id $PID).Path

        function script:Get-ConsoleProbe([int]$TargetPid, [switch]$RequireWindowless) {
            $probe = Start-Process -FilePath $script:PsHost -PassThru -Wait -ArgumentList @(
                '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $script:Probe, $TargetPid, ([int]$RequireWindowless.IsPresent))
            return $probe.ExitCode
        }

        # The watch lock, not the service declaration, is what `status` reports
        # as `running`, so this says a real watcher has taken over (or let go).
        function script:Wait-Watcher([bool]$Running) {
            $deadline = (Get-Date).AddSeconds(60)
            do {
                $status = mise bootstrap dotfiles status --json | Out-String | ConvertFrom-Json
                if (($status.history.watcher -eq 'running') -eq $Running) { return $true }
                Start-Sleep -Milliseconds 250
            } while ((Get-Date) -lt $deadline)
            return $false
        }

        # A process by pid and start time, so a pid Windows has since handed
        # to something else is never taken for it.
        function script:Get-ProcessRoot([int]$Id) {
            $process = Get-CimInstance Win32_Process -Filter "ProcessId = $Id"
            if ($null -eq $process) { return $null }
            return [pscustomobject]@{ Id = $Id; StartTime = $process.CreationDate }
        }

        # Pester deletes TestDrive as soon as AfterAll returns, and Windows
        # refuses while any process still has its working directory there —
        # which everything started from this file does, down to the `git` a
        # watcher runs. The watch lock going free, which is all Wait-Watcher
        # sees, says neither that the process holding it has exited nor that
        # what it started has. So this ends `$Roots` and every descendant
        # still running, then waits for all of them to exit. An orphan still
        # names its dead parent's pid, so it is found too; start times keep a
        # reused pid, and whatever that new process started, out.
        #
        # The tree is walked again after every kill: a watcher can start a
        # `git` between one walk and the kill, and only a walk that finds
        # nothing running says the tree is gone. Everything found stays
        # known, so a process whose parent has since died is still reached.
        function script:Stop-Tree([object[]]$Roots, [int]$TimeoutSec = 30) {
            $known = @{}
            foreach ($root in $Roots) { if ($null -ne $root) { $known[$root.Id] = $root } }
            $isTree = {
                $member = $known[[int]$_.ProcessId]
                $null -ne $member -and [int]$_.ProcessId -ne $PID -and
                [math]::Abs(($_.CreationDate - $member.StartTime).TotalSeconds) -lt 1
            }
            $deadline = (Get-Date).AddSeconds($TimeoutSec)
            while ($true) {
                $all = @(Get-CimInstance Win32_Process)
                $walked = @{}
                $pending = [System.Collections.Generic.Queue[object]]::new()
                foreach ($member in @($known.Values)) { $pending.Enqueue($member) }
                while ($pending.Count -gt 0) {
                    $node = $pending.Dequeue()
                    if ($walked.ContainsKey($node.Id)) { continue }
                    $walked[$node.Id] = $true
                    if (-not $known.ContainsKey($node.Id)) { $known[$node.Id] = $node }
                    # the pid's next owner, if it has one: what started after
                    # it is that process's, not this one's
                    $next = $all | Where-Object {
                        $_.ProcessId -eq $node.Id -and $_.CreationDate -gt $node.StartTime.AddSeconds(1)
                    } | Select-Object -First 1
                    foreach ($child in $all) {
                        if ($child.ParentProcessId -ne $node.Id) { continue }
                        if ($child.CreationDate -lt $node.StartTime) { continue }
                        if ($next -and $child.CreationDate -ge $next.CreationDate) { continue }
                        $pending.Enqueue([pscustomobject]@{
                                Id = [int]$child.ProcessId; StartTime = $child.CreationDate })
                    }
                }
                $live = @($all | Where-Object $isTree |
                        ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction Ignore })
                if ($live.Count -eq 0) { return $true }
                $remaining = [int][math]::Ceiling(($deadline - (Get-Date)).TotalSeconds)
                if ($remaining -le 0) { break }
                $live | Stop-Process -Force -ErrorAction Ignore
                $live | Wait-Process -Timeout $remaining -ErrorAction Ignore
            }
            foreach ($process in $all | Where-Object $isTree) {
                Write-Warning "still running: $($process.ProcessId) $($process.CommandLine)"
            }
            return $false
        }
    }

    AfterAll {
        # out of TestDrive first, then whatever is still running there:
        # everything this host started since the file began, and anything
        # whose command line names a path inside TestDrive, such as the
        # service launcher Task Scheduler started — a case that failed before
        # its own cleanup may have left either
        Set-Location $script:OriginalDir
        $leaf = Split-Path $TestDrive -Leaf
        $roots = @([pscustomobject]@{ Id = $PID; StartTime = $script:Began }) +
            @(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like "*$leaf*" } |
                ForEach-Object { [pscustomobject]@{ Id = [int]$_.ProcessId; StartTime = $_.CreationDate } })
        Stop-Tree $roots | Out-Null
        foreach ($pair in @(
                @('MISE_EXPERIMENTAL', $script:OriginalExperimental),
                @('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted),
                @('MISE_CONFIG_DIR', $script:OriginalConfigDir),
                @('MISE_STATE_DIR', $script:OriginalStateDir))) {
            if ($null -eq $pair[1]) {
                Remove-Item ("Env:" + $pair[0]) -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
            }
        }
    }

    It 'tracks without experimental opt-in' {
        $env:MISE_EXPERIMENTAL = '0'
        try {
            $output = mise bootstrap dotfiles track $script:Tracked 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $output | Should -Not -Match 'dotfile tracking is experimental'
        } finally {
            $env:MISE_EXPERIMENTAL = '0'
        }
    }

    It 'reconciles once and reports capture health' {
        $tracked = $script:Tracked -replace '\\', '/'
        mise bootstrap dotfiles track $tracked 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        $status = mise bootstrap dotfiles status --json | Out-String | ConvertFrom-Json
        $status.history.watcher | Should -Be 'not-declared'
        $before = @(mise bootstrap dotfiles history --json | Out-String | ConvertFrom-Json).Count

        'two' | Out-File -FilePath (Join-Path $script:Tracked 'file.txt') -Encoding utf8NoBOM
        mise bootstrap dotfiles watch --once 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $entries = @(mise bootstrap dotfiles history --json | Out-String | ConvertFrom-Json)
        $entries.Count | Should -Be ($before + 1)
        $entries[0].trigger | Should -Be 'edit'

        @"
[bootstrap.services.mise-history]
builtin = "history-watch"
"@ | Out-File -FilePath (Join-Path $env:MISE_CONFIG_DIR 'config.toml') -Encoding utf8NoBOM -Append
        $status = mise bootstrap dotfiles status --json | Out-String | ConvertFrom-Json
        $status.history.watcher | Should -Be 'declared-not-running'
    }

    # Task Scheduler starts a console program in a console of its own, so
    # without this the watcher runs behind a terminal window that closing
    # would kill. See jdx/mise#13426.
    It 'hides a console of its own' {
        $tracked = $script:Tracked -replace '\\', '/'
        mise bootstrap dotfiles track $tracked 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $watch = @('bootstrap', 'dotfiles', 'watch')

        # A console of its own, which is what Task Scheduler gives it and what
        # says nobody is reading: the watcher hides or replaces that console.
        $alone = Start-Process -FilePath 'mise' -PassThru -ArgumentList $watch
        $aloneRoot = Get-ProcessRoot $alone.Id
        try {
            Wait-Watcher $true | Should -BeTrue
            $console = Get-ConsoleProbe $alone.Id -RequireWindowless
            @(5, 6, 7) | Should -Contain $console
        } finally {
            # the watcher and any `git` it had running, which killing the
            # watcher alone would leave behind
            $aloneStopped = Stop-Tree @($aloneRoot)
        }
        $aloneStopped | Should -BeTrue
        Wait-Watcher $false | Should -BeTrue
        if ($console -eq 7) {
            Set-ItResult -Skipped -Because 'this Windows version cannot replace a private Terminal console'
        }
    }

    It 'leaves a shared shell console visible' {
        mise bootstrap dotfiles track ($script:Tracked -replace '\\', '/') 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $watch = @('bootstrap', 'dotfiles', 'watch')

        # A separate console with `cmd.exe` waiting for the watcher models a
        # shared interactive shell. Its window must stay up, so a probe that
        # called every console hidden would fail here. Probing `cmd` reaches
        # the watcher's console without finding the watcher's pid.
        #
        # Deliberately not this test host's console: a runner may own a hidden
        # or windowless one, which cannot be a visible positive control.
        # Exit 5 means a valid windowless console; this case requires exit 3.
        $shared = Start-Process -FilePath 'cmd.exe' -PassThru -ArgumentList @(
            '/c', "mise $($watch -join ' ')")
        $sharedRoot = Get-ProcessRoot $shared.Id
        try {
            Wait-Watcher $true | Should -BeTrue
            Get-ConsoleProbe $shared.Id | Should -Be 3
        } finally {
            # `cmd` does not take the watcher with it, so the tree goes together
            $sharedStopped = Stop-Tree @($sharedRoot)
        }
        $sharedStopped | Should -BeTrue
        # the store is left with no watcher holding it, so this file can grow
        # another case without inheriting one
        Wait-Watcher $false | Should -BeTrue
    }

    # A service that sets `environment` used to run through
    # `cmd.exe /c set ... && ...`, and that `cmd.exe` held the console Task
    # Scheduler allocated for as long as the watcher lived — so the window
    # stayed even though the watcher itself had given its console up. mise
    # carries the environment now; launcher and watcher share a hidden or
    # windowless console.
    It 'runs windowless through a task that sets an environment' {
        $env:MISE_EXPERIMENTAL = '1'
        $serviceName = 'mise-history-test-' + [guid]::NewGuid().ToString('N')
        $task = 'mise\' + $serviceName
        $watcherStopped = $false
        $launcherRoot = $null
        $treeStopped = $false
        try {
            # a watcher left over from an earlier case would hold the lock
            # this waits on, and the wait below would say nothing
            Wait-Watcher $false | Should -BeTrue

            # Task Scheduler starts the service with the user's logon
            # environment, not this session's, so without carrying these the
            # watcher would use the real config and state directories and
            # take its lock where Wait-Watcher is not looking. Carrying them
            # is what `environment` is for, and here it is also what makes
            # the watcher reachable at all — so this asserts the environment
            # arrived as much as it asserts the window is gone.
            $cfgDir = $env:MISE_CONFIG_DIR -replace '\\', '\\'
            $stateDir = $env:MISE_STATE_DIR -replace '\\', '\\'
            $trusted = "$TestDrive" -replace '\\', '\\'
            @"
[bootstrap.services.$serviceName]
builtin = "history-watch"
state = "stopped"
enabled = false
environment = { MISE_CONFIG_DIR = "$cfgDir", MISE_STATE_DIR = "$stateDir", MISE_TRUSTED_CONFIG_PATHS = "$trusted", E2E_WATCH_MARK = "1" }
"@ | Out-File -FilePath (Join-Path $env:MISE_CONFIG_DIR 'config.toml') -Encoding utf8NoBOM
            # `track` declares into the same file, which was just rewritten
            mise bootstrap dotfiles track ($script:Tracked -replace '\\', '/') 2>&1 | Out-String | Out-Null
            $LASTEXITCODE | Should -Be 0

            mise bootstrap services apply --yes 2>&1 | Out-String | Out-Null
            $LASTEXITCODE | Should -Be 0
            # The launcher loads its stored environment after startup. Do not let
            # that startup read the user's global config before isolation arrives.
            $registered = Get-ScheduledTask -TaskPath '\mise\' -TaskName $serviceName
            $registered.Actions[0].Arguments = '--no-config ' + $registered.Actions[0].Arguments
            Set-ScheduledTask -TaskPath '\mise\' -TaskName $serviceName -Action $registered.Actions | Out-Null
            Start-ScheduledTask -TaskPath '\mise\' -TaskName $serviceName
            Wait-Watcher $true | Should -BeTrue

            $services = Join-Path $env:MISE_STATE_DIR 'user-services'
            # one launch, named by its digest, and the registered action
            # points at exactly that file
            $launches = @(Get-ChildItem (Join-Path $services ($serviceName + '.launches')) -Filter '*.json')
            $launches.Count | Should -Be 1
            (Get-Content (Join-Path $services ($serviceName + '.xml')) -Raw) |
                Should -BeLike ('*' + $launches[0].Name + '*')
            (Get-Content (Join-Path $services ($serviceName + '.xml')) -Raw) |
                Should -Not -BeLike '*cmd.exe*'

            # the process Task Scheduler started and tracks, and the watcher
            # it started in turn. Its `--launch` is under this state
            # directory, which tells it apart from the launcher of a real
            # service on the same machine: this case ends the one it finds.
            $leaf = Split-Path $TestDrive -Leaf
            $launcher = Get-CimInstance Win32_Process -Filter "Name = 'mise.exe'" |
                Where-Object { $_.CommandLine -like '*__service-exec*' -and $_.CommandLine -like "*$leaf*" } |
                Select-Object -First 1
            $launcher | Should -Not -BeNullOrEmpty
            $launcherRoot = [pscustomobject]@{
                Id = [int]$launcher.ProcessId; StartTime = $launcher.CreationDate }
            $watcher = Get-CimInstance Win32_Process -Filter "Name = 'mise.exe'" |
                Where-Object { $_.ParentProcessId -eq $launcher.ProcessId } |
                Select-Object -First 1
            $watcher | Should -Not -BeNullOrEmpty
            $watcher.CommandLine | Should -BeLike '*dot watch*'

            # The launcher hides or replaces the console Task Scheduler gave
            # it, and the service inherits that hidden or windowless console
            # rather than getting one of its own. A private Terminal console
            # requires AllocConsoleWithOptions; classic consoles do not.
            # The probe is the one the case above shows reports 3 when a
            # window is visible.
            $consoles = @(
                Get-ConsoleProbe ([int]$launcher.ProcessId) -RequireWindowless
                Get-ConsoleProbe ([int]$watcher.ProcessId) -RequireWindowless
            )
            foreach ($console in $consoles) { @(5, 6, 7) | Should -Contain $console }
        } finally {
            schtasks /end /tn $task 2>&1 | Out-Null
            # `/end` asks for the launcher to be terminated and does not wait,
            # and the watcher dies only as the launcher's job closes after it
            $treeStopped = Stop-Tree @($launcherRoot)
            mise bootstrap services remove $serviceName 2>&1 | Out-String | Out-Null
            schtasks /delete /tn $task /f 2>&1 | Out-Null
            $watcherStopped = Wait-Watcher $false
            $env:MISE_EXPERIMENTAL = '0'
        }
        $treeStopped | Should -BeTrue
        $watcherStopped | Should -BeTrue
        if ($consoles -contains 7) {
            Set-ItResult -Skipped -Because 'this Windows version cannot replace a private Terminal console'
        }
    }
}
