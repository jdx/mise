Describe 'pypi dependency locks' {
    It 'executes a frozen wheel console script and preserves the pipx alias' {
        $fixture = Join-Path $PSScriptRoot '../e2e/fixtures/pypi/server.py'
        $project = Join-Path $TestDrive 'pypi-lock'
        New-Item -ItemType Directory -Path $project | Out-Null
        Push-Location $project
        $server = $null
        try {
            @'
[tools]
python = "3.12.11"
uv = "0.12.10"
'@ | Set-Content mise.toml
            mise trust
            $LASTEXITCODE | Should -Be 0
            mise install
            $LASTEXITCODE | Should -Be 0
            $python = (mise which python | Out-String).Trim()
            $index = Join-Path $project 'index'
            $server = Start-Process -FilePath $python -ArgumentList @("`"$fixture`"", "`"$index`"") -PassThru -NoNewWindow
            $portFile = Join-Path $index 'port'
            for ($attempt = 0; $attempt -lt 100 -and !(Test-Path $portFile); $attempt++) {
                Start-Sleep -Milliseconds 100
            }
            $port = (Get-Content $portFile).Trim()
            @"
"pypi:mise-lock-cli" = "1.0.0"
[settings]
minimum_release_age = "0"
pypi.registry_url = "http://127.0.0.1:$port/simple/{}/"
"@ | Add-Content mise.toml
            mise lock
            $LASTEXITCODE | Should -Be 0
            (Get-Content mise.lock -Raw) | Should -Match 'path = "\.mise/locks/pipx-mise-lock-cli/1\.0\.0"'
            Test-Path '.mise/locks/pipx-mise-lock-cli/1.0.0/uv.lock' | Should -BeTrue
            mise x --locked -- lock-cli | Should -Be 'dependency=1.0.0'
            $LASTEXITCODE | Should -Be 0
            (mise where pypi:mise-lock-cli) | Should -Be (mise where pipx:mise-lock-cli)
            New-Item -ItemType File -Path (Join-Path $index 'publish') | Out-Null
            mise x --locked -- lock-cli | Should -Be 'dependency=1.0.0'
            mise lock --bump pypi:mise-lock-cli
            $LASTEXITCODE | Should -Be 0
            mise x --locked -- lock-cli | Should -Be 'dependency=2.0.0'
        }
        finally {
            if ($null -ne $server) { Stop-Process -Id $server.Id -ErrorAction SilentlyContinue }
            Pop-Location
        }
    }
}
