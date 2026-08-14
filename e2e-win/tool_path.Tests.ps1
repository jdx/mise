Describe 'tool-path' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # A directory to point `path:` at. Nothing is installed into it -- `path:` means "the
        # tool is already here", so parsing and resolving the value is the whole of it, and
        # parsing is where this used to fail.
        $script:ToolDir = Join-Path $script:TestRoot "tools\bin"
        New-Item -ItemType Directory -Path $script:ToolDir | Out-Null

        # `mise ls` reports the resolved path with forward slashes, so this is what the native
        # spelling below has to come back as.
        $script:Forward = $script:ToolDir -replace '\\', '/'

        # Saved rather than just cleared afterwards: Pester runs every suite in one process, so
        # removing an inherited value would leave the next file without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'accepts a tool path spelled the way Windows spells it' {
        # Backslashes are what Explorer shows and what `pwd` prints, so this is the spelling a
        # Windows user actually has. It used to be rejected outright:
        #   invalid tool path "C:\\...": contains forbidden character '\'
        #
        # Written as a TOML literal string, which is what takes backslashes as-is. In a basic
        # string they are escapes -- see the doubled form below.
        @"
[tools]
node = { path = '$($script:ToolDir)' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        # Asserting the resolved value, not just that the command survived: the separators are
        # rewritten rather than allowed through, and this is where that shows.
        $out | Should -BeLike "*path:$($script:Forward)*"
    }

    It 'accepts the same path written as a TOML basic string' {
        # The other spelling that actually delivers backslashes to mise, and the one the docs
        # offer as an alternative: doubled inside a double-quoted string. `"C:\tools\node"` is a
        # third thing entirely -- TOML reads `\t` as a tab and the path silently becomes
        # something else -- which is why the docs say which two to use.
        $doubled = $script:ToolDir -replace '\\', '\\'
        @"
[tools]
node = { path = "$doubled" }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike "*path:$($script:Forward)*"
    }

    It 'accepts the same path written with forward slashes' {
        # The workaround people were given while the above was broken. It has to keep working.
        @"
[tools]
node = { path = '$($script:Forward)' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike "*path:$($script:Forward)*"
    }

    It 'still rejects a path containing a quote' {
        # The control for the two above. Only the separator is rewritten; the rest of the list
        # that `path:` values are checked against is untouched, and a quote is on it because the
        # resolved path reaches plugin hooks that build shell commands with it.
        @"
[tools]
node = { path = 'C:/a"b' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*invalid tool path*'
    }

    # `\\?\` and `\\.\` are the one place Windows does not accept `/`, so they are the branch that
    # is deliberately *not* rewritten. Both assert the message rather than only the failure: it is
    # the one that branch produces, so a change that started rewriting them would surface here as
    # the generic "forbidden character" wording instead of as a passing test.
    It 'still rejects an extended-length path, and says why' {
        @"
[tools]
node = { path = '\\?\C:\Users\foo' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*extended-length and device paths*'
    }

    It 'still rejects a device path, and says why' {
        @"
[tools]
node = { path = '\\.\COM1' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*extended-length and device paths*'
    }
}
