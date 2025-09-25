--- Parse the legacy file found by vfox to determine the version of the tool.
--- Useful to extract version numbers from files like JavaScript's package.json or Golangs go.mod.
function PLUGIN:ParseLegacyFile(ctx)
    local filepath = ctx.filepath
    local file = io.open(filepath, "r")
    local content = file:read("*a")
    file:close()
    content = content:gsub("%s+", "")
    if content == "" then
        return {}
    end

    return {
        version = content
    }
end