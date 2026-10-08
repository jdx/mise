# Wait for Windows to release mapped fixture executables before Pester deletes
# TestDrive. Output completion does not guarantee executable file access yet.
function Wait-MiseFixtureExecutables {
    param([Parameter(Mandatory)][string]$Directory)

    foreach ($executable in Get-ChildItem -LiteralPath $Directory -Recurse -File -Filter '*.exe') {
        $elapsed = [System.Diagnostics.Stopwatch]::StartNew()
        while ($true) {
            try {
                $stream = [System.IO.File]::Open($executable.FullName, 'Open', 'ReadWrite', 'None')
                $stream.Dispose()
                break
            } catch [System.IO.IOException], [System.UnauthorizedAccessException] {
                $errorCode = $_.Exception.GetBaseException().HResult -band 0xffff
                if ($errorCode -notin @(5, 32, 33) -or $elapsed.Elapsed.TotalSeconds -ge 30) {
                    throw
                }
                Start-Sleep -Milliseconds 100
            }
        }
    }
}
