--- Returns a list of available versions of Poetry
--- Fetches from GitHub releases

function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    -- Fetch releases from GitHub API
    local resp, err = http.get({
        url = "https://api.github.com/repos/python-poetry/poetry/tags?per_page=100",
    })
    if err ~= nil then
        error("Failed to fetch version list: " .. err)
    end
    if resp.status_code ~= 200 then
        error("Failed to fetch version list: HTTP " .. resp.status_code)
    end

    -- PyPI knows when each release was uploaded. A failure here only means
    -- no dates are reported; the version list still comes from the tags.
    local dates = {}
    local pypi, pypi_err = http.try_get({ url = "https://pypi.org/pypi/poetry/json" })
    if pypi_err == nil and pypi.status_code == 200 then
        local ok, data = pcall(json.decode, pypi.body)
        if ok and type(data) == "table" then
            for ver, files in pairs(data.releases or {}) do
                local first = files[1]
                if first and first.upload_time_iso_8601 then
                    dates[ver] = first.upload_time_iso_8601
                end
            end
        end
    end

    local tags = json.decode(resp.body)
    local versions = {}

    for _, tag in ipairs(tags) do
        local version = tag.name
        -- Remove 'v' prefix if present (though poetry doesn't use it)
        if version:sub(1, 1) == "v" then
            version = version:sub(2)
        end
        -- Only include stable versions (no alpha/beta/rc)
        if version:match("^%d+%.%d+%.%d+$") then
            table.insert(versions, {
                version = version,
                created_at = dates[version],
            })
        end
    end

    return versions
end
