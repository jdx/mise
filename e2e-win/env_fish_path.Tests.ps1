Describe 'fish PATH on Windows' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        # `_.path` is what puts a drive-lettered directory in front of PATH. `FOO` is the control:
        # it holds a `;` too, but it is not PATH, so nothing may split it.
        @(
            '[env]'
            '_.path = ["C:/aaa/bin"]'
            'FOO = "a;b"'
        ) | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        Set-Location $script:TestRoot

        function Get-FishLine {
            param([string]$Pattern)
            (mise env -s fish | Select-String -Pattern $Pattern | Select-Object -First 1).Line
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

    It 'gives fish one list element per directory' {
        # The reproduction. This printed `set -gx PATH C '/aaa/bin;C' '\Users\...;C' ...`: fish
        # keeps PATH as a list, and the value was split on `:`, so every drive letter became an
        # element of its own and the rest of that path was glued to the next one. A `;` surviving
        # anywhere on this line means the split did not happen on the host's separator.
        $line = Get-FishLine '^set -gx PATH '
        $line | Should -Not -BeNullOrEmpty
        $line | Should -Not -Match ';'
    }

    It 'keeps a declared directory whole' {
        # Asserted separately from the line above: an empty PATH would also contain no `;`.
        $line = Get-FishLine '^set -gx PATH '
        $line | Should -Match "'C:[\\/]aaa[\\/]bin'"
    }

    It 'leaves a semicolon in any other variable alone' {
        # Control: only PATH is a list. Without this, splitting too much would look like a fix.
        $line = Get-FishLine '^set -gx FOO '
        $line | Should -Match "set -gx FOO 'a;b'"
    }

    It 'still joins PATH into one value for pwsh' {
        # Control: pwsh takes PATH as a single separated string, which was already right and has
        # to stay that way. It is also what shows this suite is reading real output -- the two
        # PATH assertions above would pass just as well against nothing at all.
        $line = (mise env -s pwsh | Select-String -Pattern 'Env:PATH' | Select-Object -First 1).Line
        $line | Should -Not -BeNullOrEmpty
        $line | Should -Match ';'
    }
}
