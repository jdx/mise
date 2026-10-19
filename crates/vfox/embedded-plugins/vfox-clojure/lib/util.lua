local http = require("http")
local json = require("json")

local util = {}

local function split_version(version)
  local parts = {}
  for n in version:gmatch("%d+") do
    table.insert(parts, tonumber(n))
  end
  return parts
end

local function version_gt(a, b)
  local av = split_version(a.version)
  local bv = split_version(b.version)
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
    url = "https://api.github.com/repos/clojure/brew-install/tags?per_page=100",
  })
  if err ~= nil then
    error("failed to fetch Clojure tool tags: " .. err)
  end
  if resp.status_code ~= 200 then
    error("failed to fetch Clojure tool tags: status " .. resp.status_code)
  end

  local versions = {}
  for _, tag in ipairs(json.decode(resp.body)) do
    local version = tag.name:match("^(%d+%.%d+%.%d+%.%d+)$")
    if version then
      table.insert(versions, { version = version })
    end
  end

  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  return "https://download.clojure.org/install/clojure-tools-" .. version .. ".tar.gz"
end

return util
