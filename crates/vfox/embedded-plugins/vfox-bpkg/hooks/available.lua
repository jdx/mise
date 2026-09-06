--- Returns all available versions of bpkg from GitHub releases
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    local result = {}

    -- Get GitHub token from environment if available
    local github_token = os.getenv("GITHUB_TOKEN") or os.getenv("GH_TOKEN")
    local headers = {
        ["Accept"] = "application/vnd.github.v3+json",
    }
    if github_token and github_token ~= "" then
        headers["Authorization"] = "token " .. github_token
    end

    local resp, err = http.get({
        url = "https://api.github.com/repos/bpkg/bpkg/tags",
        headers = headers,
    })

    if err ~= nil then
        error("Failed to fetch versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("Failed to fetch versions, status: " .. resp.status_code)
    end

    local tags = json.decode(resp.body)
    if tags == nil then
        return result
    end

    for _, tag in ipairs(tags) do
        local version = tag.name
        -- Remove 'v' prefix if present
        if version:sub(1, 1) == "v" then
            version = version:sub(2)
        end
        table.insert(result, {
            version = version,
        })
    end

    return result
end
