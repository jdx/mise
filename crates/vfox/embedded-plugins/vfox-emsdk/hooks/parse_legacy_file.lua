function PLUGIN:ParseLegacyFile(ctx)
    local file = io.open(ctx.filepath, "r")
    if not file then
        return {}
    end
    local version = file:read("*l")
    file:close()
    if version then
        version = version:gsub("%s+", "")
    end
    if version and version ~= "" then
        return { version = version }
    end
    return {}
end
