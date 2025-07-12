Describe 'vfox' {
    It 'executes vfox backend command execution' {
        # Test that vfox backend can execute commands cross-platform
        # This tests the cmd.exec function that was fixed for Windows compatibility
        $result = mise x vfox:version-fox/vfox-node -- node -v
        $result | Should -Not -BeNullOrEmpty
        $result | Should -Match "v\d+\.\d+\.\d+"
    }

    It 'installs and uses vfox plugin' {
        # Test installing a vfox plugin and using it
        $pluginName = "vfox-test-plugin"
        
        # Clean up any existing test plugin
        mise plugins unlink $pluginName -ErrorAction SilentlyContinue
        
        # Test that we can list available vfox plugins
        $plugins = mise registry | Select-String "vfox:"
        $plugins | Should -Not -BeNullOrEmpty
        
        # Test installing a specific version
        $result = mise install vfox:version-fox/vfox-node@22.0.0
        $result | Should -Not -BeNullOrEmpty
        
        # Test using the installed version
        $version = mise x vfox:version-fox/vfox-node@22.0.0 -- node -v
        $version | Should -Be "v22.0.0"
    }

    It 'handles vfox plugin:tool format' {
        # Test the plugin:tool format that uses backend methods
        $result = mise x vfox:version-fox/vfox-node -- node --version
        $result | Should -Not -BeNullOrEmpty
        $result | Should -Match "v\d+\.\d+\.\d+"
    }
} 
