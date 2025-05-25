Describe 'zig' {
    It 'executes zig 0.13.0' {
        mise x zig@0.13.0 -- zig version | Should -be "0.13.0"
    }

    It 'executes zig 2024.11.0-mach' {
        mise x zig@2024.11.0-mach -- zig version | Should -be "0.14.0-dev.2577+271452d22"
    }
}
