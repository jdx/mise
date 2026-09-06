Describe 'tool-path-metacharacters' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved and restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next file without it.
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

    # `path:` values are checked against a denylist because the resolved path reaches vfox plugin
    # hooks that build shell commands with it. The list was written for a POSIX shell; on Windows
    # the shell is cmd.exe, and these are its metacharacters. `%` is the one that matters most:
    # cmd expands `%NAME%` even inside double quotes, so a value like this does not stay literal.
    #
    # Each case is written out rather than shared through a helper: Pester runs an `It` body in
    # its own scope and does not carry a function defined in the `Describe` into it.
    It 'rejects a tool path containing a percent sign' {
        @"
[tools]
node = { path = 'C:/%USERPROFILE%/tool' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*invalid tool path*'
    }

    It 'rejects a tool path containing an ampersand' {
        @"
[tools]
node = { path = 'C:/a&b/tool' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*invalid tool path*'
    }

    It 'rejects a tool path containing a caret' {
        @"
[tools]
node = { path = 'C:/a^b/tool' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*invalid tool path*'
    }

    It 'rejects a tool path containing a pipe' {
        @"
[tools]
node = { path = 'C:/a|b/tool' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*invalid tool path*'
    }

    It 'still accepts an ordinary Windows path' {
        # The control: only the metacharacters are rejected, not forward-slash paths in general.
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "Program Files\tool") -Force | Out-Null
        $ok = ($script:TestRoot -replace '\\', '/') + '/Program Files/tool'
        @"
[tools]
node = { path = '$ok' }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        $out = mise ls --current 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*Program Files/tool*'
    }
}
