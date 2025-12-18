-- hooks/available.lua
function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    -- GitHub API URL for releases
    local repo_url = "https://api.github.com/repos/fsaintjacques/semver-tool/tags"

    -- Prepare headers
    local headers = {}
    if os.getenv("GITHUB_TOKEN") then
        headers["Authorization"] = "token " .. os.getenv("GITHUB_TOKEN")
    elseif os.getenv("GITHUB_API_TOKEN") then
        headers["Authorization"] = "token " .. os.getenv("GITHUB_API_TOKEN")
    end

    -- Make API request
    local resp, err = http.get({
        url = repo_url,
        headers = headers,
    })

    if err ~= nil then
        error("Failed to fetch versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("GitHub API returned status " .. resp.status_code .. ": " .. resp.body)
    end

    -- Parse response
    local tags = json.decode(resp.body)
    local result = {}

    -- Convert tags to versions
    for i, tag_info in ipairs(tags) do
        local version = tag_info.name

        -- Add version to result
        table.insert(result, {
            version = version,
            note = nil,
        })
    end

    return result
end
