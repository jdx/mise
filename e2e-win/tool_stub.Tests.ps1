Describe 'tool-stub' {
    BeforeAll {
        $originalPath = Get-Location
        # The test workflow sets MISE_TRUSTED_CONFIG_PATHS for the whole job, and Pester runs every
        # suite in one process, so dropping it here would take it away from the files that run
        # after this one. Put back what was there instead.
        $originalTrustedConfigPaths = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        # Shared here rather than built up across It blocks, so any one of them can be run alone
        # with `run.ps1 -TestName`.
        @'
#!/usr/bin/env -S mise tool-stub
tool = "aqua:jqlang/jq"
version = "1.7.1"
bin = "jq"
'@ | Out-File -FilePath 'jqstub' -Encoding utf8NoBOM
        mise generate tool-stub jqstub --fetch | Out-Null
    }

    AfterAll {
        Set-Location $originalPath
        if ($null -eq $originalTrustedConfigPaths) {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $originalTrustedConfigPaths, 'Process')
        }
    }

    It 'writes a .cmd launcher beside the stub' {
        # Windows cannot execute the stub itself -- it is a shebang script, which CreateProcess
        # refuses -- so the generator has to leave something Windows will launch.
        mise generate tool-stub plainstub --url 'https://example.com/thing.tar.gz' --skip-download | Out-Null
        Test-Path 'plainstub' | Should -BeTrue
        Test-Path 'plainstub.cmd' | Should -BeTrue
        # %~dpn0, not the stub name: the launcher derives the path from its own filename, which is
        # what keeps a name containing `%` from being mangled by cmd.exe.
        (Get-Content 'plainstub.cmd' -Raw) | Should -BeLike '*mise tool-stub "%~dpn0" %`**'
    }

    It 'omits the launcher for a stub that does not ship for Windows' {
        # The counterpart to the test above: without this one, a launcher written unconditionally
        # would look just as correct.
        mise generate tool-stub unixonly --skip-download `
            --platform-url 'linux-x64:https://example.com/tool-linux.tar.gz' `
            --platform-url 'macos-arm64:https://example.com/tool-macos.tar.gz' | Out-Null
        Test-Path 'unixonly' | Should -BeTrue
        Test-Path 'unixonly.cmd' | Should -BeFalse
    }

    It 'writes the launcher when the stub names a Windows platform' {
        mise generate tool-stub crossplat --skip-download `
            --platform-url 'linux-x64:https://example.com/tool-linux.tar.gz' `
            --platform-url 'windows-x64:https://example.com/tool-windows.zip' | Out-Null
        Test-Path 'crossplat.cmd' | Should -BeTrue
    }

    It 'runs a real tool through the generated launcher' {
        # The point of the whole change: the stub is launchable from PowerShell/cmd. Uses a real
        # tool rather than a dummy URL so this exercises install + execute, not just file creation.
        Test-Path 'jqstub.cmd' | Should -BeTrue
        & '.\jqstub.cmd' --version | Select-Object -Last 1 | Should -BeLike 'jq-1.7.1*'
    }

    It 'gives the same result through mise tool-stub directly' {
        # The explicit invocation already worked before this change; keep it that way.
        mise tool-stub jqstub --version | Select-Object -Last 1 | Should -BeLike 'jq-1.7.1*'
    }

    It 'runs a stub whose name contains a percent sign' {
        # A batch file drops `%x` when x is not a parameter, so a launcher that spelled the stub
        # name out would compute `jqstub` for this file and fail to find it. Deriving the path from
        # the launcher's own filename with %~dpn0 sidesteps that.
        #
        # One `%`, not a `%VAR%` pair: cmd.exe expands pairs in the path it is handed, so a stub
        # named `jq%PATH%stub` cannot be launched at all, whatever its launcher contains.
        @'
#!/usr/bin/env -S mise tool-stub
tool = "aqua:jqlang/jq"
version = "1.7.1"
bin = "jq"
'@ | Out-File -FilePath 'jq%stub' -Encoding utf8NoBOM
        mise generate tool-stub 'jq%stub' --fetch | Out-Null
        Test-Path 'jq%stub.cmd' | Should -BeTrue

        & '.\jq%stub.cmd' --version | Select-Object -Last 1 | Should -BeLike 'jq-1.7.1*'
    }

    It 'removes its own launcher when the stub stops shipping for Windows' {
        mise generate tool-stub transition --skip-download `
            --platform-url 'windows-x64:https://example.com/tool-windows.zip' | Out-Null
        Test-Path 'transition.cmd' | Should -BeTrue

        # Rewrite the same stub as unix-only, then regenerate through the same code path.
        @'
#!/usr/bin/env -S mise tool-stub

[platforms.linux-x64]
url = "https://example.com/tool-linux.tar.gz"
'@ | Out-File -FilePath 'transition' -Encoding utf8NoBOM
        mise generate tool-stub transition --skip-download `
            --platform-url 'linux-x64:https://example.com/tool-linux.tar.gz' | Out-Null

        Test-Path 'transition.cmd' | Should -BeFalse
    }

    It 'leaves a launcher it did not write alone' {
        mise generate tool-stub handwritten --skip-download `
            --platform-url 'linux-x64:https://example.com/tool-linux.tar.gz' | Out-Null
        'echo mine' | Out-File -FilePath 'handwritten.cmd' -Encoding utf8NoBOM

        mise generate tool-stub handwritten --skip-download `
            --platform-url 'linux-x64:https://example.com/tool-linux.tar.gz' | Out-Null

        Test-Path 'handwritten.cmd' | Should -BeTrue
        (Get-Content 'handwritten.cmd' -Raw) | Should -BeLike '*echo mine*'
    }
}
