--- Returns all available versions of ag from GitHub tags
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    local result = {}
    local page = 1

    -- Get GitHub token from environment for rate limiting
    local github_token = os.getenv("GITHUB_TOKEN") or os.getenv("GH_TOKEN")
    local headers = {
        ["Accept"] = "application/vnd.github.v3+json",
    }
    if github_token and github_token ~= "" then
        headers["Authorization"] = "token " .. github_token
    end

    while true do
        local resp, err = http.get({
            url = "https://api.github.com/repos/ggreer/the_silver_searcher/tags?per_page=100&page=" .. page,
            headers = headers,
        })

        if err ~= nil then
            error("Failed to fetch tags: " .. err)
        end

        if resp.status_code ~= 200 then
            error("Failed to fetch tags, status: " .. resp.status_code)
        end

        local tags = json.decode(resp.body)
        if tags == nil or #tags == 0 then
            break
        end

        for _, tag in ipairs(tags) do
            local version = tag.name
            table.insert(result, {
                version = version,
            })
        end

        -- If we got less than 100 results, we've reached the end
        if #tags < 100 then
            break
        end

        page = page + 1
    end

    return result
end
