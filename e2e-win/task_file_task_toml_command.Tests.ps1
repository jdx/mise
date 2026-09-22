Describe 'TOML blocks naming a paired Windows file task' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "mise-tasks") | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $script:ConfigPath = Join-Path $script:TestRoot "mise.toml"

        # A POSIX/Windows pair: both files reduce to the task name `build`, so on Windows the
        # native script takes over and answers to the bare stem. That rename is what decides
        # which `[tasks.<name>]` spelling reaches the task, and it only happens here.
        # LF endings for the shebang line, which is what bash wants.
        [System.IO.File]::WriteAllText(
            (Join-Path $script:TestRoot "mise-tasks\build.sh"),
            "#!/usr/bin/env bash`nprintf 'posix-script-ran\n'`n")
        [System.IO.File]::WriteAllText(
            (Join-Path $script:TestRoot "mise-tasks\build.ps1"),
            "Write-Output 'windows-script-ran'`r`n")

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

    It 'runs the native sibling under the bare stem with no block at all' {
        # Control: without this rename the rest of the suite would be asserting about a task
        # name that does not exist.
        "[tools]" | Out-File -Encoding utf8NoBOM $script:ConfigPath
        (mise run build | Out-String) | Should -Match 'windows-script-ran'
    }

    It 'lets a command under the bare stem replace the native sibling' {
        @'
[tasks.build]
run = "echo inline-ran"
'@ | Out-File -Encoding utf8NoBOM $script:ConfigPath
        $output = mise run build 2>&1 | Out-String
        $output | Should -Match 'inline-ran'
        $output | Should -Not -Match 'windows-script-ran'
    }

    It 'lets run_windows replace it too' {
        @'
[tasks.build]
run_windows = "echo windows-inline-ran"
'@ | Out-File -Encoding utf8NoBOM $script:ConfigPath
        $output = mise run build 2>&1 | Out-String
        $output | Should -Match 'windows-inline-ran'
        $output | Should -Not -Match 'windows-script-ran'
    }

    It 'still overlays a block that carries no command' {
        @'
[tasks.build]
description = "overlaid description"
'@ | Out-File -Encoding utf8NoBOM $script:ConfigPath
        (mise run build | Out-String) | Should -Match 'windows-script-ran'
        (mise tasks | Out-String) | Should -Match 'overlaid description'
    }

    It 'leaves the extension spelling as a task of its own' {
        # `build.ps1` is not the task name any more -- the rename took it to `build` -- so a
        # block spelled that way defines a separate task instead of replacing the script.
        @'
[tasks."build.ps1"]
run = "echo extension-spelled-ran"
'@ | Out-File -Encoding utf8NoBOM $script:ConfigPath
        (mise run build | Out-String) | Should -Match 'windows-script-ran'
        (mise run build.ps1 | Out-String) | Should -Match 'extension-spelled-ran'
    }
}
