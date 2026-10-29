Describe 'path-env' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        # Not the checkout: its config has dozens of tools, and other suites change which of them
        # count as installed (a `MISE_NPM_PACKAGE_MANAGER` run relocks the npm ones), so one of
        # these runs could see a tool the run before it did not. Own `_.path` entries still make
        # mise set PATH for the child, which is what this compares.
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        $dirs = 'one', 'two', 'three' | ForEach-Object {
            (New-Item -ItemType Directory -Path (Join-Path $script:TestRoot $_)).FullName
        }
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        @(
            '[env]'
            "_.path = [$(($dirs | ForEach-Object { "'$_'" }) -join ', ')]"
        ) | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")
        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'mise x produces the same path environment for successive runs' {
        $paths = 1..10 | ForEach-Object {
            $output = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim()
            $output
        }
        $paths | Should -HaveCount 10
        # The comparison below means nothing if mise did not set PATH at all.
        $paths[0] | Should -Match '\\three(;|$)'
        $paths | ForEach-Object {
            $_ | Should -Not -Match "ECHO is on."
            $_.Length | Should -BeExactly $paths[0].Length
            $_ | Should -BeExactly $paths[0]
        }
    }
}
