Describe 'daemon readiness probes' {
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
        # An argv ready_cmd needs pitchfork 2.30.0, which a release-age cutoff may otherwise skip.
        $script:Pitchfork = 'pitchfork@2.30.0'
        mise install $script:Pitchfork --minimum-release-age 0s 2>&1 | Out-Null

        # The probe passes only with the project's [env], which only `mise x` provides: mise does
        # not write [env] into the pitchfork file.
        @'
if ($env:PROBE_MARK -ne 'from-mise') { exit 1 }
'@ | Out-File -FilePath probe.ps1 -Encoding utf8NoBOM
        @'
while ($true) { Start-Sleep -Seconds 1 }
'@ | Out-File -FilePath server.ps1 -Encoding utf8NoBOM

        # A supervisor started elsewhere lacks this project's [env], as one shared by several
        # projects or worktrees would. Started here, it would inherit [env] from mise instead.
        New-Item -ItemType Directory -Path elsewhere | Out-Null
        Push-Location elsewhere
        mise x $script:Pitchfork -- pitchfork supervisor start 2>&1 | Out-File supervisor.log
        $started = $LASTEXITCODE
        Pop-Location
        $started | Should -Be 0 -Because (Get-Content elsewhere\supervisor.log -Raw)
    }

    AfterEach {
        mise daemons stop 2>&1 | Out-Null
    }

    AfterAll {
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

    It 'runs an argv ready_cmd in the project tool environment' {
        @'
[tools]
pitchfork = "2.30.0"

[env]
PROBE_MARK = "from-mise"

[daemons.server]
run = "pwsh -NoProfile -File server.ps1"
# Fails within the timeout instead of waiting for pitchfork's own limit.
ready_cmd = { run = ["pwsh", "-NoProfile", "-File", "probe.ps1"], timeout = "30s" }
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        mise daemons start server 2>&1 | Out-File start.log
        $LASTEXITCODE | Should -Be 0 -Because ((Get-Content start.log -Tail 5) -join "`n")
    }

    It 'runs a ready_cmd string in the project tool environment' {
        @'
[tools]
pitchfork = "2.30.0"

[env]
PROBE_MARK = "from-mise"

[daemons.server]
run = "pwsh -NoProfile -File server.ps1"
ready_cmd = { run = "pwsh -NoProfile -File probe.ps1", timeout = "30s" }
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        mise daemons start server 2>&1 | Out-File start.log
        $LASTEXITCODE | Should -Be 0 -Because ((Get-Content start.log -Tail 5) -join "`n")
    }
}
