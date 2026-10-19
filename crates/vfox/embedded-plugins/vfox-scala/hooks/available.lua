local util = require("util")
local http = require("http")
local json = require("json")

--- Merge GitHub release dates (tag -> published_at) for a repo into `dates`.
--- Only published releases are listed, so older versions may have no date.
local function add_release_dates(dates, repo, prefix)
    for page = 1, 5 do
        local resp, err = http.try_get({
            url = "https://api.github.com/repos/" .. repo .. "/releases?per_page=100&page=" .. page,
        })
        if err ~= nil or resp.status_code ~= 200 then
            return
        end
        local ok, releases = pcall(json.decode, resp.body)
        if not ok or type(releases) ~= "table" or #releases == 0 then
            return
        end
        for _, rel in ipairs(releases) do
            if rel.tag_name and rel.published_at and not rel.draft then
                local tag = rel.tag_name
                if prefix ~= "" and tag:sub(1, #prefix) == prefix then
                    tag = tag:sub(#prefix + 1)
                end
                dates[tag] = rel.published_at
            end
        end
        if #releases < 100 then
            return
        end
    end
end

--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions
function PLUGIN:Available(ctx)
    local resp, err = http.get({
        url = util.SEARCH_URL,
    })
    if err ~= nil or resp.status_code ~= 200 then
        return {}
    end

    local htmlBody = resp.body
    local htmlContent = [[]] .. htmlBody .. [[]]
    local versions = {}

    for version in htmlContent:gmatch('<a href="/download/([^"]-)%.html">Scala%s+[^<]-</a>') do
        if util:isVersion(version) then
            table.insert(versions, version)
        end
    end

    local dates = {}
    add_release_dates(dates, "scala/scala3", "")
    add_release_dates(dates, "scala/scala", "v")

    local result = {}
    for _, version in ipairs(versions) do
        table.insert(result, { version = version, note = "", created_at = dates[version] })
    end

    return result
end
