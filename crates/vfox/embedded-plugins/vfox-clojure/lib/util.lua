local util = {}

--- Platform facts, from vfox's `RUNTIME` global.
---
--- Kept here rather than in a module of its own so every hook can call it
--- without pulling in the network stack below. `http`/`json` are therefore
--- required lazily, inside the version-fetching functions, so that install
--- and env hooks (which only need `is_windows`) stay dependency-free.
function util.is_windows()
  return RUNTIME.osType == "windows"
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

--- Map of release tag -> published_at (RFC 3339) from GitHub releases.
--- Tags without a published release are simply absent (no date reported).
local function release_dates()
  local http = require("http")
  local json = require("json")
  local dates = {}
  for page = 1, 5 do
    local resp, err = http.try_get({
      url = "https://api.github.com/repos/clojure/brew-install/releases?per_page=100&page=" .. page,
    })
    if err ~= nil or resp.status_code ~= 200 then
      break
    end
    local ok, releases = pcall(json.decode, resp.body)
    if not ok or type(releases) ~= "table" or #releases == 0 then
      break
    end
    for _, rel in ipairs(releases) do
      if rel.tag_name and rel.published_at and not rel.draft then
        dates[rel.tag_name] = rel.published_at
      end
    end
    if #releases < 100 then
      break
    end
  end
  return dates
end

function util.get_versions()
  local http = require("http")
  local json = require("json")
  local resp, err = http.get({
    url = "https://api.github.com/repos/clojure/brew-install/tags?per_page=100",
  })
  if err ~= nil then
    error("failed to fetch Clojure tool tags: " .. err)
  end
  if resp.status_code ~= 200 then
    error("failed to fetch Clojure tool tags: status " .. resp.status_code)
  end

  local dates = release_dates()
  local versions = {}
  for _, tag in ipairs(json.decode(resp.body)) do
    local version = tag.name:match("^(%d+%.%d+%.%d+%.%d+)$")
    if version then
      table.insert(versions, { version = version, created_at = dates[tag.name] })
    end
  end

  table.sort(versions, version_gt)
  return versions
end

--- Clojure ships two distinct archives per release:
---   * .tar.gz  -> POSIX tools: bash launchers ("clj", "clojure") + man pages.
---   * .zip     -> Windows tools: a PowerShell module (ClojureTools.psm1/.psd1)
---                 exposing Invoke-Clojure, aliased to clj/clojure.
--- Both are extracted automatically by vfox/mise (format chosen by extension),
--- each unwrapping its single top-level folder. Pick the one that matches the
--- platform so the right launcher flavour lands in the install directory.
function util.download_url(version)
  local ext = util.is_windows() and ".zip" or ".tar.gz"
  return "https://download.clojure.org/install/clojure-tools-" .. version .. ext
end

-- ---------------------------------------------------------------------------
-- Filesystem / shell helpers (platform-agnostic; only wrap the Lua stdlib).
-- ---------------------------------------------------------------------------

-- Run a shell command and fail loudly instead of silently ignoring failures.
-- os.execute returns 0 (real Lua) or true (gopher-lua) on success.
function util.run(command)
  local ok = os.execute(command)
  if ok == 0 or ok == true then
    return
  end
  error("command failed: " .. command, 2)
end

function util.exists(path)
  local f = io.open(path, "r")
  if f then
    f:close()
    return true
  end
  return false
end

function util.read_file(path)
  local f = assert(io.open(path, "r"))
  local content = f:read("*a")
  f:close()
  return content
end

--- Write a file. `binary` skips newline translation, which matters when
--- emitting CRLF-delimited Windows shims: a text-mode write on Windows would
--- turn an explicit "\r\n" into "\r\r\n".
function util.write_file(path, content, binary)
  local f = assert(io.open(path, binary and "wb" or "w"))
  f:write(content)
  f:close()
end

return util
