local util = {}
local http = require("http")

util.SEARCH_URL = "https://www.scala-lang.org/download/all.html"
util.SCALA3_DOWNLOAD_URL = "https://github.com/scala/scala3/releases/download/%s/scala3-%s.%s"
util.SCALA2_DOWNLOAD_PAGE_URL = "https://scala-lang.org/download/%s.html"

function util:isVersion(version)
    return version:match("^%d+%.%d+%.%d+[%w%.%-]*$") ~= nil
end

function util:findDownloadUrl(body, linkId)
    return body:match('id="#?' .. linkId .. '"[^>]-href="([^"]+)"')
        or body:match('href="([^"]+)"[^>]-id="#?' .. linkId .. '"')
end

function util:getArchiveSuffix()
    local suffixType = ""
    if RUNTIME.osType == "windows" then
        suffixType = "zip"
    else
        suffixType = "tar.gz"
    end

    return suffixType
end

function util:getScala2DownloadUrl(version)
    local resp, err = http.get({
        url = util.SCALA2_DOWNLOAD_PAGE_URL:format(version),
    })
    if err ~= nil or resp.status_code ~= 200 then
        error("failed to resolve Scala download page for " .. version)
    end

    local linkId = "link%-main%-unixsys"
    if RUNTIME.osType == "windows" then
        linkId = "link%-main%-windows"
    end

    local downloadUrl = util:findDownloadUrl(resp.body, linkId)
    if downloadUrl == nil and RUNTIME.osType == "windows" then
        downloadUrl = util:findDownloadUrl(resp.body, "link%-non%-main%-sys")
    end
    if downloadUrl == nil then
        error("failed to find Scala archive URL for " .. version)
    end

    if RUNTIME.osType == "windows" then
        downloadUrl = downloadUrl:gsub("%.msi$", ".zip")
    end

    return downloadUrl
end

function util:getDownloadUrl(version)
    if version:match("^3%.") then
        local suffixType = util:getArchiveSuffix()
        return util.SCALA3_DOWNLOAD_URL:format(version, version, suffixType)
    end

    return util:getScala2DownloadUrl(version)
end

return util
