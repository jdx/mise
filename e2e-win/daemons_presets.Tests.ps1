Describe 'daemon presets' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:
        $script:OriginalEnv = @{}
        $script:Env = @{
            MISE_TRUSTED_CONFIG_PATHS = "$TestDrive"
            MISE_EXPERIMENTAL = '1'
            # Keeps the per-project daemon state out of the user's own state directory.
            MISE_STATE_DIR = "$TestDrive\mise-state"
            # A supervisor of this test's own, not the one the machine may already run.
            PITCHFORK_STATE_DIR = "$TestDrive\pitchfork-state"
            PITCHFORK_CONFIG_DIR = "$TestDrive\pitchfork-config"
        }
        foreach ($name in $script:Env.Keys) {
            $script:OriginalEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
            [Environment]::SetEnvironmentVariable($name, $script:Env[$name], 'Process')
        }
        # Stopping postgres cleanly on Windows needs a pitchfork that sends it Ctrl+C,
        # which a release-age cutoff may otherwise skip.
        $script:Pitchfork = 'pitchfork@2.29.0'
        mise install $script:Pitchfork --minimum-release-age 0s 2>&1 | Out-Null
    }

    AfterAll {
        mise daemons stop 2>&1 | Out-Null
        mise x $script:Pitchfork -- pitchfork supervisor stop 2>&1 | Out-Null
        Set-Location $script:OriginalDir
        foreach ($name in $script:OriginalEnv.Keys) {
            # Setting a variable to $null leaves it empty rather than removing it.
            if ($null -eq $script:OriginalEnv[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:OriginalEnv[$name], 'Process')
            }
        }
    }

    It 'starts nats under cmd.exe, passes its readiness check, and stops it' {
        # Ports away from the defaults, which another service or a reserved range may hold.
        @'
[tools]
nats-server = "2.15.0"
pitchfork = "2.29.0"

[daemons.events]
preset = "nats"
version = "2"
port = 14222
ports = { monitor_port = 18222 }
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        mise daemons start events 2>&1 | Out-File start.log
        $LASTEXITCODE | Should -Be 0 -Because (Get-Content start.log -Raw)
        curl.exe -fsS -m 10 http://127.0.0.1:18222/healthz | Should -Match '"status":"ok"'
        (mise env --json | ConvertFrom-Json).NATS_URL | Should -Be 'nats://127.0.0.1:14222'

        mise daemons stop events 2>&1 | Out-File stop.log
        $LASTEXITCODE | Should -Be 0 -Because (Get-Content stop.log -Raw)
        $deadline = (Get-Date).AddSeconds(15)
        do {
            $left = @(Get-CimInstance Win32_Process -Filter "Name='nats-server.exe'" |
                Where-Object { $_.CommandLine -like "*14222*" })
            if ($left.Count -eq 0) { break }
            Start-Sleep -Milliseconds 250
        } while ((Get-Date) -lt $deadline)
        $left | Should -BeNullOrEmpty -Because 'the server should stop with its daemon'
    }

    It 'initializes postgres, serves the requested database, and shuts it down cleanly' {
        # PostgreSQL refuses to run with administrative rights, which CI runners have.
        $identity = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
        if ($identity.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            Set-ItResult -Skipped -Because 'PostgreSQL does not run with administrative rights'
            return
        }
        # The supervisor started for nats runs this too, so postgres reaches the
        # initialization only through the PATH `mise x` builds.
        @'
[tools]
postgres = "18.6"
pitchfork = "2.29.0"

[daemons.db]
preset = "postgres"
version = "18"
port = 15432
options = { database = "app" }
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        mise daemons start db 2>&1 | Out-File start.log
        $LASTEXITCODE | Should -Be 0 -Because (Get-Content start.log -Raw)
        mise x -- psql -h 127.0.0.1 -p 15432 -U postgres -d app -tAc 'select current_database()' |
            Should -Be 'app'

        mise daemons stop db 2>&1 | Out-File stop.log
        $LASTEXITCODE | Should -Be 0 -Because (Get-Content stop.log -Raw)
        $data = Get-ChildItem "$TestDrive\mise-state\daemons" -Recurse -Directory -Filter db |
            Where-Object { Test-Path (Join-Path $_.FullName 'PG_VERSION') } |
            Select-Object -First 1
        $data | Should -Not -BeNullOrEmpty
        # A server terminated instead of stopped leaves the cluster "in production". English
        # messages, since a translated build would word both the label and the state differently.
        $messages = $env:LC_MESSAGES
        $env:LC_MESSAGES = 'C'
        try {
            mise x -- pg_controldata $data.FullName | Select-String 'cluster state' |
                Should -Match 'shut down'
        } finally {
            if ($null -eq $messages) { Remove-Item Env:\LC_MESSAGES -ErrorAction SilentlyContinue } else { $env:LC_MESSAGES = $messages }
        }
    }
}
