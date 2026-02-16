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
        Set-Location TestDrive:
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        $script:shimPath = Join-Path -Path $env:MISE_DATA_DIR -ChildPath "shims"

        # Create a fake "mytool" binary that echoes a marker
        $script:toolDir = Join-Path $TestDrive "toolbin"
        New-Item -ItemType Directory -Path $script:toolDir -Force | Out-Null
        @'
@echo off
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
        Remove-Item -Path $script:toolDir -Recurse -ErrorAction SilentlyContinue
        Set-Location $script:originalPath
        Remove-Item -Path Env:\MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
    }

    It 'mise x resolves real tool, not shim' {
        # Without the fix, which::which_in would resolve to the shim.
        # With the fix, shims are stripped from the lookup path and the
        # real tool in $toolDir is found instead.
        $result = mise x -- mytool
        $LASTEXITCODE | Should -Be 0
        $result | Should -Contain "REAL_TOOL_OUTPUT"
        $result | Should -Not -Contain "SHIM_NOT_REAL"
    }
}
