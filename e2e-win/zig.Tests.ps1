Describe 'zig' {
    It 'executes zig 0.13.0' {
        mise x zig@0.13.0 -- zig version | Should -be "0.13.0"
    }

    It 'executes zig 0.14.0-dev.2577+271452d22' {
        mise x zig@0.14.0-dev.2577+271452d22 -- zig version | Should -be "0.14.0-dev.2577+271452d22"
    }
}
