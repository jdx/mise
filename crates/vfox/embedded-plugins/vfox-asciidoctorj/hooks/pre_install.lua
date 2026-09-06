local http = require("http")

--- Returns pre-installed information, such as version number, download address, etc.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    local base_url = "https://repo1.maven.org/maven2/org/asciidoctor/asciidoctorj/"
    local download_url = base_url .. version .. "/asciidoctorj-" .. version .. "-bin.zip"

    -- Verify URL exists
    local resp = http.head({ url = download_url })
    if resp.status_code ~= 200 then
        error("Download URL not found: " .. download_url .. " (status: " .. resp.status_code .. ")")
    end

    return {
        version = version,
        url = download_url,
    }
end
