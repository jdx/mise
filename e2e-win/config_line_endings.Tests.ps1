Describe 'writing a config back keeps its line endings' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $script:Config = Join-Path $script:TestRoot "mise.toml"
        Set-Location $script:TestRoot

        # WriteAllText, not Out-File: the endings under test have to be the ones written here, not
        # whatever a cmdlet decides to append.
        function Set-Config {
            param([string]$Text)
            [IO.File]::WriteAllText($script:Config, $Text)
        }

        # Counted, not matched. A `\r$` pattern check reported every file on this machine as CRLF
        # once; counting the bytes cannot be fooled that way, and it names a mixed file rather than
        # rounding it to one answer.
        function Get-Eol {
            $bytes = [IO.File]::ReadAllBytes($script:Config)
            $cr = @($bytes | Where-Object { $_ -eq 13 }).Count
            $lf = @($bytes | Where-Object { $_ -eq 10 }).Count
            if ($lf -eq 0) { "NONE" }
            elseif ($cr -eq 0) { "LF" }
            elseif ($cr -eq $lf) { "CRLF" }
            else { "MIXED cr=$cr lf=$lf" }
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

    It 'leaves a CRLF config on CRLF' {
        # CRLF is what an editor on this platform writes by default, so this is the ordinary case
        # here rather than an exotic one.
        Set-Config "[env]`r`nA = `"1`"`r`n"
        mise set B=2 | Out-Null
        $LASTEXITCODE | Should -Be 0

        Get-Eol | Should -Be 'CRLF'
        # The edit still has to have happened -- a write that failed would also "keep" the endings.
        (Get-Content $script:Config -Raw) | Should -Match 'B = "2"'
    }

    It 'leaves an LF config on LF' {
        # Control: the fix restores what was there, so it must not push everything to CRLF either.
        Set-Config "[env]`nA = `"1`"`n"
        mise set B=2 | Out-Null
        $LASTEXITCODE | Should -Be 0

        Get-Eol | Should -Be 'LF'
        (Get-Content $script:Config -Raw) | Should -Match 'B = "2"'
    }
}
