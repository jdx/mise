Describe 'mise activate pwsh pipeline input' {
    BeforeAll {
        $originalPath = $PWD
        $originalMiseConfigFile = [Environment]::GetEnvironmentVariable('MISE_CONFIG_FILE', 'Process')
        $config = Join-Path $TestDrive 'mise.toml'
        $reader = Join-Path $TestDrive 'read-stdin.ps1'

        '[Console]::In.ReadToEnd()' | Set-Content $reader
        @(
            '[tasks.read-stdin]'
            "run = 'pwsh -NoProfile -File `"$reader`"'"
        ) | Set-Content $config

        Set-Location TestDrive:
        $env:MISE_CONFIG_FILE = $config
        mise activate pwsh | Out-String | Invoke-Expression
    }

    AfterAll {
        mise deactivate
        Set-Location $originalPath
        if ($null -eq $originalMiseConfigFile) {
            Remove-Item Env:MISE_CONFIG_FILE -ErrorAction SilentlyContinue
        } else {
            $env:MISE_CONFIG_FILE = $originalMiseConfigFile
        }
    }

    It 'forwards pipeline input to a task' {
        $output = 'data' | mise run read-stdin 2>&1 | Out-String

        $LASTEXITCODE | Should -BeExactly 0
        $output | Should -Match '(?m)^\uFEFF?data\r?$'
        $output | Should -Not -Match 'cannot be bound to any parameters'
    }
}
