Describe 'vfox' {
    It 'executes vfox backend command execution' {
        # Test that vfox backend can execute commands cross-platform
        # This tests the cmd.exec function that was fixed for Windows compatibility
        $result = mise x vfox:version-fox/vfox-nodejs -- node -v
        $result | Should -Not -BeNullOrEmpty
        $result | Should -Match "v\d+\.\d+\.\d+"
    }

    It 'installs and uses vfox plugin' {
        # Test installing a vfox plugin and using it
        $pluginName = "vfox-test-plugin"
        
        # Clean up any existing test plugin
        mise plugins uninstall $pluginName -ErrorAction SilentlyContinue
        
        # Test that we can list available vfox plugins
        $plugins = mise registry | Select-String "vfox:"
        $plugins | Should -Not -BeNullOrEmpty
        
        # Test installing a specific version using a working vfox plugin
        $result = mise install vfox:version-fox/vfox-nodejs@24.4.0
        # The install result might be empty but the tool should still work
        
        # Test using the installed version
        $version = mise x vfox:version-fox/vfox-nodejs@24.4.0 -- node -v
        $version | Should -Be "v24.4.0"
    }

    It 'handles vfox plugin:tool format' {
        # Test the plugin:tool format that uses backend methods
        $result = mise x vfox:version-fox/vfox-nodejs -- node --version
        $result | Should -Not -BeNullOrEmpty
        $result | Should -Match "v\d+\.\d+\.\d+"
    }

    It 'installs and uses vfox-npm plugin' {
        # Test installing and using the vfox-npm plugin
        # First, ensure the vfox-npm plugin is installed
        mise plugin install -f vfox-npm https://github.com/jdx/vfox-npm
        
        # Test installing a specific npm tool through vfox-npm
        mise install vfox-npm:prettier@3.0.0 --debug
        # The install result might be empty but the tool should still work
        
        # Test using the installed npm tool
        $which = mise where vfox-npm:prettier@3.0.0
        $which | Should -Match "vfox-npm-prettier"
        $version = mise x vfox-npm:prettier@3.0.0 -- prettier --version
        $version | Should -Be "3.0.0"
        
        # Test that the tool is available in the current environment
        $prettierVersion = mise x vfox-npm:prettier -- prettier --version
        $prettierVersion | Should -Not -BeNullOrEmpty
        $prettierVersion | Should -Match "\d+\.\d+\.\d+"
    }
} 
