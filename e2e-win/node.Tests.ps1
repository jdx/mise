
Describe 'node' {
    It 'executes node 22.0.0' {
        mise x node@22.0.0 -- node -v | Should -be "v22.0.0"
    }
}
