Describe 'a linked version whose target is gone' {
    # `mise link` writes a junction on Windows, and a junction whose target is gone is removed by a
    # different system call than a unix symlink (`remove_file` refuses it outright). Passing on
    # Linux says nothing about here, so the same behaviour is checked on both.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        Set-Location $script:TestRoot

        # `mise link` does not need the tool installed -- it points a version name at a directory
        # of the user's, which is exactly the arrangement that breaks when that directory moves.
        $script:Target = Join-Path $script:TestRoot 'vanishing'
        New-Item -ItemType Directory -Path $script:Target | Out-Null
        mise link node@e2e-broken $script:Target | Out-Null
        $script:LinkExit = $LASTEXITCODE
        # Captured on a line of its own: a failed `mise where` would otherwise hand `Split-Path`
        # an empty string, and every path assertion below would be about the wrong place while
        # still looking like a result.
        $where = mise where node@e2e-broken
        $script:WhereExit = $LASTEXITCODE
        $script:Entry = Join-Path (Split-Path $where) 'e2e-broken'

        # Whether the entry is there, asked without resolving it -- the same question
        # `file::entry_exists` asks on the Rust side, and the one this whole change is about.
        #
        # `Test-Path` measured True for a dangling junction here, so it would work; enumerating the
        # parent does not depend on that at all. It matters because the assertion after the
        # uninstall is only meaningful if the check is True while the entry dangles: a check that
        # resolved the target would be False either way and would pass whether or not anything was
        # removed.
        function script:EntryPresent([string]$Path) {
            $name = Split-Path $Path -Leaf
            [bool](Get-ChildItem -LiteralPath (Split-Path $Path) -Force -ErrorAction SilentlyContinue |
                    Where-Object { $_.Name -eq $name })
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        mise uninstall node@e2e-broken 2>&1 | Out-Null
        mise uninstall node@e2e-kept 2>&1 | Out-Null
    }

    It 'is an ordinary linked version while its target is there' {
        # The control. Everything below is about what changes when the target goes away, so the
        # starting state has to be the working one.
        $script:LinkExit | Should -Be 0
        $script:WhereExit | Should -Be 0
        script:EntryPresent $script:Entry | Should -BeTrue

        $out = mise ls node | Out-String
        $status = $LASTEXITCODE
        $out | Should -BeLike '*e2e-broken (symlink)*'
        $status | Should -Be 0
    }

    It 'is reported as broken once the target is gone' {
        Remove-Item -LiteralPath $script:Target -Recurse -Force
        # The entry is still there. This is the control for the assertion after the uninstall:
        # a check that resolved the link would be False here too, and that later assertion would
        # then pass whether or not anything had been removed.
        script:EntryPresent $script:Entry | Should -BeTrue

        $out = mise ls node | Out-String
        $status = $LASTEXITCODE
        $out | Should -BeLike '*e2e-broken (broken symlink)*'
        # A listing that named it and still failed would not be the fix this claims to be.
        $status | Should -Be 0
    }

    It 'is not counted as installed' {
        # `mise link` already makes this call by keeping the incomplete marker for a dangling link.
        # Listing it is about making it findable, not about claiming it works.
        $json = mise ls --json node | Out-String
        $jsonStatus = $LASTEXITCODE
        ($json | jq -r '.[] | select(.version == "e2e-broken") | "\(.broken) \(.installed)"') |
            Should -Be 'true false'
        $jsonStatus | Should -Be 0

        $out = mise ls -i node | Out-String
        $status = $LASTEXITCODE
        $out | Should -Not -BeLike '*e2e-broken*'
        # Without this, a `mise ls -i` that failed outright would satisfy the line above.
        $status | Should -Be 0
    }

    It 'can be uninstalled, which is what frees the name' {
        mise uninstall node@e2e-broken 2>&1 | Out-Null
        $LASTEXITCODE | Should -Be 0
        script:EntryPresent $script:Entry | Should -BeFalse

        $out = mise ls node | Out-String
        $status = $LASTEXITCODE
        $out | Should -Not -BeLike '*e2e-broken*'
        $status | Should -Be 0
    }

    It 'still removes only the link when the target is alive' {
        # The regression control: the directory a live link points at belongs to the user, and
        # uninstalling the version must not take it.
        $kept = Join-Path $script:TestRoot 'kept'
        New-Item -ItemType Directory -Path $kept | Out-Null
        Set-Content -LiteralPath (Join-Path $kept 'sentinel.txt') -Value 'important'
        mise link node@e2e-kept $kept | Out-Null
        $LASTEXITCODE | Should -Be 0
        $where = mise where node@e2e-kept
        $LASTEXITCODE | Should -Be 0
        $entry = Join-Path (Split-Path $where) 'e2e-kept'
        script:EntryPresent $entry | Should -BeTrue

        mise uninstall node@e2e-kept 2>&1 | Out-Null
        $LASTEXITCODE | Should -Be 0
        script:EntryPresent $entry | Should -BeFalse
        # A plain file, so `Test-Path` is exactly the right question for it.
        Test-Path -LiteralPath (Join-Path $kept 'sentinel.txt') | Should -BeTrue
    }
}
