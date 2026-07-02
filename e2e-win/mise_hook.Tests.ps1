Describe 'mise_hook' {
    It 'doesn''t clobber $LASTEXITCODE' {
        mise activate pwsh | Out-String | Invoke-Expression
        cmd /C 'exit 12'
        # simulate interactive command execution
        prompt
        $LASTEXITCODE | Should -BeExactly 12
    }
}