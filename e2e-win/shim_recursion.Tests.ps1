Describe 'shim_exec_recursion' {
    # Regression test: when not_found_auto_install preserves shims in PATH,
    # `mise x -- tool` must not resolve "tool" back to the shim script,
    # which would cause infinite process spawning on Windows.

    BeforeAll {
        $script:originalPath = Get-Location
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        $script:shimPath = Join-Path -Path $env:MISE_DATA_DIR -ChildPath "shims"

        # Create a fake "mytool" binary that just echoes a marker
        $script:toolDir = Join-Path $TestDrive "toolbin"
        New-Item -ItemType Directory -Path $script:toolDir -Force | Out-Null
        @'
@echo off
echo REAL_TOOL_OUTPUT
'@ | Out-File -FilePath (Join-Path $script:toolDir "mytool.cmd") -Encoding ascii -NoNewline

        # Create a shim script for mytool in the shims directory (mimics "file" mode)
        New-Item -ItemType Directory -Path $script:shimPath -Force | Out-Null
        @"
@echo off
setlocal
mise x -- mytool %*
"@ | Out-File -FilePath (Join-Path $script:shimPath "mytool.cmd") -Encoding ascii -NoNewline

        # Put both the tool dir and shims dir on PATH (shims before tool, the
        # problematic ordering). The fix should strip shims from the exec PATH
        # so the real tool is found instead.
        $env:PATH = "$($script:shimPath);$($script:toolDir);$env:PATH"
    }

    AfterAll {
        Remove-Item -Path (Join-Path $script:shimPath "mytool.cmd") -ErrorAction SilentlyContinue
        Remove-Item -Path $script:toolDir -Recurse -ErrorAction SilentlyContinue
        Set-Location $script:originalPath
        Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
    }

    It 'mise x does not recurse through shims' {
        # Without the fix, this would spawn processes infinitely and hang.
        # With the fix, shims are stripped from PATH and the real tool is found.
        $result = mise x -- mytool
        $LASTEXITCODE | Should -Be 0
        $result | Should -Contain "REAL_TOOL_OUTPUT"
    }
}
