Describe 'a task directory that links back into itself' {
    # Task discovery follows links, which is what lets a shared task directory be linked in, and
    # also what makes a loop reachable. On Windows the link is a junction, made by `mklink /J`
    # without any privilege -- the "current" junction beside a versioned directory is a common
    # shape -- so this is easier to arrive at here than a symlink loop is on unix. One of them
    # used to fail every task command, including a task defined in `mise.toml` with no task file
    # behind it at all.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        function script:NewProject([string]$path) {
            New-Item -ItemType Directory -Path (Join-Path $path 'mise-tasks') -Force | Out-Null
            @'
[tasks.from_config]
run = "echo CONFIG_RAN"
'@ | Out-File -FilePath (Join-Path $path 'mise.toml') -Encoding utf8NoBOM
            "#!/usr/bin/env bash`necho FILE_RAN" |
                Out-File -FilePath (Join-Path $path 'mise-tasks\healthy') -Encoding utf8NoBOM
        }

        $script:TestRoot = Join-Path $TestDrive 'symlink-loop'
        New-Item -ItemType Directory -Path $script:TestRoot -Force | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        # The subject and its control differ in exactly one thing: the junction.
        $script:Looped = Join-Path $script:TestRoot 'looped'
        $script:Plain = Join-Path $script:TestRoot 'plain'
        script:NewProject $script:Looped
        script:NewProject $script:Plain
        cmd /c mklink /J "$script:Looped\mise-tasks\current" "$script:Looped\mise-tasks" | Out-Null
        $script:LinkExit = $LASTEXITCODE
    }

    AfterAll {
        Set-Location $script:OriginalDir
        # Pester hands the drive back by enumerating it with
        # Directory.GetFileSystemEntries(AllDirectories), which follows this junction and does not
        # come back out. Left in place it fails the run in the framework, with every test in it
        # green. Delete the link, not what it points at.
        $junction = Join-Path $script:Looped 'mise-tasks\current'
        if (Test-Path -LiteralPath $junction) {
            [System.IO.Directory]::Delete($junction, $false)
        }
        if ($null -ne $script:OriginalTrusted) {
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:OriginalTrusted
        } else {
            Remove-Item Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
    }

    It 'made the junction it is about' {
        # Checked on its own: if the fixture has no junction, everything below passes for the
        # wrong reason while still looking like a result.
        $script:LinkExit | Should -Be 0
        $entry = Get-Item -LiteralPath (Join-Path $script:Looped 'mise-tasks\current') -Force
        $entry.Attributes.ToString() | Should -Match 'ReparsePoint'
    }

    It 'lists the tasks it can reach' {
        Set-Location $script:Looped
        $out = mise tasks ls --name-only 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        # The failure had one exact signature, and it is the thing this test is here to keep out.
        $out | Should -Not -Match 'File system loop found'
        $names = @($out -split "`r?`n" | Where-Object { $_ -ne '' } | ForEach-Object { $_.Trim() })
        $names | Should -Contain 'healthy'
        $names | Should -Contain 'from_config'
        # The junction is a directory and is skipped, so it does not become a task of its own.
        $names | Should -Not -Contain 'current'
    }

    It 'runs a task defined in the config, which the walk has nothing to do with' {
        Set-Location $script:Looped
        $out = mise run from_config 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'CONFIG_RAN'
    }

    It 'runs a file task from the directory the junction points at' {
        Set-Location $script:Looped
        $out = mise run healthy 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'FILE_RAN'
    }

    It 'gives the same answer as the identical tree without the junction' {
        # The control. Stated as equality so the claim is "the loop changes nothing", not
        # "each of these happens to work".
        Set-Location $script:Looped
        $looped = (mise tasks ls --name-only 2>&1 | Out-String).Trim()
        $loopedExit = $LASTEXITCODE
        Set-Location $script:Plain
        $plain = (mise tasks ls --name-only 2>&1 | Out-String).Trim()
        $plainExit = $LASTEXITCODE
        # Each captured before the next command can overwrite it. Equality on its own would also
        # hold if both sides failed the same way, which would read as the loop changing nothing
        # while nothing had been listed at all.
        $loopedExit | Should -Be 0
        $plainExit | Should -Be 0
        $looped | Should -Be $plain
    }
}
