Describe 'a download whose destination is past MAX_PATH' {
    # Every downloaded file lands through `tempfile`, whose persist does not get the
    # extended-length path handling `std::fs` applies -- `file::PreparedAtomicWrite::commit`
    # records the measurement: it breaks at a 253-character target while `fs::rename` on the same
    # tree succeeds at 415. So a long `MISE_DATA_DIR` fails the download, and it used to fail as a
    # bare `os error 3`: "the specified path cannot be found", for a directory the user had just
    # configured and which was right there.
    #
    # This does not make long paths work. It makes the failure say what it is.

    BeforeAll {
        $script:OriginalDir = Get-Location
        # All three, every time. A probe that isolated only some of these once wrote junctions into
        # the real installs directory.
        $script:Saved = @{}
        foreach ($v in 'MISE_DATA_DIR', 'MISE_CONFIG_DIR', 'MISE_CACHE_DIR', 'MISE_TRUSTED_CONFIG_PATHS') {
            $script:Saved[$v] = [Environment]::GetEnvironmentVariable($v, 'Process')
        }

        $script:Root = Join-Path $TestDrive 'longpath'
        New-Item -ItemType Directory -Path $script:Root -Force | Out-Null

        # Past MAX_PATH, so it has to be created with the extended-length prefix.
        $script:LongData = Join-Path $script:Root ('d' * 40)
        while ($script:LongData.Length -lt 300) { $script:LongData = Join-Path $script:LongData ('d' * 40) }
        [System.IO.Directory]::CreateDirectory("\\?\$($script:LongData)") | Out-Null
        $script:ShortData = Join-Path $script:Root 'short'
        New-Item -ItemType Directory -Path $script:ShortData -Force | Out-Null

        function script:InstallWith([string]$dataDir, [string]$tag) {
            $cfg = Join-Path $script:Root "cfg-$tag"
            $cache = Join-Path $script:Root "cache-$tag"
            $proj = Join-Path $script:Root "proj-$tag"
            New-Item -ItemType Directory -Path $cfg, $cache, $proj -Force | Out-Null
            $env:MISE_DATA_DIR = $dataDir
            $env:MISE_CONFIG_DIR = $cfg
            $env:MISE_CACHE_DIR = $cache
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:Root
            Set-Location $proj
            '' | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM
            $out = mise install jq@1.8.2 2>&1 | Out-String
            return [pscustomobject]@{ Out = $out; Exit = $LASTEXITCODE }
        }

        $script:Long = script:InstallWith $script:LongData 'long'
        $script:Short = script:InstallWith $script:ShortData 'short'
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($v in $script:Saved.Keys) {
            if ($null -ne $script:Saved[$v]) { Set-Item "Env:\$v" $script:Saved[$v] }
            else { Remove-Item "Env:\$v" -ErrorAction SilentlyContinue }
        }
        # A tree past MAX_PATH cannot be removed through the ordinary API, and Pester hands the
        # drive back by walking it -- so a tree left here does not stay a local problem: the run
        # fails inside the framework afterwards, with every test in this file green. Swallowing
        # the failure would remove the one line that explains that, so it is reported. Not
        # rethrown: the tests themselves passed, and a throw here would attribute the framework's
        # later failure to this container instead of leaving the warning next to it.
        if (Test-Path -LiteralPath $script:LongData) {
            try {
                [System.IO.Directory]::Delete("\\?\$($script:Root)", $true)
            } catch {
                $why = $_.Exception.Message
                Write-Warning "could not remove the long-path fixture at $($script:Root): $why"
                Write-Warning ('Pester walks TestDrive to hand it back, so the run may fail ' +
                    'after this file with every test in it green.')
            }
        }
    }

    It 'is actually testing the two lengths it claims to' {
        # Checked on its own so a fixture that stopped being long fails here, saying so, rather
        # than making the assertions below pass or fail for a reason nobody can see.
        $downloadDest = Join-Path $script:LongData 'downloads\jq\1.8.2\jq-windows-amd64.exe'
        $downloadDest.Length | Should -BeGreaterThan 260
        (Join-Path $script:ShortData 'downloads\jq\1.8.2\jq-windows-amd64.exe').Length |
            Should -BeLessThan 260
    }

    It 'still installs when the path is short' {
        # The control. Without it "the long one failed" says nothing about the length.
        $script:Short.Exit | Should -Be 0
    }

    It 'fails on the long one, because tempfile cannot write there' {
        $script:Long.Exit | Should -Not -Be 0
    }

    It 'says the path length is what went wrong' {
        # The old failure was `os error 3` and nothing else -- "the specified path cannot be
        # found", about a directory that was right there.
        $script:Long.Out | Should -Match '260'
        $script:Long.Out | Should -Match 'characters'
        $script:Long.Out | Should -Match 'shorter'
    }

    It 'names the operation that failed' {
        # Without this the message could be attached to any step and still match above.
        $script:Long.Out | Should -Match 'download'
    }
}
