Describe 'self-update with a TEMP that is too long' {
    BeforeAll {
        # Saved here and restored in AfterAll: Pester runs every suite in one process, so a
        # TEMP left pointing at $TestDrive would follow the suites that run after this one.
        $script:OriginalTemp = $env:TEMP
        $script:OriginalTmp = $env:TMP

        # Replacing the running binary goes through a helper that self-replace drops in TEMP
        # under a 57-character generated name and then launches. Past 201 characters of TEMP
        # that helper's path exceeds MAX_PATH, Windows cannot launch it, and the failure
        # lands after mise.exe has already left its install directory.
        $script:LongTemp = Join-Path $TestDrive "temp"
        while ($script:LongTemp.Length -lt 203) { $script:LongTemp = $script:LongTemp + "x" }
        New-Item -ItemType Directory -Path $script:LongTemp -Force | Out-Null

        # A version that does not exist. If the guard ever stops working, the run fails while
        # looking up the release instead of replacing the binary, so a regression here cannot
        # destroy the mise these tests are running against.
        $script:MissingVersion = "0.0.0-nonexistent"
        $script:MiseExe = (Get-Command mise).Source
        $script:CurrentVersion = ((& $script:MiseExe --version) -split '\s+')[0] -replace '-DEBUG$', ''
    }

    AfterAll {
        $env:TEMP = $script:OriginalTemp
        $env:TMP = $script:OriginalTmp
    }

    It 'refuses before touching the binary when TEMP is too long' {
        $env:TEMP = $script:LongTemp
        $env:TMP = $script:LongTemp

        $output = mise self-update $script:MissingVersion --force --yes 2>&1 | Out-String

        $LASTEXITCODE | Should -Not -Be 0
        $output | Should -Match "TEMP is too long"
        # The preflight guard stops the update before a release API request.
        $output | Should -Not -Match "404 Not Found"
        Test-Path -LiteralPath $script:MiseExe | Should -BeTrue
    }

    It 'lets an up-to-date check proceed with an ordinary TEMP' {
        $env:TEMP = $script:OriginalTemp
        $env:TMP = $script:OriginalTmp

        # The installed version is a fully offline success path, so a GitHub
        # outage cannot turn a passing TEMP preflight into a test failure.
        $output = mise self-update $script:CurrentVersion --yes --no-plugins 2>&1 | Out-String

        $LASTEXITCODE | Should -Be 0
        $output | Should -Not -Match "TEMP is too long"
        $output | Should -Match "already up to date"
        Test-Path -LiteralPath $script:MiseExe | Should -BeTrue
    }
}
