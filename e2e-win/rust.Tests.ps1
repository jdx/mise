
Describe 'node' {
    It 'executes rust 1.82.0' {
        mise x rust@1.82.0 -- rustc -V | Should -BeLike "rustc 1.82.0*"
    }
}
