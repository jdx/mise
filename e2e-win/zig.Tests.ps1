Describe 'zig' {
    It 'executes zig 0.13.0' {
        mise x zig@0.13.0 -- zig version | Should -be "0.13.0"
    }
}
