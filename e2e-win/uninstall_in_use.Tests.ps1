Describe 'uninstall while the tool is running' {
    BeforeAll {
        $script:originalPath = Get-Location
        $script:originalEnv = @{}
        foreach ($name in 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_YES') {
            $script:originalEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }

        $script:testDir = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:testDir | Out-Null
        Set-Location $script:testDir
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:testDir
        $env:MISE_YES = '1'

        mise install jq@1.8.2 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "mise install jq failed with exit code $LASTEXITCODE"
        }
        $script:installDir = (mise where jq@1.8.2) | Select-Object -Last 1
        $script:jq = Join-Path $script:installDir 'jq.exe'

        # Hold the image mapped. jq blocks on stdin, so the file stays locked until it is killed.
        $psi = [Diagnostics.ProcessStartInfo]::new($script:jq, '.')
        $psi.RedirectStandardInput = $true
        $psi.UseShellExecute = $false
        $script:held = [Diagnostics.Process]::Start($psi)
        Start-Sleep -Milliseconds 300
    }

    AfterAll {
        # Unconditional: leaving it running would lock $TestDrive and break Pester's own cleanup.
        if ($script:held -and -not $script:held.HasExited) {
            $script:held.Kill()
            $script:held.WaitForExit()
        }
        Set-Location $script:originalPath
        foreach ($name in $script:originalEnv.Keys) {
            if ($null -eq $script:originalEnv[$name]) {
                Remove-Item -Path "Env:\$name" -ErrorAction SilentlyContinue
            } else {
                [Environment]::SetEnvironmentVariable($name, $script:originalEnv[$name], 'Process')
            }
        }
    }

    It 'says the file is in use rather than only "Access is denied"' {
        $script:held.HasExited | Should -BeFalse -Because 'the lock is the whole premise'

        $out = & mise uninstall jq@1.8.2 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -Match 'in use'
        # `rm -rf` is not a command a Windows reader has.
        $out | Should -Not -Match 'rm -rf'
        # The failure has to stay a failure: the tool is still on disk. Asserted on the directory
        # rather than `mise ls`, whose output matches "jq" for any other jq version the job has
        # installed.
        Test-Path -LiteralPath $script:installDir | Should -BeTrue
    }

    It 'succeeds once nothing is holding it' {
        # The control. Without it, "uninstall failed" would not show that the lock is what did it.
        $script:held.Kill()
        $script:held.WaitForExit()

        $out = & mise uninstall jq@1.8.2 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -Not -Match 'in use'
        Test-Path -LiteralPath $script:installDir | Should -BeFalse
    }
}
