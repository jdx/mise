local http = require("http")

local util = {}

local base_url = "https://repo.maven.apache.org/maven2/org/springframework/boot/spring-boot-cli"

local function split_version(version)
  local nums = {}
  local base = version:match("^([^-]+)") or version
  for n in base:gmatch("%d+") do
    table.insert(nums, tonumber(n))
  end
  local milestone = version:match("%-M(%d+)$")
  return nums, milestone and tonumber(milestone) or nil
end

local function version_gt(a, b)
  local av, am = split_version(a.version)
  local bv, bm = split_version(b.version)
  for i = 1, math.max(#av, #bv) do
    local ai = av[i] or 0
    local bi = bv[i] or 0
    if ai ~= bi then
      return ai > bi
    end
  end

  if am == nil and bm ~= nil then
    return true
  end
  if am ~= nil and bm == nil then
    return false
  end
  if am ~= nil and bm ~= nil then
    return am > bm
  end
  return false
end

function util.get_versions()
  local resp, err = http.get({
    url = base_url .. "/",
  })
  if err ~= nil then
    error("failed to fetch Spring Boot CLI versions: " .. err)
  end
  if resp.status_code ~= 200 then
    error("failed to fetch Spring Boot CLI versions: status " .. resp.status_code)
  end

  local versions = {}
  for version in resp.body:gmatch('href="([0-9][^"/]+)/"') do
    table.insert(versions, { version = version })
  end

  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  return base_url
    .. "/"
    .. version
    .. "/spring-boot-cli-"
    .. version
    .. "-bin.tar.gz"
end

return util
