Describe 'activate keeps $Error clean' {
    # The deactivation preamble that `activate` prepends removes `function:mise` and
    # $global:__mise_pwsh_chpwd_handled so that re-activating is idempotent. A session
    # that has only inherited __MISE_DIFF has neither yet, and -ErrorAction
    # SilentlyContinue keeps the two failures off the screen while still appending them
    # to $Error - so the shell started with two entries the user had not caused.
    #
    # Two levels, because the preamble is only emitted when __MISE_DIFF is set: the
    # outer shell activates to produce it, the inner one is the fresh session that used
    # to start dirty. -NoProfile so a developer profile cannot pre-activate and define
    # the very names under test.
    It 'adds nothing to $Error when activating with __MISE_DIFF inherited' {
        $probe = Join-Path $TestDrive 'error_hygiene_probe.ps1'
        Set-Content -LiteralPath $probe -Value @'
$Error.Clear()
mise activate pwsh | Out-String | Invoke-Expression
# without this the count below would pass even if Invoke-Expression never ran the script
if (-not (Get-Command mise -CommandType Function -ErrorAction Ignore)) {
    Write-Output 'ACTIVATION-ERROR: activation did not define the mise function'
    exit 1
}
foreach ($e in $Error) { Write-Output "ERROR-RECORD: $($e.Exception.Message)" }
Write-Output "ERROR-COUNT: $($Error.Count)"
'@
        $outer = @"
mise activate pwsh | Out-String | Invoke-Expression
if (-not (Test-Path Env:/__MISE_DIFF)) {
    Write-Output 'SETUP-ERROR: activation left __MISE_DIFF unset, so the child gets no deactivation preamble'
    exit 1
}
& pwsh -NoProfile -NonInteractive -File '$probe'
"@
        $out = pwsh -NoProfile -NonInteractive -Command $outer 2>&1 | Out-String

        $out | Should -Not -Match 'SETUP-ERROR'
        $out | Should -Not -Match 'ACTIVATION-ERROR'
        $out | Should -Not -Match 'ERROR-RECORD'
        $out | Should -Match 'ERROR-COUNT: 0'
    }
}
