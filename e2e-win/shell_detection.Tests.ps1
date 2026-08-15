Describe 'shell detection' {
    BeforeAll {
        $script:originalPath = Get-Location
        # The workflow sets these for the whole job and Pester runs every suite in one process, so
        # put back what was there rather than dropping it.
        $script:originalEnv = @{}
        foreach ($name in 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_SHELL') {
            $script:originalEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

        $script:testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:testDir | Out-Null
        Set-Location $script:testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:testDir

        # The whole point is what happens with nothing to detect from. `MISE_SHELL` is what an
        # activated session sets, so an inherited one would make every case below pass vacuously.
        Remove-Item -Path Env:\MISE_SHELL -ErrorAction SilentlyContinue
    }

    AfterAll {
        Set-Location $script:originalPath
        foreach ($name in $script:originalEnv.Keys) {
            if ($null -eq $script:originalEnv[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:originalEnv[$name], 'Process')
            }
        }
    }

    # Deliberately not a key named `Args` splatted as `@Args`. That is what this was, and mise
    # received the wrong argv under Pester: once Pester's own state object as an extra argument,
    # once nothing at all. `Args` is PowerShell's automatic variable for a block's unbound
    # arguments, so what `@Args` expands to inside a test is not simply what `-ForEach` set. Both
    # commands take a bare subcommand, so pass it as one argument and do not splat.
    It 'reports a missing shell for <Command> instead of panicking' -ForEach @(
        @{ Command = 'activate' }
        @{ Command = 'hook-env' }
    ) {
        # `COMSPEC` is all mise reads on Windows and it names cmd.exe, which is not a shell mise
        # generates for, so this is the ordinary path here rather than an edge case. It used to
        # abort: "The application panicked (crashed)."
        $out = & mise $Command 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -Not -Match 'panicked'
        $out | Should -Match 'could not tell which shell'
        # The instruction has to name something the reader can actually run on this platform.
        $out | Should -Match 'pwsh'
    }

    It 'still generates a script when the shell is named' {
        # The control. Without it "exited non-zero" above would not distinguish the fix from mise
        # being broken for every invocation.
        $out = & mise activate pwsh 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Match 'MISE_SHELL'
    }
}
