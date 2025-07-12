
PLUGIN = {
    name = "test-backend",
    version = "1.0.0",
    description = "Test plugin with backend support",
    author = "Test",
    license = "MIT",
    backendEnabled = true,
    backendName = "test-backend",
    
    backend_list_versions = function(ctx)
        return {
            versions = {"1.0.0", "1.1.0", "2.0.0"}
        }
    end,
    
    backend_install = function(ctx)
        return {
            success = true,
            message = "Installation successful"
        }
    end,
    
    backend_exec_env = function(ctx)
        return {
            env_vars = {
                {key = "TEST_BACKEND_ROOT", value = ctx.install_path},
                {key = "PATH", value = ctx.install_path .. "/bin"}
            }
        }
    end,
    
    backend_uninstall = function(ctx)
        return {
            success = true,
            message = "Uninstallation successful"
        }
    end
}
