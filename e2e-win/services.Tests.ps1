Describe 'bootstrap user services' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        Set-Location TestDrive:

        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $script:Task = 'mise\mise-e2e-sleep'
    }

    AfterAll {
        schtasks /delete /tn $script:Task /f 2>&1 | Out-Null
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'renders and validates a user service without installing it' {
        @"
[bootstrap.services.agent]
scope = "user"
command = "C:\\Tools\\agent.exe --serve"
environment = { RUST_LOG = "info" }

[bootstrap.services.mise-history]
builtin = "history-watch"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        $json = mise bootstrap status --json 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $status = $json | ConvertFrom-Json
        $agent = $status.user_services | Where-Object { $_.name -eq 'agent' }
        $agent.definition | Should -BeLike '*<Command>cmd.exe</Command>*'
        $agent.definition | Should -BeLike '*RUST_LOG=info*'
        $agent.current | Should -Be 'not installed'
        $history = $status.user_services | Where-Object { $_.name -eq 'mise-history' }
        $history.command | Should -BeLike '*dotfiles watch*'
        $history.definition | Should -BeLike '*<LogonTrigger>*'

        @"
[bootstrap.services.docker]
command = "dockerd"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM
        $out = mise bootstrap services status 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*only applies to `scope = "user"` services*'
    }

    It 'installs, runs, and removes a scheduled task' {
        @"
[bootstrap.services.mise-e2e-sleep]
scope = "user"
command = "powershell.exe -NoProfile -Command Start-Sleep 300"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM

        mise bootstrap services apply --yes 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        schtasks /query /tn $script:Task 2>&1 | Out-Null
        $LASTEXITCODE | Should -Be 0
        $json = mise bootstrap services status --json | Out-String
        $LASTEXITCODE | Should -Be 0
        $status = ($json | ConvertFrom-Json)[0]
        $status.current | Should -Be 'running'
        $status.action | Should -Be 'noop'

        @"
[bootstrap.services.mise-e2e-sleep]
scope = "user"
command = "powershell.exe -NoProfile -Command Start-Sleep 300"
state = "absent"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM
        $json = mise bootstrap services status --json | Out-String
        $LASTEXITCODE | Should -Be 0
        $status = ($json | ConvertFrom-Json)[0]
        $status.action | Should -Be 'remove'
        mise bootstrap services apply --yes 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        schtasks /query /tn $script:Task 2>&1 | Out-Null
        $LASTEXITCODE | Should -Not -Be 0

        @"
[bootstrap.services.mise-e2e-sleep]
scope = "user"
command = "powershell.exe -NoProfile -Command Start-Sleep 300"
"@ | Out-File -FilePath mise.toml -Encoding utf8NoBOM
        mise bootstrap services apply --yes 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        "[tools]" | Out-File -FilePath mise.toml -Encoding utf8NoBOM
        $out = mise bootstrap services remove mise-e2e-sleep 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*removed its Scheduled Task*'
        schtasks /query /tn $script:Task 2>&1 | Out-Null
        $LASTEXITCODE | Should -Not -Be 0
    }
}
