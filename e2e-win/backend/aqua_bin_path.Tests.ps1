Describe 'backend_aqua' {
    It 'executes tree-sitter via aqua backend on Windows' {
        mise x aqua:tree-sitter/tree-sitter -- tree-sitter --version | Should -BeLike "tree-sitter *"
    }
}
