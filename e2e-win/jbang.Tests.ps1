Describe 'jbang' {
    It 'installs and executes jbang via the registry aqua backend' {
        mise x jbang@0.138.0 -- jbang --version | Should -BeLike "0.138.0*"
    }
}
