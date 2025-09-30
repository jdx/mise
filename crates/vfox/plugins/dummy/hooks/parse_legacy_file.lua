--- Parse the legacy file found by vfox to determine the version of the tool.
--- Useful to extract version numbers from files like JavaScript's package.json or Golangs go.mod.
function PLUGIN:ParseLegacyFile(ctx)
    local filename = ctx.filename
    if filename == nil then
        error("ctx.filename is nil")
    end
    if filename ~= ".dummy-version" then
        error("Expected filename to be .dummy-version, got " .. filename)
    end
    local filepath = ctx.filepath
    local file = require("file")
    local content = file.read(filepath)
    content = content:gsub("%s+", "")
    if content == "" then
        return {}
    end

    return {
        version = content,
    }
end
