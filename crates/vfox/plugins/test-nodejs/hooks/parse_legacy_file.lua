--- Parse the legacy file to extract the version
--- @param ctx table See /vfox/ctx.md#ctx-hooks for more information on ctx
--- @return table Version information
function PLUGIN:ParseLegacyFile(ctx)
    local filepath = ctx.filepath
    local file = io.open(filepath, "r")
    if file == nil then
        return {}
    end
    local content = file:read("*a")
    file:close()

    -- Remove any leading/trailing whitespace and 'v' prefix
    content = content:gsub("%s+", "")
    content = content:gsub("^v", "")

    if content == "" then
        return {}
    end

    return {
        version = content
    }
end