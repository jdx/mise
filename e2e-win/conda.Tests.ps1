Describe 'conda' {
    It 'executes ripgrep via conda backend' {
        mise x conda:ripgrep@14.1.0 -- rg --version | Out-String | Should -Match "ripgrep 14"
    }
}
