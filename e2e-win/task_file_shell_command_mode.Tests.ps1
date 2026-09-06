Describe 'file tasks whose shell is in -c mode' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "mise-tasks") | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        "[tools]" | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        function Write-Task {
            param([string]$Name, [string]$Body)
            $path = Join-Path $script:TestRoot "mise-tasks\$Name"
            # LF, because this file's own line endings are CRLF and a stray `r would ride along
            # into the script and reach bash as part of a command.
            $lf = $Body -replace "`r`n", "`n"
            [System.IO.File]::WriteAllText($path, $lf, (New-Object System.Text.UTF8Encoding $false))
        }

        # A file task hands its shell a script *path*. `bash -c` reads what follows as a command
        # string, so here the path is the command -- and on Windows its backslashes are eaten as
        # escapes before it is even looked up, leaving `command not found` and exit 127.
        Write-Task -Name "bash_c" -Body @'
#!/usr/bin/env bash
#MISE shell="bash -c"
printf 'bash -c: [%s] [%s]\n' "$1" "$2"
'@

        # Control: the same task without the override, taking its shell from the shebang. It has
        # always worked, and pins that the fix left that shape alone.
        Write-Task -Name "plain" -Body @'
#!/usr/bin/env bash
printf 'plain: [%s]\n' "$1"
'@

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

    It 'passes the task its arguments through a -c shell' {
        $output = mise run bash_c ARG1 ARG2 2>&1 | Out-String
        $output | Should -Match "bash -c: \[ARG1\] \[ARG2\]"
    }

    It 'does not mangle the script path on the way' {
        # The failure this guards against ate every backslash out of the path, so the message named
        # `C:UsersRunner...`. Asserted separately from the arguments because it is a different
        # break with the same cause.
        $output = mise run bash_c ARG1 ARG2 2>&1 | Out-String
        $output | Should -Not -Match "command not found"
    }

    It 'still runs a task that takes its shell from the shebang' {
        $output = mise run plain ARG1 | Out-String
        $output | Should -Match "plain: \[ARG1\]"
    }
}
