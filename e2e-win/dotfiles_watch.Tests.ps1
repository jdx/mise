Describe 'history watch' {
    BeforeAll {
        $script:OriginalExperimental = [Environment]::GetEnvironmentVariable('MISE_EXPERIMENTAL', 'Process')
        $env:MISE_EXPERIMENTAL = '0'
        $script:OriginalDir = Get-Location
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

        # The watcher keeps its console and hides the window, so owning one
        # no longer tells the two apart — whether the window is visible does.
        # AttachConsole joins the target's console, and only a caller owning
        # none may, so the probe runs in a process of its own where giving up
        # a console cannot reach Pester. Exit 3: the window is visible. Exit 6:
        # hidden. Exit 4: no console. Exit 5: a console with no window, which
        # no case here expects.
        $script:Probe = Join-Path $TestDrive 'console-probe.ps1'
        @'
param([int]$TargetPid)
Add-Type -Namespace W -Name C -MemberDefinition @"
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool AttachConsole(uint dwProcessId);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool FreeConsole();
[DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
"@
[W.C]::FreeConsole() | Out-Null
if (-not [W.C]::AttachConsole($TargetPid)) { exit 4 }
$window = [W.C]::GetConsoleWindow()
if ($window -eq [IntPtr]::Zero) { exit 5 }
if ([W.C]::IsWindowVisible($window)) { exit 3 } else { exit 6 }
'@ | Out-File -FilePath $script:Probe -Encoding utf8NoBOM
        $script:PsHost = (Get-Process -Id $PID).Path

        function script:Get-ConsoleProbe([int]$TargetPid) {
            $probe = Start-Process -FilePath $script:PsHost -PassThru -Wait -ArgumentList @(
                '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $script:Probe, $TargetPid)
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
    }

    AfterAll {
        Set-Location $script:OriginalDir
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
    It 'hides a console of its own but not a shell it shares' {
        $tracked = $script:Tracked -replace '\\', '/'
        mise bootstrap dotfiles track $tracked 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $watch = @('bootstrap', 'dotfiles', 'watch')

        # A console of its own, which is what Task Scheduler gives it and what
        # says nobody is reading: the watcher hides that window. Exit 5 here
        # would mean `Start-Process` stopped giving it a real console window,
        # leaving nothing for this case to be about.
        $alone = Start-Process -FilePath 'mise' -PassThru -ArgumentList $watch
        try {
            Wait-Watcher $true | Should -BeTrue
            Get-ConsoleProbe $alone.Id | Should -Be 6
        } finally {
            Stop-Process -Id $alone.Id -Force -ErrorAction Ignore
        }
        Wait-Watcher $false | Should -BeTrue

        # A console of its own again, but with `cmd.exe` in it too — which is
        # how a service that sets `environment` runs, and what says somebody is
        # reading. The window must stay up, and a probe that called every
        # console hidden would fail here. Probing `cmd` reaches the console the
        # watcher is in, so its own pid never has to be found.
        #
        # Deliberately not this test host's console: on a runner that is a
        # pseudoconsole there is no window there to call visible, which is exit
        # 5 rather than 3.
        $shared = Start-Process -FilePath 'cmd.exe' -PassThru -ArgumentList @(
            '/c', "mise $($watch -join ' ')")
        try {
            Wait-Watcher $true | Should -BeTrue
            Get-ConsoleProbe $shared.Id | Should -Be 3
        } finally {
            # `cmd` does not take the watcher with it, so the tree goes together
            taskkill.exe /T /F /PID $shared.Id 2>&1 | Out-Null
        }
        # the store is left with no watcher holding it, so this file can grow
        # another case without inheriting one
        Wait-Watcher $false | Should -BeTrue
    }

    # A service that sets `environment` used to run through
    # `cmd.exe /c set ... && ...`, and that `cmd.exe` held the console Task
    # Scheduler allocated for as long as the watcher lived — so the window
    # stayed even though the watcher itself had given its console up. mise
    # carries the environment now, and nothing is left attached.
    It 'runs windowless through a task that sets an environment' {
        $env:MISE_EXPERIMENTAL = '1'
        $task = 'mise\mise-history'
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
[bootstrap.services.mise-history]
builtin = "history-watch"
environment = { MISE_CONFIG_DIR = "$cfgDir", MISE_STATE_DIR = "$stateDir", MISE_TRUSTED_CONFIG_PATHS = "$trusted", E2E_WATCH_MARK = "1" }
"@ | Out-File -FilePath (Join-Path $env:MISE_CONFIG_DIR 'config.toml') -Encoding utf8NoBOM
            # `track` declares into the same file, which was just rewritten
            mise bootstrap dotfiles track ($script:Tracked -replace '\\', '/') 2>&1 | Out-String | Out-Null
            $LASTEXITCODE | Should -Be 0

            mise bootstrap services apply --yes 2>&1 | Out-String | Out-Null
            $LASTEXITCODE | Should -Be 0
            Wait-Watcher $true | Should -BeTrue

            $services = Join-Path $env:MISE_STATE_DIR 'user-services'
            # one launch, named by its digest, and the registered action
            # points at exactly that file
            $launches = @(Get-ChildItem (Join-Path $services 'mise-history.launches') -Filter '*.json')
            $launches.Count | Should -Be 1
            (Get-Content (Join-Path $services 'mise-history.xml') -Raw) |
                Should -BeLike ('*' + $launches[0].Name + '*')
            (Get-Content (Join-Path $services 'mise-history.xml') -Raw) |
                Should -Not -BeLike '*cmd.exe*'

            # the process Task Scheduler started and tracks, and the watcher
            # it started in turn
            $launcher = Get-CimInstance Win32_Process -Filter "Name = 'mise.exe'" |
                Where-Object { $_.CommandLine -like '*__service-exec*' } |
                Select-Object -First 1
            $launcher | Should -Not -BeNullOrEmpty
            $watcher = Get-CimInstance Win32_Process -Filter "Name = 'mise.exe'" |
                Where-Object { $_.ParentProcessId -eq $launcher.ProcessId } |
                Select-Object -First 1
            $watcher | Should -Not -BeNullOrEmpty
            $watcher.CommandLine | Should -BeLike '*dot watch*'

            # Neither puts a window on the desktop: the launcher hides the
            # console Task Scheduler gave it, and the service inherits that
            # same hidden console rather than being handed one of its own.
            # The probe is the one the case above shows reports 3 when a
            # window is visible.
            Get-ConsoleProbe ([int]$launcher.ProcessId) | Should -Be 6
            Get-ConsoleProbe ([int]$watcher.ProcessId) | Should -Be 6
        } finally {
            schtasks /end /tn $task 2>&1 | Out-Null
            mise bootstrap services remove mise-history 2>&1 | Out-String | Out-Null
            schtasks /delete /tn $task /f 2>&1 | Out-Null
            Wait-Watcher $false | Out-Null
            $env:MISE_EXPERIMENTAL = '0'
        }
    }
}
