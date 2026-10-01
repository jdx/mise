Describe 'shim_exec_recursion' {
    # Regression test: when not_found_auto_install preserves shims in PATH,
    # `mise x -- tool` should not resolve "tool" to a shim in the shims
    # directory, which would cause infinite process spawning on Windows.
    #
    # We verify this by checking that which::which_in resolves the real tool
    # binary (in toolDir) rather than the shim (in shimPath), even when the
    # shims directory appears before toolDir in PATH.

    BeforeAll {
        $script:originalPath = Get-Location
        $script:originalEnvPath = $env:PATH
        $script:originalTrustedConfigPaths = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        $script:shimPath = Join-Path -Path $env:MISE_DATA_DIR -ChildPath "shims"

        # Create a fake "mytool" binary that echoes a marker
        $script:toolDir = Join-Path $TestDrive "toolbin"
        New-Item -ItemType Directory -Path $script:toolDir -Force | Out-Null
        @'
@echo off
if defined __MISE_SHIM_PATH echo SHIM_PATH_LEAKED
echo REAL_TOOL_OUTPUT
'@ | Out-File -FilePath (Join-Path $script:toolDir "mytool.cmd") -Encoding ascii -NoNewline

        # Create a shim script for mytool in the shims directory (mimics "file" mode).
        # If the fix fails and exec resolves to this shim, the `where` command below
        # would show the shim path instead of the real tool path.
        New-Item -ItemType Directory -Path $script:shimPath -Force | Out-Null
        @'
@echo off
echo SHIM_NOT_REAL
'@ | Out-File -FilePath (Join-Path $script:shimPath "mytool.cmd") -Encoding ascii -NoNewline

        # Put shims BEFORE toolDir in PATH (the problematic ordering).
        # The fix should strip shims from the exec lookup path so the real
        # tool is resolved instead of the shim.
        $env:PATH = "$($script:shimPath);$($script:toolDir);$env:PATH"
    }

    AfterAll {
        Remove-Item -Path (Join-Path $script:shimPath "mytool.cmd") -ErrorAction SilentlyContinue
        Set-Location $script:originalPath
        $env:PATH = $script:originalEnvPath
        if ($null -eq $script:originalTrustedConfigPaths) {
            Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:originalTrustedConfigPaths, 'Process')
        }

        # Windows can briefly retain an executable's file lock after its output
        # stream closes. Wait for exclusive write access without changing files,
        # then leave fixture removal to Pester's TestDrive teardown.
        foreach ($executable in Get-ChildItem -LiteralPath $TestDrive -Recurse -File -Filter '*.exe') {
            for ($attempt = 0; ; $attempt++) {
                try {
                    $stream = [System.IO.File]::Open($executable.FullName, 'Open', 'ReadWrite', 'None')
                    $stream.Dispose()
                    break
                } catch [System.IO.IOException], [System.UnauthorizedAccessException] {
                    # File.Open wraps the Windows error in MethodInvocationException.
                    # Mapped executables can report ERROR_ACCESS_DENIED (5) as well
                    # as ERROR_SHARING_VIOLATION (32) / ERROR_LOCK_VIOLATION (33).
                    $errorCode = $_.Exception.GetBaseException().HResult -band 0xffff
                    if ($errorCode -notin @(5, 32, 33) -or $attempt -ge 20) {
                        throw
                    }
                    Start-Sleep -Milliseconds 100
                }
            }
        }
    }

    It 'mise x resolves real tool, not shim' {
        # Without the fix, which::which_in would resolve to the shim.
        # With the fix, shims are stripped from the lookup path and the
        # real tool in $toolDir is found instead.
        $result = mise x -- mytool
        $LASTEXITCODE | Should -Be 0
        $result | Should -Contain "REAL_TOOL_OUTPUT"
        $result | Should -Not -Contain "SHIM_NOT_REAL"
        $result | Should -Not -Contain "SHIM_PATH_LEAKED"
    }

    It 'native shim resolves a real tool when MISE_DATA_DIR is filtered out' {
        $customShimPath = Join-Path $TestDrive "custom-shims"
        New-Item -ItemType Directory -Path $customShimPath -Force | Out-Null
        Copy-Item (Join-Path $PSScriptRoot "..\target\debug\mise-shim.exe") `
            (Join-Path $customShimPath "mytool.exe")

        $originalDataDir = $env:MISE_DATA_DIR
        $previousPath = $env:PATH
        try {
            Remove-Item Env:\MISE_DATA_DIR -ErrorAction Ignore
            $env:PATH = "$customShimPath;$($script:toolDir);$env:PATH"

            $result = & (Join-Path $customShimPath "mytool.exe")

            $LASTEXITCODE | Should -Be 0
            $result | Should -Contain "REAL_TOOL_OUTPUT"
            $result | Should -Not -Contain "SHIM_PATH_LEAKED"
        } finally {
            $env:MISE_DATA_DIR = $originalDataDir
            $env:PATH = $previousPath
        }
    }

    It 'native shim preserves a sibling real executable in the same directory' {
        $customShimPath = Join-Path $TestDrive "same-dir-native-shims"
        New-Item -ItemType Directory -Path $customShimPath -Force | Out-Null
        Copy-Item (Join-Path $PSScriptRoot "..\target\debug\mise-shim.exe") `
            (Join-Path $customShimPath "mytool.exe")
        @'
@echo off
if defined __MISE_SHIM_PATH echo SHIM_PATH_LEAKED
echo SAME_DIRECTORY_REAL_TOOL
'@ | Out-File -FilePath (Join-Path $customShimPath "mytool.cmd") -Encoding ascii -NoNewline

        $originalDataDir = $env:MISE_DATA_DIR
        $previousPath = $env:PATH
        try {
            Remove-Item Env:\MISE_DATA_DIR -ErrorAction Ignore
            $env:PATH = "$customShimPath;$($script:originalEnvPath)"

            $result = & (Join-Path $customShimPath "mytool.exe")

            $LASTEXITCODE | Should -Be 0
            $result | Should -Contain "SAME_DIRECTORY_REAL_TOOL"
            $result | Should -Not -Contain "SHIM_PATH_LEAKED"
        } finally {
            $env:MISE_DATA_DIR = $originalDataDir
            $env:PATH = $previousPath
        }
    }

    It 'native shim stops when mise x resolves it to another copy of itself' {
        # `mise x` clears __MISE_SHIM_PATH before running what it resolved, so a second copy
        # (a shim farm plus WinGet\Links, say) used to hand the call back and forth forever.
        # `mise x` now records its pick in __MISE_SHIM_TARGET, and that copy stops at once.
        $copyA = Join-Path $TestDrive "copy-a"
        $copyB = Join-Path $TestDrive "copy-b"
        foreach ($dir in $copyA, $copyB) {
            New-Item -ItemType Directory -Path $dir -Force | Out-Null
            Copy-Item (Join-Path $PSScriptRoot "..\target\debug\mise-shim.exe") (Join-Path $dir "mytool.exe")
        }

        # A job object lets a regression be killed as a whole tree rather than hang CI; a live
        # loop spawns faster than `taskkill /T` walks it.
        if (-not ('MiseShimLoopJob' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MiseShimLoopJob {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, IntPtr name);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);
}
'@
        }

        function Invoke-CopyA {
            $job = [MiseShimLoopJob]::CreateJobObjectW([IntPtr]::Zero, [IntPtr]::Zero)
            $process = $null
            try {
                # cmd waits on stdin so the shim starts only after cmd has joined the job.
                $psi = [System.Diagnostics.ProcessStartInfo]::new($env:ComSpec,
                    "/d /c `"set /p _= & `"$(Join-Path $copyA 'mytool.exe')`"`"")
                $psi.UseShellExecute = $false
                # .NET starts in its own current directory, not Pester's location; without this,
                # `mise x` would load the repository's config and install its tools first.
                $psi.WorkingDirectory = (Get-Location).ProviderPath
                $psi.RedirectStandardInput = $true
                $psi.RedirectStandardOutput = $true
                $psi.RedirectStandardError = $true
                $process = [System.Diagnostics.Process]::Start($psi)
                if (-not [MiseShimLoopJob]::AssignProcessToJobObject($job, $process.Handle)) {
                    # cmd is still waiting on stdin, so the shim has not started; outside the job,
                    # only this handle can stop it.
                    $process.Kill()
                    $null = $process.WaitForExit(10000)
                    throw "the launcher could not be assigned to a job object"
                }
                $stdout = $process.StandardOutput.ReadToEndAsync()
                $stderr = $process.StandardError.ReadToEndAsync()
                $process.StandardInput.WriteLine()
                $process.StandardInput.Close()

                $exited = $process.WaitForExit(60000)
                if (-not $exited) {
                    $terminated = [MiseShimLoopJob]::TerminateJobObject($job, 1)
                    if (-not ($terminated -and $process.WaitForExit(10000))) {
                        throw "the shim copies alternated and the job could not be terminated"
                    }
                }
                [pscustomobject]@{
                    Exited   = $exited
                    ExitCode = $process.ExitCode
                    Output   = $stdout.Result -split "`r?`n"
                    Error    = $stderr.Result
                }
            } finally {
                if ($process) {
                    $process.Dispose()
                }
                [MiseShimLoopJob]::CloseHandle($job) | Out-Null
            }
        }

        $originalDataDir = $env:MISE_DATA_DIR
        $previousPath = $env:PATH
        try {
            Remove-Item Env:\MISE_DATA_DIR -ErrorAction Ignore
            $env:PATH = "$copyA;$copyB;$($script:toolDir);$($script:originalEnvPath)"

            # The copy ahead of the real tool is a misconfigured PATH: say so rather than skip it.
            $run = Invoke-CopyA
            $run.Exited | Should -BeTrue -Because "the shim copies alternated: $($run.Error)"
            $run.ExitCode | Should -Not -Be 0
            $run.Error | Should -Match 'which is another shim, not the tool'
            $run.Output | Should -Not -Contain "REAL_TOOL_OUTPUT"

            # The same with no real tool behind the copies.
            $env:PATH = "$copyA;$copyB;$($script:originalEnvPath)"
            $run = Invoke-CopyA
            $run.Exited | Should -BeTrue -Because "the shim copies alternated: $($run.Error)"
            $run.ExitCode | Should -Not -Be 0
            $run.Error | Should -Match 'which is another shim, not the tool'
        } finally {
            $env:MISE_DATA_DIR = $originalDataDir
            $env:PATH = $previousPath
        }
    }

    It 'file shim resolves a real tool when MISE_DATA_DIR is filtered out' {
        $customShimPath = Join-Path $TestDrive "file-shims"
        New-Item -ItemType Directory -Path $customShimPath -Force | Out-Null
        @'
@echo off
setlocal
set "shim_path=%~f0"
if /I "%__MISE_SHIM_PATH%"=="%shim_path%" exit /b 1
set "__MISE_SHIM_PATH=%shim_path%"
mise x -- mytool %*
'@ | Out-File -FilePath (Join-Path $customShimPath "mytool.cmd") -Encoding ascii -NoNewline

        $originalDataDir = $env:MISE_DATA_DIR
        $previousPath = $env:PATH
        try {
            Remove-Item Env:\MISE_DATA_DIR -ErrorAction Ignore
            $env:PATH = "$customShimPath;$($script:toolDir);$env:PATH"

            $result = & (Join-Path $customShimPath "mytool.cmd")

            $LASTEXITCODE | Should -Be 0
            $result | Should -Contain "REAL_TOOL_OUTPUT"
            $result | Should -Not -Contain "SHIM_PATH_LEAKED"
        } finally {
            $env:MISE_DATA_DIR = $originalDataDir
            $env:PATH = $previousPath
        }
    }

    It 'file shim preserves a sibling real executable in the same directory' {
        $customShimPath = Join-Path $TestDrive "same-dir-file-shims"
        New-Item -ItemType Directory -Path $customShimPath -Force | Out-Null
        @'
@echo off
setlocal
set "shim_path=%~f0"
if /I "%__MISE_SHIM_PATH%"=="%shim_path%" exit /b 1
set "__MISE_SHIM_PATH=%shim_path%"
mise x -- mytool %*
'@ | Out-File -FilePath (Join-Path $customShimPath "mytool.cmd") -Encoding ascii -NoNewline
        Copy-Item $env:ComSpec (Join-Path $customShimPath "mytool.exe")

        $originalDataDir = $env:MISE_DATA_DIR
        $previousPath = $env:PATH
        try {
            Remove-Item Env:\MISE_DATA_DIR -ErrorAction Ignore
            $env:PATH = "$customShimPath;$($script:originalEnvPath)"

            $result = & (Join-Path $customShimPath "mytool.cmd") /d /c `
                'if defined __MISE_SHIM_PATH (echo SHIM_PATH_LEAKED) else echo SAME_DIRECTORY_REAL_TOOL'

            $LASTEXITCODE | Should -Be 0
            $result | Should -Contain "SAME_DIRECTORY_REAL_TOOL"
            $result | Should -Not -Contain "SHIM_PATH_LEAKED"
        } finally {
            $env:MISE_DATA_DIR = $originalDataDir
            $env:PATH = $previousPath
        }
    }

    It 'hardlink shim resolves a real tool when MISE_DATA_DIR is filtered out' {
        $customShimPath = Join-Path $TestDrive "direct-shims"
        $directToolDir = Join-Path $TestDrive "direct-toolbin"
        New-Item -ItemType Directory -Path $customShimPath -Force | Out-Null
        New-Item -ItemType Directory -Path $directToolDir -Force | Out-Null
        $misePath = Join-Path $PSScriptRoot "..\target\debug\mise.exe"
        $localMisePath = Join-Path $customShimPath "mise.exe"
        Copy-Item $misePath $localMisePath
        New-Item -ItemType HardLink -Path (Join-Path $customShimPath "mytool.exe") `
            -Target $localMisePath | Out-Null
        Copy-Item $env:ComSpec (Join-Path $directToolDir "mytool.exe")

        $originalDataDir = $env:MISE_DATA_DIR
        $previousPath = $env:PATH
        try {
            Remove-Item Env:\MISE_DATA_DIR -ErrorAction Ignore
            $env:PATH = "$customShimPath;$directToolDir;$env:PATH"

            $result = & mytool.exe /d /c `
                'if defined __MISE_SHIM_PATH (echo SHIM_PATH_LEAKED) else echo REAL_TOOL_OUTPUT'

            $LASTEXITCODE | Should -Be 0
            $result | Should -Contain "REAL_TOOL_OUTPUT"
            $result | Should -Not -Contain "SHIM_PATH_LEAKED"
        } finally {
            $env:MISE_DATA_DIR = $originalDataDir
            $env:PATH = $previousPath
        }
    }
}
