--- Parse legacy version files like .node-version, .nvmrc, etc.
--- @param ctx table Context information
--- @field ctx.filepath string Path to the legacy file
--- @return table Version information
function PLUGIN:ParseLegacyFile(ctx)
    local filepath = ctx.filepath
    local content = io.open(filepath, "r")
    if content then
        local version = content:read("*line")
        content:close()
        if version then
            -- Remove any "v" prefix and trim whitespace
            version = version:gsub("^v", ""):match("^%s*(.-)%s*$")
            return {
                version = version
            }
        end
    end
    return {
        version = nil
    }
end 
