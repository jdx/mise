Describe 'mise env --shell pwsh quoting' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        Set-Location $script:TestRoot

        # A unit test can only check the text mise emits. This runs it, which is the thing that
        # was broken: an apostrophe closed the literal and PowerShell refused to parse the line.
        function Invoke-MiseEnv {
            $script = Join-Path $script:TestRoot "env.ps1"
            $body = mise env --shell pwsh | Out-String
            $probe = 'Write-Output ("QUOTED=[" + ${Env:QUOTED} + "] PLAIN=[" + ${Env:PLAIN} + "]")'
            [System.IO.File]::WriteAllText($script, $body + "`n" + $probe + "`n")
            pwsh -NoProfile -File $script 2>&1 | Out-String
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

    It 'emits a value containing an apostrophe that PowerShell can parse' {
        # `O'Brien` is an ordinary Windows username, so this reaches anyone whose home directory
        # carries one -- every `hook-env` during `mise activate pwsh`.
        @"
[env]
QUOTED = "C:\\Users\\O'Brien\\tools"
PLAIN = "no quote here"
"@ | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        $output = Invoke-MiseEnv
        $output | Should -Not -BeLike '*ParserError*'
        # round-tripped exactly, not merely parsed
        $output.Contains("QUOTED=[C:\Users\O'Brien\tools]") | Should -BeTrue
        $output.Contains("PLAIN=[no quote here]") | Should -BeTrue
    }

    It 'still emits a value with no apostrophe correctly' {
        # Control: pins the fixture, so the test above cannot pass by emitting nothing at all.
        @"
[env]
QUOTED = "plain value"
PLAIN = "no quote here"
"@ | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        $output = Invoke-MiseEnv
        $output | Should -Not -BeLike '*ParserError*'
        $output.Contains("QUOTED=[plain value]") | Should -BeTrue
    }

    It 'emits an env name containing a space that PowerShell can parse' {
        # `$Env:MY VAR=...` is a parse error; the braced form is what accepts it.
        @"
[env]
"MY VAR" = "spaced"
"@ | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        $script = Join-Path $script:TestRoot "env2.ps1"
        $body = mise env --shell pwsh | Out-String
        $probe = 'Write-Output ("SPACED=[" + ${Env:MY VAR} + "]")'
        [System.IO.File]::WriteAllText($script, $body + "`n" + $probe + "`n")
        $output = pwsh -NoProfile -File $script 2>&1 | Out-String

        $output | Should -Not -BeLike '*ParserError*'
        $output.Contains("SPACED=[spaced]") | Should -BeTrue
    }
}
