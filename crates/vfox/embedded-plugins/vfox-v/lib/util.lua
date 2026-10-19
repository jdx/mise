local http = require("http")
local json = require("json")

local util = {}

local repo = "https://github.com/vlang/v"

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

local function split_tag(tag)
  local weekly_year, weekly_week, weekly_patch = tag:match("^weekly%.(%d+)%.(%d+)%.?(%d*)$")
  if weekly_year then
    return {
      kind = "weekly",
      year = tonumber(weekly_year),
      week = tonumber(weekly_week),
      patch = tonumber(weekly_patch) or 0,
    }
  end

  local parts = {}
  for n in tag:gmatch("%d+") do
    table.insert(parts, tonumber(n))
  end
  return {
    kind = "release",
    parts = parts,
  }
end

local function version_gt(a, b)
  local av = split_tag(a.version)
  local bv = split_tag(b.version)

  if av.kind ~= bv.kind then
    return av.kind == "release"
  end

  if av.kind == "weekly" then
    if av.year ~= bv.year then
      return av.year > bv.year
    end
    if av.week ~= bv.week then
      return av.week > bv.week
    end
    return av.patch > bv.patch
  end

  for i = 1, math.max(#av.parts, #bv.parts) do
    local ai = av.parts[i] or 0
    local bi = bv.parts[i] or 0
    if ai ~= bi then
      return ai > bi
    end
  end
  return false
end

local function add_version(seen, versions, version)
  version = version:gsub("^v", "")
  if seen[version] then
    return
  end
  if version == "mytest" then
    return
  end
  if version:match("^%d+%.%d+%.%d+$") or version:match("^weekly%.%d+%.%d+%.?%d*$") then
    seen[version] = true
    table.insert(versions, { version = version })
  end
end

function util.get_versions()
  local versions = {}
  local seen = {}

  local latest_resp, latest_err = http.get({
    url = "https://api.github.com/repos/vlang/v/releases/latest",
  })
  if latest_err == nil and latest_resp.status_code == 200 then
    add_version(seen, versions, json.decode(latest_resp.body).tag_name)
  end

  for page = 1, 5 do
    local resp, err = http.get({
      url = "https://api.github.com/repos/vlang/v/tags?per_page=100&page=" .. page,
    })
    if err ~= nil then
      error("failed to fetch V tags: " .. err)
    end
    if resp.status_code ~= 200 then
      error("failed to fetch V tags: status " .. resp.status_code)
    end

    local tags = json.decode(resp.body)
    if #tags == 0 then
      break
    end
    for _, tag in ipairs(tags) do
      add_version(seen, versions, tag.name)
    end
  end

  -- mise reverses traditional vfox plugin lists before matching. Keep the
  -- plugin order newest-to-oldest so the backend's reversed list ends with the
  -- stable latest release rather than a weekly tag.
  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  if version:match("^ref:") then
    return nil
  end

  local asset
  if OS_TYPE == "windows" then
    asset = "v_windows.zip"
  elseif OS_TYPE == "darwin" or OS_TYPE == "macos" then
    if ARCH_TYPE == "arm64" or ARCH_TYPE == "aarch64" then
      asset = "v_macos_arm64.zip"
    else
      asset = "v_macos_x86_64.zip"
    end
  elseif ARCH_TYPE == "arm64" or ARCH_TYPE == "aarch64" then
    asset = "v_linux_arm64.zip"
  else
    asset = "v_linux.zip"
  end

  return repo .. "/releases/download/" .. version .. "/" .. asset
end

function util.install(root, version)
  if root == nil or root == "" or root == "/" then
    error("refusing to install into unsafe root path: " .. tostring(root))
  end

  if version:match("^ref:") then
    local ref = version:gsub("^ref:", "")
    run("rm -rf " .. shell_quote(root))
    run("git clone --quiet " .. shell_quote(repo) .. " " .. shell_quote(root))
    run("git --git-dir " .. shell_quote(root .. "/.git") .. " --work-tree " .. shell_quote(root) .. " -c advice.detachedHead=false checkout " .. shell_quote(ref))
    run("make --quiet -C " .. shell_quote(root))
  end

  run("mkdir -p " .. shell_quote(root .. "/bin"))

  local binary = root .. "/v"
  if exists(root .. "/v/v") then
    binary = root .. "/v/v"
  elseif exists(root .. "/v.exe") then
    binary = root .. "/v.exe"
  elseif exists(root .. "/v/v.exe") then
    binary = root .. "/v/v.exe"
  end

  run("chmod +x " .. shell_quote(binary))
  run("ln -sf " .. shell_quote(binary) .. " " .. shell_quote(root .. "/bin/v"))
  run("test -x " .. shell_quote(root .. "/bin/v"))
end

return util
