--- Returns all available PostgreSQL versions from the official FTP server
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")

    local result = {}
    local seen = {}

    -- Fetch the PostgreSQL source directory listing
    local resp, err = http.get({
        url = "https://ftp.postgresql.org/pub/source/",
    })

    if err ~= nil then
        error("Failed to fetch PostgreSQL versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("Failed to fetch PostgreSQL versions, status: " .. resp.status_code)
    end

    -- Parse HTML to extract version directories
    -- Format: >v17.2/<, >v16.6/<, >v9.6.24/<
    for version in string.gmatch(resp.body, '>v([0-9]+%.[0-9]+[%.0-9]*)/<') do
        if not seen[version] then
            seen[version] = true
            table.insert(result, {
                version = version,
            })
        end
    end

    return result
end
