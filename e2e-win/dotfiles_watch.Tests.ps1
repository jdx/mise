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

        # AttachConsole succeeds only while the target process owns a console,
        # and only from a caller that owns none. The probe therefore runs in a
        # process of its own, where giving up a console cannot reach Pester.
        # Exit 3: the target has a console. Exit 4: it has none.
        $script:Probe = Join-Path $TestDrive 'console-probe.ps1'
        @'
param([int]$TargetPid)
Add-Type -Namespace W -Name C -MemberDefinition @"
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool AttachConsole(uint dwProcessId);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool FreeConsole();
"@
[W.C]::FreeConsole() | Out-Null
if ([W.C]::AttachConsole($TargetPid)) { exit 3 } else { exit 4 }
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
    It 'gives up a console of its own but not a shell it shares' {
        $tracked = $script:Tracked -replace '\\', '/'
        mise bootstrap dotfiles track $tracked 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $watch = @('bootstrap', 'dotfiles', 'watch')

        # A console of its own, which is what Task Scheduler gives it and what
        # says nobody is reading: the watcher gives it up.
        $alone = Start-Process -FilePath 'mise' -PassThru -ArgumentList $watch
        try {
            Wait-Watcher $true | Should -BeTrue
            Get-ConsoleProbe $alone.Id | Should -Be 4
        } finally {
            Stop-Process -Id $alone.Id -Force -ErrorAction Ignore
        }
        Wait-Watcher $false | Should -BeTrue

        # Sharing this test host's console instead, the way a shell waiting on
        # `mise dot watch` does. Somebody is reading, so the console stays —
        # and a probe that cannot see one either way would fail here.
        $shared = Start-Process -FilePath 'mise' -PassThru -NoNewWindow `
            -RedirectStandardOutput (Join-Path $TestDrive 'watch.out') `
            -RedirectStandardError (Join-Path $TestDrive 'watch.err') `
            -ArgumentList $watch
        try {
            Wait-Watcher $true | Should -BeTrue
            Get-ConsoleProbe $shared.Id | Should -Be 3
        } finally {
            Stop-Process -Id $shared.Id -Force -ErrorAction Ignore
        }
        # the store is left with no watcher holding it, so this file can grow
        # another case without inheriting one
        Wait-Watcher $false | Should -BeTrue
    }
}
