Describe 'multi-line inline task scripts under cmd' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        @'
[tasks.probe]
run = """
echo alpha
echo beta
"""

[tasks.loop]
run = """
for %i in (alpha beta) do echo loop-%i
echo after
"""

[tasks.fails]
run_windows = """
echo one || exit /b 3
exit /b 3
echo never
"""
'@ | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")
        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:OriginalTrusted
    }

    It 'runs every line' {
        $out = (mise run probe 2>$null) -join "`n"
        $out | Should -Match 'alpha'
        $out | Should -Match 'beta'
    }

    It "keeps inline for-loop syntax" {
        $out = (mise run loop 2>$null) -join "`n"
        $out | Should -Match "loop-beta"
        $out | Should -Match "after"
    }

    It 'propagates the exit code of a later line' {
        $out = (mise run fails 2>$null) -join "`n"
        $LASTEXITCODE | Should -Be 3
        $out | Should -Not -Match 'never'
    }
}
