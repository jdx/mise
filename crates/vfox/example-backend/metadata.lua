-- Plugin metadata
PLUGIN = {
    name = "example-backend",
    version = "1.0.0",
    description = "Example vfox plugin with custom backend support",
    author = "mise team",
    license = "MIT",
    legacyFilenames = {".example-version"},
    backendEnabled = true,
    backendName = "example",
    
    -- Custom backend operations
    backend_list_versions = function(ctx)
        -- Custom implementation to list versions
        -- This would typically fetch from a remote API or registry
        return {
            versions = {"1.0.0", "1.1.0", "1.2.0", "2.0.0", "2.1.0"}
        }
    end,
    
    backend_install = function(ctx)
        -- Custom implementation to install a version
        -- This would typically download and install the tool
        local install_path = ctx.install_path
        local version = ctx.version
        
        -- Create directories
        os.execute("mkdir -p " .. install_path .. "/bin")
        
        -- Create a simple executable for demonstration
        local exec_content = string.format([[#!/bin/bash
echo "Example tool version %s"
echo "Installed at: %s"
]], version, install_path)
        
        local exec_file = install_path .. "/bin/example"
        local file = io.open(exec_file, "w")
        if file then
            file:write(exec_content)
            file:close()
            os.execute("chmod +x " .. exec_file)
        end
        
        return {
            success = true,
            message = "Successfully installed example tool " .. version
        }
    end,
    
    backend_exec_env = function(ctx)
        -- Custom implementation to set environment variables
        local install_path = ctx.install_path
        local version = ctx.version
        
        return {
            env_vars = {
                {key = "EXAMPLE_ROOT", value = install_path},
                {key = "EXAMPLE_VERSION", value = version},
                {key = "PATH", value = install_path .. "/bin"}
            }
        }
    end,
    
    backend_uninstall = function(ctx)
        -- Custom implementation to uninstall a version
        local install_path = ctx.install_path
        os.execute("rm -rf " .. install_path)
        
        return {
            success = true,
            message = "Successfully uninstalled example tool"
        }
    end
} 
