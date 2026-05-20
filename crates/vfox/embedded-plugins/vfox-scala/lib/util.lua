local util = {}

util.SEARCH_URL = "https://www.scala-lang.org/download/all.html"
util.SCALA3_DOWNLOAD_URL = "https://github.com/scala/scala3/releases/download/%s/scala3-%s.%s"
util.SCALA2_DOWNLOAD_URL_2104_UP = "https://scala-lang.org/files/archive/scala-%s.%s"
util.SCALA2_DOWNLOAD_URL_250_UP = "https://scala-lang.org/files/archive/scala-%s.%s"

function util:compare_versions(v1o, v2o)
    local v1 = v1o.version
    local v2 = v2o.version
    local v1_parts = {}
    for part in string.gmatch(v1, "[^.]+") do
        table.insert(v1_parts, tonumber(part))
    end

    local v2_parts = {}
    for part in string.gmatch(v2, "[^.]+") do
        table.insert(v2_parts, tonumber(part))
    end

    for i = 1, math.max(#v1_parts, #v2_parts) do
        local v1_part = v1_parts[i] or 0
        local v2_part = v2_parts[i] or 0
        if v1_part > v2_part then
            return true
        elseif v1_part < v2_part then
            return false
        end
    end

    return false
end

function util:getDownloadUrl(version)
    local suffixType = ""
    local downloadUrl = ""
    if RUNTIME.osType == "windows" then
        suffixType = "zip"
    else
        if
            util:compare_versions({ version = "2.7.1" }, { version = version })
            or util:compare_versions({ version = version }, { version = "3.0.0" })
        then
            suffixType = "tar.gz"
        else
            suffixType = "tgz"
        end
    end

    if
        util:compare_versions({ version = version }, { version = "2.5.0" })
        and util:compare_versions({ version = "2.10.4" }, { version = version })
    then
        if util:compare_versions({ version = "2.7.0" }, { version = version }) then
            version = version:gsub("%.final", "-final")
        end
        downloadUrl = util.SCALA2_DOWNLOAD_URL_250_UP:format(version, suffixType)
    elseif util:compare_versions({ version = "3.0.0" }, { version = version }) then
        downloadUrl = util.SCALA2_DOWNLOAD_URL_2104_UP:format(version, suffixType)
    else
        downloadUrl = util.SCALA3_DOWNLOAD_URL:format(version, version, suffixType)
    end

    return downloadUrl
end

return util
