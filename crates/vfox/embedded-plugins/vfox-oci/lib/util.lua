local http = require("http")
local json = require("json")

local util = {}

local repo = "https://github.com/oracle/oci-cli"

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
      url = "https://api.github.com/repos/oracle/oci-cli/releases?per_page=100&page=" .. page,
    })
    if err ~= nil then
      error("failed to fetch OCI CLI releases: " .. err)
    end
    if resp.status_code ~= 200 then
      error("failed to fetch OCI CLI releases: status " .. resp.status_code)
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

function util.install(root, version)
  if root == nil or root == "" or root == "/" then
    error("refusing to install into unsafe root path: " .. tostring(root))
  end

  local bin = root .. "/bin"
  local install_script = root .. "/install.sh"
  local script_url = repo .. "/raw/v" .. version .. "/scripts/install/install.sh"

  run("mkdir -p " .. shell_quote(bin))
  run("curl -fsSL -o " .. shell_quote(install_script) .. " " .. shell_quote(script_url))
  run(
    "bash "
      .. shell_quote(install_script)
      .. " --accept-all-defaults"
      .. " --install-dir "
      .. shell_quote(root .. "/lib")
      .. " --exec-dir "
      .. shell_quote(bin)
      .. " --script-dir "
      .. shell_quote(bin .. "/oci-cli-scripts")
      .. " --rc-file-path "
      .. shell_quote(root .. "/oci.bashrc")
      .. " --oci-cli-version "
      .. shell_quote(version)
  )
  run("test -x " .. shell_quote(bin .. "/oci"))
  run(shell_quote(bin .. "/oci") .. " -v")
  run("rm -f " .. shell_quote(install_script))
end

return util
