--- Cloud SDK component helpers shared by PostInstall and MiseInstallSatisfied.

local file = require("file")
local strings = require("strings")

local M = {}

local function add(list, seen, invalid, name)
    local trimmed = strings.trim_space(name)
    if trimmed == "" or seen[trimmed] then
        return
    end
    if not string.match(trimmed, "^[%w%-_%.]+$") then
        table.insert(invalid, trimmed)
        return
    end
    seen[trimmed] = true
    table.insert(list, trimmed)
end

--- Component IDs from the `components` tool option, e.g.
---   gcloud = { version = "latest", components = ["alpha", "beta"] }
--- A string is split on commas and whitespace so it also works from the CLI.
--- Returns the valid IDs and any names that were skipped as invalid.
--- @param options table|nil Tool options
--- @return string[] components
--- @return string[] invalid
function M.from_options(options)
    local list, seen, invalid = {}, {}, {}
    local value = options and options.components
    if type(value) == "table" then
        for _, name in ipairs(value) do
            add(list, seen, invalid, tostring(name))
        end
    elseif type(value) == "string" then
        for name in string.gmatch(value, "[^,%s]+") do
            add(list, seen, invalid, name)
        end
    elseif value ~= nil then
        error("components must be an array of component IDs, got: " .. type(value))
    end
    return list, invalid
end

--- Component IDs listed one per line in a `.default-cloud-sdk-components` file.
--- @param contents string File contents
--- @return string[] components
--- @return string[] invalid
function M.from_file_contents(contents)
    local list, seen, invalid = {}, {}, {}
    for _, line in ipairs(strings.split(contents, "\n")) do
        local trimmed = strings.trim_space(line)
        if trimmed ~= "" and not string.find(trimmed, "^#") then
            add(list, seen, invalid, trimmed)
        end
    end
    return list, invalid
end

--- gcloud records each installed component as `.install/<id>.manifest`.
--- @param sdk_path string SDK root
--- @param id string Component ID
--- @return boolean
function M.is_installed(sdk_path, id)
    return file.exists(file.join_path(sdk_path, ".install", id .. ".manifest"))
end

--- The components in `list` that are not installed under `sdk_path`.
--- @param sdk_path string SDK root
--- @param list string[] Component IDs
--- @return string[]
function M.missing(sdk_path, list)
    local missing = {}
    for _, id in ipairs(list) do
        if not M.is_installed(sdk_path, id) then
            table.insert(missing, id)
        end
    end
    return missing
end

return M
