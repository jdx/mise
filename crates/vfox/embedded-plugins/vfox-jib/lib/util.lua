local http = require("http")
local json = require("json")

local util = {}

local function parse_version(version)
  local parts = {}
  for part in version:gmatch("%d+") do
    table.insert(parts, tonumber(part))
  end
  return parts
end

local function version_gt(a, b)
  local av = parse_version(a.version)
  local bv = parse_version(b.version)
  for i = 1, math.max(#av, #bv) do
    local ai = av[i] or 0
    local bi = bv[i] or 0
    if ai ~= bi then
      return ai > bi
    end
  end
  return false
end

function util.get_versions()
  local resp, err = http.get({
    url = "https://api.github.com/repos/GoogleContainerTools/jib/releases?per_page=100",
  })
  if err ~= nil then
    error("failed to fetch Jib releases: " .. err)
  end
  if resp.status_code ~= 200 then
    error("failed to fetch Jib releases: status " .. resp.status_code)
  end

  local versions = {}
  for _, release in ipairs(json.decode(resp.body)) do
    local version = release.tag_name:match("^v(.+)%-cli$")
    if version then
      table.insert(versions, { version = version })
    end
  end

  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  return "https://github.com/GoogleContainerTools/jib/releases/download/v"
    .. version
    .. "-cli/jib-jre-"
    .. version
    .. ".zip"
end

return util
