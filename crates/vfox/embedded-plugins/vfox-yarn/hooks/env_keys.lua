--- Environment keys hook

function PLUGIN:EnvKeys(ctx)
    -- Get the SDK installation path
    local version_path = ctx.path
    
    -- Return the PATH configuration for yarn binaries
    return {
        {
            key = "PATH",
            value = version_path .. "/bin"
        }
    }
end

return PLUGIN