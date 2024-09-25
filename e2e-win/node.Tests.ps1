
Describe 'node' {
    It 'executes node 22.0.0' {
        mise x node@22.0.0 -- node -v
        mise x node@22.0.0 -- where node
        mise x node@22.0.0 -- set
        mise x node@22.0.0 -- node -v | Should -be "v22.0.0"
    }
}
