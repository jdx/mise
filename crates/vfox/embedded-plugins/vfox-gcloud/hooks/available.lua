--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions

local http = require("http")
local json = require("json")

-- GCS bucket configuration
local GCS_BUCKET = "cloud-sdk-release"
local GCS_PREFIX = "google-cloud-sdk"
local GCS_API_URL = "https://storage.googleapis.com/storage/v1/b/" .. GCS_BUCKET .. "/o"

-- Cache for versions
local available_versions = nil

--- Fetch versions from GCS with pagination
local function fetch_versions()
    local versions = {}
    local seen = {}
    local page_token = nil

    repeat
        -- Build URL with query parameters
        local url = GCS_API_URL .. "?prefix=" .. GCS_PREFIX .. "&fields=kind,nextPageToken,items(name)"
        if page_token then
            url = url .. "&pageToken=" .. page_token
        end

        -- Fetch page
        local resp, err = http.get({
            url = url,
        })

        if err ~= nil or resp.status_code ~= 200 then
            error("Failed to fetch versions from GCS: " .. (err or "status " .. resp.status_code))
        end

        local data = json.decode(resp.body)

        -- Extract versions from object names
        -- Pattern: google-cloud-sdk-{version}-linux-x86_64.tar.gz
        for _, item in ipairs(data.items or {}) do
            local name = item.name
            local version = string.match(name, "^google%-cloud%-sdk%-([%d%.]+)%-linux%-x86_64%.tar%.gz$")
            if version and not seen[version] then
                seen[version] = true
                table.insert(versions, version)
            end
        end

        page_token = data.nextPageToken
    until page_token == nil

    return versions
end

--- Compare version strings for sorting
local function compare_versions(a, b)
    local function parse_version(v)
        local parts = {}
        for part in string.gmatch(v, "([^%.]+)") do
            table.insert(parts, tonumber(part) or 0)
        end
        return parts
    end

    local va = parse_version(a)
    local vb = parse_version(b)

    for i = 1, math.max(#va, #vb) do
        local na = va[i] or 0
        local nb = vb[i] or 0
        if na ~= nb then
            return na < nb
        end
    end
    return false
end

function PLUGIN:Available(ctx)
    -- Use cached versions if available
    if available_versions ~= nil then
        return available_versions
    end

    local versions = fetch_versions()

    -- Sort versions in ascending order
    table.sort(versions, compare_versions)

    -- Build result table
    local result = {}
    for _, version in ipairs(versions) do
        table.insert(result, {
            version = version,
            note = "",
        })
    end

    available_versions = result
    return result
end
