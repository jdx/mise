Describe 'self-update installs verified releases' {
    It 'replaces a disposable binary with modern and legacy releases that execute' {
        $mise = (Get-Command mise).Source
        python "$PSScriptRoot/../e2e/fixtures/self_update.py" $mise
        $LASTEXITCODE | Should -Be 0
    }
}
