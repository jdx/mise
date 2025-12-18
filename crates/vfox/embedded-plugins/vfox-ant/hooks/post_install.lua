--- Extension point, called after PreInstall, can perform additional operations,
--- such as file operations for the SDK installation directory
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
    -- The tarball extracts to apache-ant-{version}/ directory
    -- mise should handle this automatically
end
