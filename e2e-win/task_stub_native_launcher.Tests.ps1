Describe 'task-stub launchers and arguments' {
    # `bin\<task>.cmd` is a batch file, and cmd.exe re-parses the whole line before `%*` expands.
    # Calling one from PowerShell therefore used to lose `& ^ | " < >` and expand `%VAR%` out of
    # the arguments, while calling `mise run` directly does not.
    #
    # Two things address it. The `.cmd` now recovers the original text out of cmd's `CMDCMDLINE`
    # and hands it to mise through the environment, which covers nearly everything. And
    # `--windows-launcher exe` writes a native `<task>.exe` -- a copy of mise-shim.exe, handed argv
    # directly -- which has none of the problem to begin with.

    BeforeAll {
        $script:OriginalDir = Get-Location
        # Pester runs every suite in one process, so what this suite sets has to be put back.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')

        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot 'mise-tasks') | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        '[tools]' | Out-File -Encoding utf8NoBOM (Join-Path $script:TestRoot 'mise.toml')

        # The observation channel. A pwsh file task, because `$args` is the one place on Windows
        # that shows each argument as its own string -- `echo` through cmd would join them back
        # together and hide the very thing under test. LF, so no `r rides into the interpreter.
        $body = @'
#!/usr/bin/env pwsh
foreach ($a in $args) { Write-Output "ARG[$a]" }
'@ -replace "`r`n", "`n"
        [System.IO.File]::WriteAllText(
            (Join-Path $script:TestRoot 'mise-tasks\argecho'),
            $body,
            (New-Object System.Text.UTF8Encoding $false))

        # Every shape measured to reach the task differently through a plain `%*` launcher, plus
        # ones that always survived, so the comparison is not carried by a single character.
        # `k|l` and a bare `"` are deliberately absent: cmd builds a pipe for the first and the
        # second is genuinely ambiguous, so neither is recoverable and both are covered on their
        # own further down.
        $script:TaskArgs = @('plain', 'c&d', 'i^j', 'e%OS%f', 'a>b', 'a<b', '^caret', 'm n')

        # Only the task's own output. mise writes progress of its own, and a launcher that ran
        # nothing at all would otherwise still "match" an empty reference.
        function script:ArgLines($Output) {
            ((@($Output) | Where-Object { $_ -like 'ARG`[*' }) -join "`n")
        }

        # What the task should see: the reference every launcher is compared against.
        function script:Expected {
            (($script:TaskArgs | ForEach-Object { "ARG[$_]" }) -join "`n")
        }

        Set-Location $script:TestRoot
        mise generate task-stubs | Out-Null
        $script:GenerateExit = $LASTEXITCODE
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'is measuring the right thing' {
        # Asserted on its own, so a setup failure downstream cannot be read as the defect. `mise
        # run` is the reference the launchers are held to, and if it does not deliver the arguments
        # then every comparison below passes while proving nothing.
        $script:GenerateExit | Should -Be 0
        Test-Path 'bin\argecho' | Should -BeTrue
        Test-Path 'bin\argecho.cmd' | Should -BeTrue

        $direct = script:ArgLines (& mise run argecho @script:TaskArgs)
        $LASTEXITCODE | Should -Be 0
        $direct | Should -Be (script:Expected)
    }

    It 'delivers every argument through the default .cmd launcher' {
        # The point of the change, on the path everyone gets without opting in.
        $out = script:ArgLines (& '.\bin\argecho.cmd' @script:TaskArgs)
        # Captured before anything else runs. `script:ArgLines` drops every line that is not an
        # `ARG[...]`, so a launcher can print all of them and still exit nonzero.
        $status = $LASTEXITCODE
        $out | Should -Be (script:Expected)
        $status | Should -Be 0
    }

    It 'reports the task exit code rather than a command cmd queued' {
        # `&` in an argument makes cmd queue a second command from the same line -- given `c&d` it
        # intends to run `d` afterwards. The launcher has to stop that, or a task that succeeded
        # reports the failure of a command nobody asked for.
        $out = & '.\bin\argecho.cmd' 'c&d' 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Not -Match 'is not recognized'
    }

    It 'is what a launcher without argument recovery could not do' {
        # The control. Without it, the assertions above would also pass on a Windows where nothing
        # was ever broken, and would say nothing. This is the body mise wrote before the fix.
        New-Item -ItemType Directory -Force 'oldbin' | Out-Null
        Copy-Item 'bin\argecho' 'oldbin\argecho'
        [System.IO.File]::WriteAllText(
            (Join-Path (Get-Location) 'oldbin\argecho.cmd'),
            "@echo off`r`nrem generated by mise`r`n`"mise`" run `"argecho`" %*`r`n")

        $out = script:ArgLines (& '.\oldbin\argecho.cmd' @script:TaskArgs 2>&1)
        $out | Should -Not -Be (script:Expected)
    }

    It 'replaces a launcher written before argument recovery existed' {
        # These files are committed, so every project generated by an older mise has the old body
        # checked in. Refusing to recognise it would make regeneration bail on exactly the
        # launchers that need replacing.
        mise generate task-stubs --dir oldbin | Out-Null
        $LASTEXITCODE | Should -Be 0
        (Get-Content 'oldbin\argecho.cmd' -Raw) | Should -BeLike '*!CMDCMDLINE!*'
        $out = script:ArgLines (& '.\oldbin\argecho.cmd' @script:TaskArgs)
        $status = $LASTEXITCODE
        $out | Should -Be (script:Expected)
        $status | Should -Be 0
    }

    It 'leaves the shell in charge when the shell already split the arguments' {
        # `call` from a batch file: cmd parsed the line before the launcher ran, exactly as it
        # would have for a native program, so there is nothing to recover. The launcher must
        # notice and must not `exit` -- that would end the caller's script.
        $outer = Join-Path (Get-Location) 'outer.cmd'
        [System.IO.File]::WriteAllText($outer,
            "@echo off`r`ncall `"%~dp0bin\argecho.cmd`" plain`r`necho OUTER-ALIVE`r`n")
        $out = & cmd /c "`"$outer`"" 2>&1 | Out-String
        $out | Should -Match 'ARG\[plain\]'
        # The caller survived: the fallback path does not terminate cmd.
        $out | Should -Match 'OUTER-ALIVE'
    }

    It 'does not make a pipe in an argument any worse' {
        # `k|l` cannot be recovered: cmd builds a pipe, so the launcher's own command line is not
        # the one the user typed. Pinned rather than fixed -- what matters is that it is no worse
        # than the launcher without recovery.
        $new = script:ArgLines (& '.\bin\argecho.cmd' 'k|l' 2>&1)
        $old = script:ArgLines (& '.\oldbin\argecho.cmd' 'k|l' 2>&1)
        $new | Should -Be $old
    }

    Context 'native launcher' {
        BeforeAll {
            Set-Location $script:TestRoot
            mise generate task-stubs --dir exebin --windows-launcher exe | Out-Null
            $script:ExeGenerateExit = $LASTEXITCODE
        }

        It 'writes a native launcher instead of the .cmd' {
            $script:ExeGenerateExit | Should -Be 0
            Test-Path 'exebin\argecho' | Should -BeTrue
            Test-Path 'exebin\argecho.exe' | Should -BeTrue
            # The other form must not be left beside it: Windows resolves `.exe` before `.cmd`, so
            # a leftover would shadow the launcher that replaced it.
            Test-Path 'exebin\argecho.cmd' | Should -BeFalse
            # It really is the shipped shim, byte for byte -- which is also how cleanup knows it.
            $shim = Join-Path (Split-Path (Get-Command mise).Source) 'mise-shim.exe'
            (Get-FileHash 'exebin\argecho.exe').Hash | Should -Be (Get-FileHash $shim).Hash
        }

        It 'delivers every argument, including the ones the .cmd cannot' {
            $out = script:ArgLines (& '.\exebin\argecho.exe' @script:TaskArgs)
            $LASTEXITCODE | Should -Be 0
            $out | Should -Be (script:Expected)

            # The two shapes no `.cmd` can carry. A native launcher is handed argv directly, so it
            # has neither problem -- this is what the mode is for.
            $out = script:ArgLines (& '.\exebin\argecho.exe' 'k|l' 'g"h')
            $status = $LASTEXITCODE
            $out | Should -Be "ARG[k|l]`nARG[g`"h]"
            $status | Should -Be 0
        }

        It 'runs the mise the stub names rather than whatever is on PATH' {
            # A native launcher is a byte copy of a shared binary and can carry nothing of its own:
            # `--mise-bin` reaches it only because it reads the stub written beside it. Pointing at
            # a path that does not exist is what makes this decisive -- a launcher that ignored the
            # stub would find `mise` on PATH and succeed.
            mise generate task-stubs --dir missingbin --windows-launcher exe --mise-bin .\no-such-mise.exe | Out-Null
            $LASTEXITCODE | Should -Be 0
            & '.\missingbin\argecho.exe' 2>&1 | Out-Null
            $LASTEXITCODE | Should -Not -Be 0

            # The other half: a custom path that does exist still works, so the failure above is
            # the stub being honoured and not `--mise-bin` breaking the launcher outright.
            #
            # The copy keeps the name `mise.exe`. mise dispatches on its own file name -- that is
            # how the `hardlink` and `symlink` shim modes work -- so a copy called anything else
            # runs as a shim for a tool of that name and exits with "is not a valid shim".
            New-Item -ItemType Directory -Force 'altmise' | Out-Null
            Copy-Item (Get-Command mise).Source 'altmise\mise.exe'
            mise generate task-stubs --dir custombin --windows-launcher exe --mise-bin .\altmise\mise.exe | Out-Null
            $LASTEXITCODE | Should -Be 0
            $out = script:ArgLines (& '.\custombin\argecho.exe' 'c&d')
            $LASTEXITCODE | Should -Be 0
            $out | Should -Be 'ARG[c&d]'
        }

        It 'switches back to the .cmd without leaving the .exe behind' {
            # The other half of the mode switch. A stale `.exe` would keep winning over the `.cmd`
            # that replaced it, so regenerating in the default mode has to remove it.
            mise generate task-stubs --dir exebin | Out-Null
            $LASTEXITCODE | Should -Be 0
            Test-Path 'exebin\argecho.cmd' | Should -BeTrue
            Test-Path 'exebin\argecho.exe' | Should -BeFalse
        }

        It 'refuses to overwrite an .exe it did not write' {
            # Same rule the `.cmd` marker enforces: `bin\<task>.exe` is a name a project may
            # already be using, and nothing about it says mise owns it.
            New-Item -ItemType Directory -Force 'ownedbin' | Out-Null
            Set-Content -LiteralPath 'ownedbin\argecho.exe' -Value 'not mine' -NoNewline

            $out = mise generate task-stubs --dir ownedbin --windows-launcher exe 2>&1 | Out-String
            # The status as well as the message: a run that printed the refusal and still exited 0
            # would leave a script believing it had generated stubs.
            $LASTEXITCODE | Should -Not -Be 0
            $out | Should -BeLike '*is not a generated launcher*'
            (Get-Content 'ownedbin\argecho.exe' -Raw) | Should -Be 'not mine'
            # Validation runs before anything is written, so the stub is not there either.
            Test-Path 'ownedbin\argecho' | Should -BeFalse
        }
    }
}
