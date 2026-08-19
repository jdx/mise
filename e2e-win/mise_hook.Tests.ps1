Describe 'mise_hook' {
    BeforeAll {
        mise activate pwsh | Out-String | Invoke-Expression
    }

    AfterAll {
        mise deactivate
    }

    It 'doesn''t clobber $LASTEXITCODE' {
        cmd /C 'exit 12'
        # simulate interactive command execution
        prompt
        $LASTEXITCODE | Should -BeExactly 12
    }

    It 'runs hook-env when MISE state changes after a directory change' {
        $originalPath = $PWD
        $originalMiseConfigFile = [Environment]::GetEnvironmentVariable('MISE_CONFIG_FILE', 'Process')
        $config = Join-Path $TestDrive 'mise.toml'
        @('[env]', 'PWSH_POST_CHPWD = "applied"') | Set-Content $config

        try {
            Set-Location TestDrive:
            $env:MISE_CONFIG_FILE = $config
            prompt | Out-Null

            $env:PWSH_POST_CHPWD | Should -BeExactly 'applied'
        } finally {
            Set-Location $originalPath
            if ($null -eq $originalMiseConfigFile) {
                Remove-Item Env:MISE_CONFIG_FILE -ErrorAction SilentlyContinue
            } else {
                $env:MISE_CONFIG_FILE = $originalMiseConfigFile
            }
            prompt | Out-Null
        }
    }
}
