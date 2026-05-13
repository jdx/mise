Describe 'jbang' {
    It 'installs and executes jbang via the registry aqua backend' {
        $output = mise x jbang@0.138.0 -- jbang --version 2>&1 | Out-String
        $output | Should -Match "0\.138\.0"
    }
}
