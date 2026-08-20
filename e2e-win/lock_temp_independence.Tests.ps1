Describe 'mise lock does not depend on TEMP' {
    BeforeAll {
        # Saved here and restored in AfterAll: Pester runs every suite in one process, so a
        # TEMP left pointing at $TestDrive would follow the suites that run after this one.
        $script:OriginalTemp = $env:TEMP
        $script:OriginalTmp = $env:TMP

        # Long enough that the lock-time artifact download would land past MAX_PATH, since it
        # goes to <scratch>\.tmpXXXXXX\<artifact filename>. When that scratch was TEMP, the
        # download failed here and `provenance` for the current platform was silently dropped
        # from the generated lockfile — so the file a machine committed depended on how deep
        # its TEMP happened to be.
        $script:LongTemp = Join-Path $TestDrive "temp"
        while ($script:LongTemp.Length -lt 230) { $script:LongTemp = $script:LongTemp + "x" }
        New-Item -ItemType Directory -Path $script:LongTemp -Force | Out-Null
    }

    AfterAll {
        $env:TEMP = $script:OriginalTemp
        $env:TMP = $script:OriginalTmp
    }

    It 'writes the same lockfile whatever TEMP is' {
        $lockfiles = @{}
        foreach ($case in @('control', 'long')) {
            $proj = Join-Path $TestDrive $case
            New-Item -ItemType Directory -Path $proj -Force | Out-Null
            Set-Content -LiteralPath (Join-Path $proj "mise.toml") -Value "[tools]`njq = `"1.8.2`""

            if ($case -eq 'long') {
                $env:TEMP = $script:LongTemp
                $env:TMP = $script:LongTemp
            } else {
                $env:TEMP = $script:OriginalTemp
                $env:TMP = $script:OriginalTmp
            }

            Push-Location $proj
            try {
                mise lock | Out-Null
                $LASTEXITCODE | Should -Be 0
            } finally {
                Pop-Location
            }

            $lockfiles[$case] = Get-Content -LiteralPath (Join-Path $proj "mise.lock") -Raw
        }

        # The control half also guards the assertion itself: if lock stopped recording
        # provenance at all, both files would still match and the comparison would say
        # nothing, so check the control actually has something to lose.
        $lockfiles['control'] | Should -Match 'provenance'
        $lockfiles['long'] | Should -Be $lockfiles['control']
    }
}
