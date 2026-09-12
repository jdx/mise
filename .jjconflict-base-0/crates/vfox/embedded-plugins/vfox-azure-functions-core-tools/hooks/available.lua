local http = require("http")
local json = require("json")

--- Get the available version list from GitHub releases.
--- @param ctx table Empty table, no data provided. Always {}.
--- @return table Version list
function PLUGIN:Available(ctx)
    local result = {}
    local page = 1
    local per_page = 100

    while true do
        local url = "https://api.github.com/repos/Azure/azure-functions-core-tools/releases?per_page="
            .. per_page
            .. "&page="
            .. page
        local resp = http.get({
            url = url,
            headers = {
                ["Accept"] = "application/vnd.github.v3+json",
            },
        })

        if resp.status_code ~= 200 then
            if page == 1 then
                error("Failed to fetch releases from GitHub: " .. resp.status_code)
            end
            break
        end

        local releases = json.decode(resp.body)
        if #releases == 0 then
            break
        end

        for _, release in ipairs(releases) do
            if not release.prerelease and not release.draft then
                local version = release.tag_name
                table.insert(result, {
                    version = version,
                    note = release.name or "",
                })
            end
        end

        page = page + 1
        -- Limit to avoid too many API calls
        if page > 5 then
            break
        end
    end

    return result
end
