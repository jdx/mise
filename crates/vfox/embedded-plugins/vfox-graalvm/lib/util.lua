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

-- Modern GraalVM Community releases follow the JDK version scheme (17.0.7,
-- 21.0.2, ...), so their minor component is always 0. Older GraalVM releases
-- from oracle/graal (1.0.0-rc*, 19.3.0, 20.1.0, 22.0.0.2, ...) are either
-- pre-17 or have a non-zero minor or a fourth numeric component.
local function version_is_new_jdk_format(version)
  if version:match("^%d+%.%d+%.%d+[%w+._-]*$") == nil or version:match("%-java%d+$") then
    return false
  end
  local maj, min = version:match("^(%d+)%.(%d+)")
  if tonumber(maj) < 17 or tonumber(min) ~= 0 then
    return false
  end
  return version:match("^%d+%.%d+%.%d+%.%d") == nil
end

local function version_is_ce_build(version)
  return version:match("%-java%d+$") ~= nil
end

local function major(version)
  return tonumber(version:match("^(%d+)"))
end

local function host_os()
  if OS_TYPE == "darwin" or OS_TYPE == "macos" then
    return "macos"
  end
  if OS_TYPE == "linux" then
    return "linux"
  end
  error("GraalVM is not available for OS " .. tostring(OS_TYPE))
end

local function host_arch()
  if ARCH_TYPE == "arm64" or ARCH_TYPE == "aarch64" then
    return "aarch64"
  end
  if ARCH_TYPE == "amd64" or ARCH_TYPE == "x86_64" or ARCH_TYPE == "x64" then
    return "x64"
  end
  error("GraalVM is not available for architecture " .. tostring(ARCH_TYPE))
end

local function os_arch_new_format()
  return host_os() .. "-" .. host_arch()
end

local function old_variant(version)
  local os_name = host_os()
  if host_arch() ~= "x64" then
    error("old GraalVM archive format is not available for " .. os_name .. " " .. tostring(ARCH_TYPE))
  end
  if os_name == "macos" then
    if major(version) == 1 then
      return "macos-amd64"
    end
    return "darwin-amd64"
  end
  return "linux-amd64"
end

local function add_versions_from_releases(versions, seen, url, handler)
  local resp, err = http.get({ url = url })
  if err ~= nil then
    error("failed to fetch GraalVM releases: " .. err)
  end
  if resp.status_code ~= 200 then
    error("failed to fetch GraalVM releases: status " .. resp.status_code)
  end
  for _, release in ipairs(json.decode(resp.body)) do
    if release.prerelease == false and release.assets and #release.assets > 0 then
      handler(versions, seen, release.tag_name)
    end
  end
end

local function add_version(versions, seen, version)
  if version and not seen[version] then
    seen[version] = true
    table.insert(versions, { version = version })
  end
end

function util.get_versions()
  local versions = {}
  local seen = {}

  add_versions_from_releases(
    versions,
    seen,
    "https://api.github.com/repos/oracle/graal/releases?per_page=100",
    function(out, seen_versions, tag)
      local version = tag:match("^vm%-(.+)$")
      if version then
        add_version(out, seen_versions, version)
      end
    end
  )

  add_versions_from_releases(
    versions,
    seen,
    "https://api.github.com/repos/graalvm/graalvm-ce-builds/releases?per_page=100",
    function(out, seen_versions, tag)
      local vm_version = tag:match("^vm%-(.+)$")
      if vm_version then
        add_version(out, seen_versions, vm_version .. "-java8")
        add_version(out, seen_versions, vm_version .. "-java11")
        return
      end

      local jdk_version = tag:match("^jdk%-(.+)$")
      if jdk_version and jdk_version:match("^%d+") then
        add_version(out, seen_versions, jdk_version)
      end
    end
  )

  table.sort(versions, version_gt)
  return versions
end

function util.download_url(version)
  if version_is_new_jdk_format(version) then
    local filename = "graalvm-community-jdk-" .. version .. "_" .. os_arch_new_format() .. "_bin.tar.gz"
    return "https://github.com/graalvm/graalvm-ce-builds/releases/download/jdk-" .. version .. "/" .. filename
  end

  local variant = old_variant(version)
  if version_is_ce_build(version) then
    local graalvm_version, java_version = version:match("^(.+)%-([^%-]+)$")
    local filename = "graalvm-ce-" .. java_version .. "-" .. variant .. "-" .. graalvm_version .. ".tar.gz"
    return "https://github.com/graalvm/graalvm-ce-builds/releases/download/vm-" .. graalvm_version .. "/" .. filename
  end

  local filename
  if major(version) == 1 then
    filename = "graalvm-ce-" .. version .. "-" .. variant .. ".tar.gz"
  else
    filename = "graalvm-ce-" .. variant .. "-" .. version .. ".tar.gz"
  end
  return "https://github.com/oracle/graal/releases/download/vm-" .. version .. "/" .. filename
end

function util.sha256(url)
  -- mise's http.get raises on connection errors (try_get returns them);
  -- official vfox's http.get already returns (resp, err). The checksum is
  -- optional, so fall back to no checksum instead of aborting the install.
  local get = http.try_get or http.get
  local resp, err = get({ url = url .. ".sha256" })
  if err ~= nil or resp == nil or resp.status_code ~= 200 then
    return nil
  end
  return resp.body:match("^%s*([0-9a-fA-F]+)")
end

function util.post_install(root)
  if exists(root .. "/bin/java") then
    return
  end

  local home = nil
  if exists(root .. "/Contents/Home/bin/java") then
    home = root .. "/Contents/Home"
  else
    local handle = io.popen("find " .. shell_quote(root) .. " -path '*/Contents/Home/bin/java' -print | head -1")
    local java = handle:read("*l")
    handle:close()
    if java and java ~= "" then
      home = java:gsub("/bin/java$", "")
    end
  end

  if not home then
    local handle = io.popen("find " .. shell_quote(root) .. " -mindepth 2 -maxdepth 3 -path '*/bin/java' -print | head -1")
    local java = handle:read("*l")
    handle:close()
    if java and java ~= "" then
      home = java:gsub("/bin/java$", "")
    end
  end

  if not home then
    error("Could not find GraalVM Home under " .. root)
  end

  run("cp -R " .. shell_quote(home .. "/.") .. " " .. shell_quote(root))
  run("find " .. shell_quote(root) .. " -mindepth 1 -maxdepth 1 -type d -name 'graalvm*' -exec rm -rf {} +")
  run("rm -rf " .. shell_quote(root .. "/Contents"))
  run("test -x " .. shell_quote(root .. "/bin/java"))
end

return util
