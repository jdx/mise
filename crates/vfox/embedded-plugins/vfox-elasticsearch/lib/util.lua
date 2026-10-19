local http = require("http")
local json = require("json")

local util = {}

local function shell_quote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

local function run(cmd)
  local ok, reason, code = os.execute(cmd)
  if ok ~= true and ok ~= 0 then
    error("command failed (" .. tostring(code or reason) .. "): " .. cmd)
  end
end

local function exists(path)
  local f = io.open(path, "r")
  if f then
    f:close()
    return true
  end
  return false
end

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

local function version_gte(version, min_version)
  local av = split_version(version)
  local bv = split_version(min_version)
  for i = 1, math.max(#av, #bv) do
    local ai = av[i] or 0
    local bi = bv[i] or 0
    if ai ~= bi then
      return ai > bi
    end
  end
  return true
end

function util.get_versions()
  local versions = {}
  local seen = {}
  for page = 1, 5 do
    local resp, err = http.get({
      url = "https://api.github.com/repos/elastic/elasticsearch/tags?per_page=100&page=" .. page,
    })
    if err ~= nil then
      error("failed to fetch Elasticsearch tags: " .. err)
    end
    if resp.status_code ~= 200 then
      error("failed to fetch Elasticsearch tags: status " .. resp.status_code)
    end

    local tags = json.decode(resp.body)
    if #tags == 0 then
      break
    end
    for _, tag in ipairs(tags) do
      local version = tag.name:match("^v(%d+%.%d+%.%d+)$")
      if version and not seen[version] then
        seen[version] = true
        table.insert(versions, { version = version })
      end
    end
  end

  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  if not version_gte(version, "7.0.0") then
    if version_gte(version, "2.4.7") then
      return "https://artifacts.elastic.co/downloads/elasticsearch/elasticsearch-" .. version .. ".tar.gz"
    end
    return "https://download.elastic.co/elasticsearch/elasticsearch/elasticsearch-" .. version .. ".tar.gz"
  end

  local os_name = "linux"
  if OS_TYPE == "darwin" or OS_TYPE == "macos" then
    os_name = "darwin"
  end

  local arch = "x86_64"
  if ARCH_TYPE == "arm64" or ARCH_TYPE == "aarch64" then
    arch = "aarch64"
  end

  return "https://artifacts.elastic.co/downloads/elasticsearch/elasticsearch-"
    .. version
    .. "-"
    .. os_name
    .. "-"
    .. arch
    .. ".tar.gz"
end

function util.post_install(root, version)
  local extracted = root .. "/elasticsearch-" .. version
  if exists(extracted .. "/bin/elasticsearch") then
    run("find " .. shell_quote(extracted) .. " -name '*.exe' -delete")
    run("find " .. shell_quote(extracted) .. " -name '*.bat' -delete")
    run("cp -R " .. shell_quote(extracted .. "/.") .. " " .. shell_quote(root))
    run("rm -rf " .. shell_quote(extracted))
  end
  run("test -x " .. shell_quote(root .. "/bin/elasticsearch"))
end

return util
