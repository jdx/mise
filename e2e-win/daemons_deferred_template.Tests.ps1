Describe 'daemon commands rendered by mise x' {
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
        # pitchfork leaves a command with mise-only variables to `mise x` from 2.30.0, which a
        # release-age cutoff may otherwise skip.
        $script:Pitchfork = 'pitchfork@2.30.0'
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

    It 'quotes [vars] values for cmd.exe' {
        # The command records what it received, then keeps running like a server would. The
        # rename keeps the test from reading received.json while it is still being written.
        @'
$args | ConvertTo-Json -Compress | Set-Content -Path received.json.tmp
Move-Item -Path received.json.tmp -Destination received.json
while ($true) { Start-Sleep -Seconds 1 }
'@ | Out-File -FilePath show.ps1 -Encoding utf8NoBOM
        @'
[tools]
pitchfork = "2.30.0"

[vars]
title = "my title"
symbols = "50% & more"

[daemons.show]
run = "pwsh -NoProfile -File show.ps1 {{ vars.title | quote }} {{ vars.symbols | quote }}"
'@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        mise daemons start show 2>&1 | Out-File start.log
        $LASTEXITCODE | Should -Be 0 -Because (Get-Content start.log -Raw)

        $deadline = (Get-Date).AddSeconds(60)
        while (-not (Test-Path received.json) -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 500
        }
        'received.json' | Should -Exist -Because 'the daemon should start'
        Get-Content received.json -Raw | ConvertFrom-Json | Should -Be @('my title', '50% & more')
    }
}
