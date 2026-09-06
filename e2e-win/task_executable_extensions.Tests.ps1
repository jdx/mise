Describe 'windows_executable_extensions and task execution' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "mise-tasks") | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $script:ConfigPath = Join-Path $script:TestRoot "mise.toml"

        # `printf` rather than `echo` so the assertion can only pass through bash: cmd has no
        # printf, and the pre-fix failure was mise handing this file straight to CreateProcess.
        # Written with LF endings, which is what bash wants for the shebang line.
        [System.IO.File]::WriteAllText(
            (Join-Path $script:TestRoot "mise-tasks\hello.sh"),
            "#!/usr/bin/env bash`nprintf 'ran-via-bash\n'`n")
        "@echo off`r`necho ran-via-cmd`r`n" | ForEach-Object {
            [System.IO.File]::WriteAllText((Join-Path $script:TestRoot "mise-tasks\native.cmd"), $_)
        }

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

    It 'runs a .sh task through its shebang by default' {
        # Control: `sh` is not in the default list, so nothing claims the OS can start the file
        # and the shebang decides.
        "[tools]" | Out-File -Encoding ascii $script:ConfigPath
        (mise run hello | Out-String) | Should -Match 'ran-via-bash'
    }

    It 'still runs it after sh is added to windows_executable_extensions' {
        # The regression. Adding an extension used to make `can_execute_directly` answer yes, so
        # mise passed the script to CreateProcess and it failed with
        # "%1 is not a valid Win32 application" (os error 193) without ever reading the shebang.
        @"
[settings]
windows_executable_extensions = ["exe", "bat", "cmd", "com", "ps1", "vbs", "sh"]
"@ | Out-File -Encoding ascii $script:ConfigPath
        $output = mise run hello 2>&1 | Out-String
        $output | Should -Not -Match 'os error 193'
        $output | Should -Match 'ran-via-bash'
    }

    It 'still starts a .cmd task directly' {
        # The other half: narrowing what the OS list allows must not stop mise launching the
        # extensions it really can launch.
        "[tools]" | Out-File -Encoding ascii $script:ConfigPath
        (mise run native | Out-String) | Should -Match 'ran-via-cmd'
    }
}
