Describe 'clang' {
    It 'executes clang via conda backend' {
        mise x conda:clang@21.1.7 -- clang --version | Should -Match "clang version 21"
    }
}
