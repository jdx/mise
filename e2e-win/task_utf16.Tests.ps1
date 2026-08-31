Describe 'a task file the Windows shell saved as UTF-16' {
    # Windows PowerShell 5.1 -- the one that ships with Windows -- writes UTF-16LE from a bare `>`
    # and from `Out-File`. `has_shebang` reads the first bytes, so the `#!` in such a file is
    # invisible to it and the file is not a task. That much is deliberate: no interpreter mise
    # dispatches to can run a UTF-16 script, so accepting one would trade silence for a confusing
    # failure at exec time. What was wrong was the advice, which told the user to add the shebang
    # that was already the first line of the file.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        $script:TestRoot = Join-Path $TestDrive 'utf16-task'
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot 'mise-tasks') -Force | Out-Null
        Set-Location $script:TestRoot
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        '' | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM

        # Written by Windows PowerShell 5.1 itself rather than by placing the bytes here: the claim
        # is that the shell shipped with the OS produces this, and a hand-built BOM would not test
        # that claim. `powershell.exe` is the 5.1 host; the `pwsh` running Pester is 7.x, which
        # defaults to UTF-8 and would not reproduce it.
        $script:Utf16Task = Join-Path $script:TestRoot 'mise-tasks\utf16'
        powershell.exe -NoProfile -Command "'#!/usr/bin/env bash' > '$script:Utf16Task'; 'echo UTF16_RAN' >> '$script:Utf16Task'"
        $script:Utf16Wrote = $LASTEXITCODE

        # Control 1: the same script, UTF-8. Only the encoding differs.
        Set-Content -Path 'mise-tasks\utf8' -Value "#!/usr/bin/env bash`necho UTF8_RAN" -Encoding utf8NoBOM

        # Control 2: UTF-8 with no shebang at all -- the case the old advice was written for.
        Set-Content -Path 'mise-tasks\noshebang' -Value "echo NOPE" -Encoding utf8NoBOM
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -ne $script:OriginalTrusted) {
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:OriginalTrusted
        } else {
            Remove-Item Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
    }

    It 'is really UTF-16LE, as Windows PowerShell 5.1 wrote it' {
        # Checked on its own: if the fixture is not what it claims to be, every assertion below is
        # about something else while still looking like a result.
        $script:Utf16Wrote | Should -Be 0
        $bytes = [System.IO.File]::ReadAllBytes($script:Utf16Task)
        $bytes[0..1] | Should -Be @(0xFF, 0xFE)
        # and the shebang really is the first thing in it
        [System.IO.File]::ReadAllText($script:Utf16Task, [Text.Encoding]::Unicode) |
            Should -BeLike '#!/usr/bin/env bash*'
    }

    It 'names the encoding instead of asking for a shebang that is already there' {
        $out = mise run utf16 2>&1 | Out-String
        $out | Should -Match 'UTF-16'
        $out | Should -Not -Match 'Add a shebang line'
    }

    It 'runs the same script saved as UTF-8' {
        # Control 1. Without it, "mise refuses this file" could be about anything in it.
        $out = mise run utf8 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'UTF8_RAN'
    }

    It 'still asks for a shebang when there is genuinely no shebang' {
        # Control 2. Without it an implementation that always blamed the encoding would pass.
        $out = mise run noshebang 2>&1 | Out-String
        $out | Should -Match 'Add a shebang line'
        $out | Should -Not -Match 'UTF-16'
    }
}
