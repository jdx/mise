Describe 'file-mode shim argument delivery' {
    # `windows_shim_mode = "file"` writes a `<tool>.cmd`, and cmd.exe parses the whole command line
    # before a batch file's `%*` expands. Every shape that reaches a native shim intact used to
    # arrive different through this one: `c&d` ran `d` as a second command, `a>b` redirected the
    # shim's own output into a file called `b`, `e%OS%f` became `eWindows_NTf`. The `.cmd` now
    # recovers the original text out of cmd's `CMDCMDLINE`, the same way a task-stub launcher does.
    #
    # Shims are invoked by bare name off PATH, so that is how they are invoked here: the recovery
    # matches the shim's own full path against the raw line, and only a run that resolved the name
    # to that path can be recovered.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalPath = $env:PATH
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        # The mode is switched through the environment, not `mise settings`: that writes to the
        # global config, and unsetting it afterwards would clear a preference the developer set.
        $script:OriginalShimMode = [Environment]::GetEnvironmentVariable('MISE_WINDOWS_SHIM_MODE', 'Process')

        $script:TestRoot = Join-Path $TestDrive 'shim-args'
        New-Item -ItemType Directory -Path $script:TestRoot -Force | Out-Null
        Set-Location $script:TestRoot
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        $script:ShimDir = Join-Path $env:MISE_DATA_DIR 'shims'

        # The fake tool. It prints one line per argument, and it has to survive its own arguments:
        # `%~1` is substituted before cmd parses redirection, so `echo ARG:%~1` would redirect on
        # `a>b`. Going through a variable and reading it back with delayed expansion puts the value
        # past that parse.
        $script:ToolRoot = Join-Path $script:TestRoot 'faketool'
        New-Item -ItemType Directory -Path $script:ToolRoot -Force | Out-Null
        $echoer = @'
@echo off
setlocal EnableDelayedExpansion
:loop
if "%~1"=="" goto :eof
set "arg=%~1"
echo ARG:!arg!
shift
goto loop
'@
        # `mise link` points a version name at a directory of the user's, so nothing is downloaded.
        # Core node's Windows bin path is the install root; the `bin` copy is there in case that
        # ever changes, and costs nothing.
        $echoer | Out-File -FilePath (Join-Path $script:ToolRoot 'argecho.cmd') -Encoding ascii
        New-Item -ItemType Directory -Path (Join-Path $script:ToolRoot 'bin') -Force | Out-Null
        $echoer | Out-File -FilePath (Join-Path $script:ToolRoot 'bin\argecho.cmd') -Encoding ascii

        # `--force` because `$TestDrive` is a new directory every run: a run that died before
        # `AfterAll` leaves `e2e-shimargs` pointing somewhere else, and an unforced link would then
        # fail for every later run rather than just this one.
        mise link --force node@e2e-shimargs $script:ToolRoot | Out-Null
        $script:LinkExit = $LASTEXITCODE
        @'
[tools]
node = "e2e-shimargs"
'@ | Out-File -FilePath (Join-Path $script:TestRoot 'mise.toml') -Encoding utf8NoBOM

        # The shapes cmd destroys on the way into a batch file. `k|l` and a bare `"` are deliberately
        # absent: cmd builds a pipe for the first and swallows the second before the shim runs, so
        # its own command line no longer holds them and no `.cmd` can recover them. They are checked
        # separately below.
        $script:Shapes = @('plain', 'c&d', 'i^j', 'e%OS%f', 'a>b', 'a<b', '^caret')

        # Compared as one string: an argument that vanished shifts everything after it, and reading
        # that off a joined line is far easier than off two collections.
        function script:ArgText($output) {
            (@($output | ForEach-Object { "$_" } | Where-Object { $_ -like 'ARG:*' } |
                        ForEach-Object { $_.Substring(4) }) -join ' :: ')
        }

        function script:SetMode([string]$mode) {
            $env:MISE_WINDOWS_SHIM_MODE = $mode
            mise reshim --force | Out-Null
        }

        function script:UseShims([string]$dir) {
            $env:PATH = "$dir;$($script:OriginalPath)"
        }

        # What the tool receives when nothing between the caller and mise is a batch file:
        # PowerShell hands `mise.exe` its argv directly. Every assertion below is against this.
        $script:Reference = script:ArgText (mise x -- argecho @script:Shapes)
        $script:Expected = ($script:Shapes -join ' :: ')
    }

    AfterAll {
        if ($null -ne $script:OriginalShimMode) {
            $env:MISE_WINDOWS_SHIM_MODE = $script:OriginalShimMode
        } else {
            # `Remove-Item`, not `SetEnvironmentVariable(.., $null, ..)`: PowerShell binds `$null`
            # to that `[string]` parameter as an empty string, so the variable stays present and
            # mise reads an empty mode for the rest of the run.
            Remove-Item Env:\MISE_WINDOWS_SHIM_MODE -ErrorAction SilentlyContinue
        }
        mise uninstall node@e2e-shimargs | Out-Null
        # Back to whatever mode this machine is configured for, so the shims left behind are the
        # ones that were there before.
        mise reshim --force | Out-Null
        Set-Location $script:OriginalDir
        $env:PATH = $script:OriginalPath
        if ($null -ne $script:OriginalTrusted) {
            $env:MISE_TRUSTED_CONFIG_PATHS = $script:OriginalTrusted
        } else {
            Remove-Item Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
    }

    It 'links the fake tool and reaches it without a shim' {
        # If this fails every comparison below is against nothing, so it is checked on its own.
        $script:LinkExit | Should -Be 0
        $script:Reference | Should -Be $script:Expected
    }

    Context 'file mode' {
        BeforeAll {
            script:SetMode 'file'
            $script:FileShim = Join-Path $script:ShimDir 'argecho.cmd'

            # The old body, kept to show the shapes really did not arrive before this change. It is
            # reached the same way -- bare name, first on PATH -- so the body is the only difference.
            $script:OldDir = Join-Path $script:TestRoot 'oldshims'
            New-Item -ItemType Directory -Path $script:OldDir -Force | Out-Null
            @'
@echo off
setlocal
set "shim_path=%~f0"
if /I "%__MISE_SHIM_PATH%"=="%shim_path%" (
  echo mise: recursive shim invocation detected for argecho: %shim_path% 1>&2
  exit /b 1
)
set "__MISE_SHIM_PATH=%shim_path%"
mise x -- argecho %*
'@ | Out-File -FilePath (Join-Path $script:OldDir 'argecho.cmd') -Encoding ascii
        }

        It 'writes a .cmd shim, not a native one' {
            Test-Path $script:FileShim | Should -BeTrue
            Test-Path (Join-Path $script:ShimDir 'argecho.exe') | Should -BeFalse
            # file mode also emits the extension-less shim for Git Bash/Cygwin.
            Test-Path (Join-Path $script:ShimDir 'argecho') -PathType Leaf | Should -BeTrue
        }

        It 'recovers the caller arguments out of the raw command line' {
            (Get-Content $script:FileShim -Raw) | Should -BeLike '*!CMDCMDLINE!*'
        }

        It 'delivers every argument through a PATH-resolved shim' {
            script:UseShims $script:ShimDir
            script:ArgText (& argecho @script:Shapes) | Should -Be $script:Reference
        }

        It 'the old body did not' {
            # The control. Without it this file would pass just as well against the old shim.
            script:UseShims $script:OldDir
            script:ArgText (& argecho @script:Shapes 2>&1) | Should -Not -Be $script:Reference
        }

        It 'the old body expanded %VAR% out of an argument and the new one does not' {
            # One shape on its own, because the aggregate above can only say "different". `e%OS%f`
            # involves no redirection and no queued command, so what it shows is unambiguous.
            script:UseShims $script:OldDir
            script:ArgText (& argecho 'e%OS%f' 2>&1) | Should -Be 'eWindows_NTf'
            script:UseShims $script:ShimDir
            script:ArgText (& argecho 'e%OS%f') | Should -Be 'e%OS%f'
        }

        It 'reports the tool exit code rather than a command cmd queued' {
            # `&` in an argument makes cmd queue a second command from the same line -- given `c&d`
            # it intends to run `d` next, and that failure would be reported as the tool's status.
            script:UseShims $script:ShimDir
            $out = & argecho 'c&d' 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            $out | Should -Not -Match 'not recognized as an internal or external command'
        }

        It 'still refuses to invoke itself' {
            script:UseShims $script:ShimDir
            $previousShimPath = [Environment]::GetEnvironmentVariable('__MISE_SHIM_PATH', 'Process')
            $env:__MISE_SHIM_PATH = (Resolve-Path $script:FileShim).Path
            try {
                $out = & argecho plain 2>&1 | Out-String
                $LASTEXITCODE | Should -Be 1
                $out | Should -Match 'recursive shim invocation detected for argecho'
            } finally {
                if ($null -ne $previousShimPath) {
                    $env:__MISE_SHIM_PATH = $previousShimPath
                } else {
                    Remove-Item Env:\__MISE_SHIM_PATH -ErrorAction SilentlyContinue
                }
            }
        }

        It 'cannot carry a pipe, and neither could the old body' {
            # Stated rather than hidden: cmd builds the pipe before the shim runs, so the shim's own
            # command line no longer holds it. A native shim is handed argv and has no such problem.
            script:UseShims $script:ShimDir
            $new = script:ArgText (& argecho 'k|l' 2>&1)
            script:UseShims $script:OldDir
            $old = script:ArgText (& argecho 'k|l' 2>&1)
            $new | Should -Not -Be 'k|l'
            $old | Should -Not -Be 'k|l'
        }
    }

    Context 'exe mode' {
        BeforeAll {
            script:SetMode 'exe'
        }

        It 'writes a native shim' {
            Test-Path (Join-Path $script:ShimDir 'argecho.exe') | Should -BeTrue
            Test-Path (Join-Path $script:ShimDir 'argecho.cmd') | Should -BeFalse
        }

        It 'delivers every argument, including the one no .cmd can carry' {
            script:UseShims $script:ShimDir
            script:ArgText (& argecho @script:Shapes) | Should -Be $script:Reference
            # A native shim receives argv, so the shape the batch recovery gives up on arrives too.
            script:ArgText (& argecho 'k|l') | Should -Be 'k|l'
        }
    }
}
