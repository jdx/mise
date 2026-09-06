Describe 'activate under Set-StrictMode' {
    # https://github.com/jdx/mise/discussions/5312 - the activation script read
    # globals that were never assigned, which Set-StrictMode turns into
    # terminating errors, so `mise activate pwsh | Invoke-Expression` aborted.
    #
    # This must run in a child process: mise_hook.Tests.ps1 activates in the
    # shared Pester runspace, and `mise deactivate` does not remove the
    # $Global:__mise_pwsh_* sentinels - so an in-process activation here would
    # skip the very branches under test and pass regardless. -NoProfile keeps a
    # developer profile from pre-activating for the same reason.
    It 'activates and runs a bare `mise` without StrictMode errors' {
        $probe = @'
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Continue'
mise activate pwsh | Out-String | Invoke-Expression
# without this the calls below would fall through to the native executable and
# pass even if Invoke-Expression never defined the wrapper
if (-not (Get-Command mise -CommandType Function -ErrorAction SilentlyContinue)) {
    Write-Output 'ACTIVATION-ERROR: activation did not define the mise function'
    exit 1
}
mise --version | Out-Null
mise | Out-Null
Set-Location $env:TEMP
# the command-not-found hook inspects PSReadLine history, which is unavailable
# here - it must stay quiet instead of erroring on every unknown command
try { some-command-that-does-not-exist-xyz } catch { }
# deactivate removes the marker while the prompt wrapper remains installed;
# rendering the next prompt must not read the removed global under StrictMode
mise deactivate
prompt | Out-Null
# only StrictMode/handler violations count - mise's own warnings and the shell's
# own "not recognized" error must not fail this
$pattern = 'has not been set|cannot be found on this object|Cannot index into a null array|Unable to find type|Index was outside the bounds'
$strict = $Error | Where-Object { $_.Exception.Message -match $pattern }
if ($strict) {
    foreach ($e in $strict) { Write-Output "STRICTMODE-ERROR: $($e.Exception.Message)" }
    exit 1
}
Write-Output 'STRICTMODE-OK'
'@
        $out = pwsh -NoProfile -NonInteractive -Command $probe 2>&1 | Out-String

        $out | Should -Not -Match 'has not been set'
        $out | Should -Not -Match 'cannot be found on this object'
        $out | Should -Not -Match 'Unable to find type'
        $out | Should -Match 'STRICTMODE-OK'
        $LASTEXITCODE | Should -Be 0
    }
}
