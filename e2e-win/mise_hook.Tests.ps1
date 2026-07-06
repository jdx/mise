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
}
