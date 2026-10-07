local http = require("http")
local json = require("json")

local util = {}

local repo = "https://github.com/rstudio/tinytex-releases"

local function shell_quote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

local function run(cmd)
  local ok, reason, code = os.execute(cmd)
  if ok ~= true and ok ~= 0 then
    error("command failed (" .. tostring(code or reason) .. "): " .. cmd)
  end
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

function util.get_versions()
  local versions = {}
  for page = 1, 3 do
    local resp, err = http.get({
      url = "https://api.github.com/repos/rstudio/tinytex-releases/releases?per_page=100&page=" .. page,
    })
    if err ~= nil then
      error("failed to fetch TinyTeX releases: " .. err)
    end
    if resp.status_code ~= 200 then
      error("failed to fetch TinyTeX releases: status " .. resp.status_code)
    end

    local releases = json.decode(resp.body)
    if #releases == 0 then
      break
    end
    for _, release in ipairs(releases) do
      local version = release.tag_name:match("^v(.+)$")
      if version then
        table.insert(versions, { version = version })
      end
    end
  end

  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  local ext = "tar.gz"
  if OS_TYPE == "darwin" or OS_TYPE == "macos" then
    ext = "tgz"
  end
  return repo .. "/releases/download/v" .. version .. "/TinyTeX-v" .. version .. "." .. ext
end

function util.bin_path()
  if OS_TYPE == "darwin" or OS_TYPE == "macos" then
    return "bin/universal-darwin"
  end
  return "bin/x86_64-linux"
end

function util.verify_install(root)
  local tex = root .. "/" .. util.bin_path() .. "/tex"
  run("test -x " .. shell_quote(tex))
end

return util
