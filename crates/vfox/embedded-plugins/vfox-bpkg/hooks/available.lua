--- Map of release tag -> published_at (RFC 3339) from GitHub releases.
--- Tags without a published release are simply absent (no date reported).
local function release_dates(headers)
    local http = require("http")
    local json = require("json")
    local dates = {}
    for page = 1, 5 do
        local resp, err = http.try_get({
            url = "https://api.github.com/repos/bpkg/bpkg/releases?per_page=100&page=" .. page,
            headers = headers,
        })
        if err ~= nil or resp.status_code ~= 200 then
            break
        end
        local ok, releases = pcall(json.decode, resp.body)
        if not ok or type(releases) ~= "table" or #releases == 0 then
            break
        end
        for _, rel in ipairs(releases) do
            if rel.tag_name and rel.published_at and not rel.draft then
                dates[rel.tag_name] = rel.published_at
            end
        end
        if #releases < 100 then
            break
        end
    end
    return dates
end

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

    local dates = release_dates(headers)
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
            created_at = dates[tag.name],
        })
    end

    return result
end
