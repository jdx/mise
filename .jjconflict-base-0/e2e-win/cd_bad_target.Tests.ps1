Describe 'a cd target that cannot be entered' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalCd = [Environment]::GetEnvironmentVariable('MISE_CD', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalCd) {
            Remove-Item Env:MISE_CD -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_CD', $script:OriginalCd, 'Process')
        }
    }

    It 'reports a missing cd target instead of aborting' {
        # `validate_cd_path` only inspects the `--cd` flag, so the environment route reaches the
        # `chdir` with no checks in front of it -- a missing directory is enough to get there.
        $env:MISE_CD = Join-Path $script:TestRoot "not-here"
        $out = mise env 2>&1 | Out-String
        # Captured before anything else runs: a cmdlet does not touch $LASTEXITCODE, but the next
        # native command would.
        $status = $LASTEXITCODE
        Remove-Item Env:MISE_CD -ErrorAction Ignore

        $out | Should -Not -Match 'panicked'
        $out | Should -Match 'failed to set current directory'
        # It has to actually fail. An error message printed on a successful exit would satisfy the
        # assertions above. "Not zero" rather than a specific code, which is all this pins.
        $status | Should -Not -Be 0
    }

    It 'still runs when the cd target is real' {
        # Control: the same variable pointing somewhere that exists, so the failure above is pinned
        # on the target rather than on MISE_CD being set at all.
        $env:MISE_CD = $script:TestRoot
        $out = mise env 2>&1 | Out-String
        $status = $LASTEXITCODE
        Remove-Item Env:MISE_CD -ErrorAction Ignore

        $out | Should -Not -Match 'panicked'
        $out | Should -Not -Match 'failed to set current directory'
        # The control has to succeed, not merely stay quiet -- otherwise it would still pass if
        # MISE_CD broke the run for some entirely different reason.
        $status | Should -Be 0
    }
}
