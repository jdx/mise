local http = require("http")
local json = require("json")

local util = {}

local metadata_url = "https://raw.githubusercontent.com/datacharmer/dbdeployer/master/downloads/tarball_list.json"
local downloads_url = "https://dev.mysql.com/downloads/mysql/"

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
  local av = split_version(type(a) == "table" and a.version or a)
  local bv = split_version(type(b) == "table" and b.version or b)
  for i = 1, math.max(#av, #bv) do
    local ai = av[i] or 0
    local bi = bv[i] or 0
    if ai ~= bi then
      return ai > bi
    end
  end
  return false
end

local function target_os()
  if OS_TYPE == "darwin" or OS_TYPE == "macos" then
    return "Darwin"
  end
  return "Linux"
end

local function target_arch()
  if ARCH_TYPE == "arm64" or ARCH_TYPE == "aarch64" then
    return "arm64"
  end
  return "amd64"
end

local function fetch_records()
  local resp, err = http.get({ url = metadata_url })
  if err ~= nil then
    error("failed to fetch MySQL tarball metadata: " .. err)
  end
  if resp.status_code ~= 200 then
    error("failed to fetch MySQL tarball metadata: status " .. resp.status_code)
  end
  return json.decode(resp.body).Tarballs
end

local function fetch_downloads(series)
  local url = downloads_url
  if series then
    local os = target_os() == "Darwin" and "33" or "2"
    url = url .. "?version=" .. series .. "&os=" .. os
  end
  -- MySQL's download page rejects generic HTTP client user agents.
  local ok, resp = pcall(http.get, {
    url = url,
    headers = { ["User-Agent"] = "curl/8.5.0" },
  })
  if ok and resp.status_code == 200 then
    return resp.body
  end

  -- Akamai intermittently rejects mise's native HTTP client while accepting curl.
  if not ok or resp.status_code == 403 then
    local handle = io.popen("curl --fail --silent --show-error --location " .. shell_quote(url))
    local body = handle:read("*a")
    local curl_ok = handle:close()
    if curl_ok and body ~= "" then
      return body
    end
  end

  local reason = ok and ("status " .. resp.status_code) or tostring(resp)
  error("failed to fetch MySQL downloads: " .. reason)
end

local function current_versions()
  local versions = {}
  local seen = {}
  for version in fetch_downloads():gmatch(">%s*(%d+%.%d+%.%d+)%s*[^<]*</option>") do
    if not seen[version] then
      seen[version] = true
      table.insert(versions, version)
    end
  end
  return versions
end

local function current_versions_or_empty()
  local ok, versions = pcall(current_versions)
  if ok then
    return versions
  end
  return {}
end

local function current_record(version)
  local series = version:match("^(%d+%.%d+)")
  if not series then
    return nil
  end

  local body = fetch_downloads(series)
  local arch = target_arch() == "arm64" and (target_os() == "Darwin" and "arm64" or "aarch64") or "x86_64"
  local prefix = "mysql-" .. version .. "-"
  local filename
  for candidate in body:gmatch("%((mysql%-%d+%.%d+%.%d+%-[^%)]+%.tar%.[gx]z)%)") do
    if
      candidate:sub(1, #prefix) == prefix
      and candidate:find(arch, 1, true)
      and not candidate:find("minimal", 1, true)
    then
      filename = candidate
      break
    end
  end
  if not filename then
    return nil
  end

  return {
    version = version,
    filename = filename,
    series = series,
    official = true,
  }
end

local function mysql_records()
  local records = {}
  local os_name = target_os()
  for _, record in ipairs(fetch_records()) do
    if record.flavor == "mysql" and record.minimal == false and record.OS == os_name then
      table.insert(records, record)
    end
  end
  return records
end

local function mysql_records_or_empty()
  local ok, records = pcall(mysql_records)
  if ok then
    return records
  end
  return {}
end

local function link_libaio_t64(root)
  if target_os() ~= "Linux" or exists(root .. "/lib/private/libaio.so.1") then
    return
  end

  local handle = io.popen("ldconfig -p 2>/dev/null | awk '$1 == \"libaio.so.1t64\" { print $NF; exit }'")
  local libaio = handle:read("*l")
  handle:close()
  if libaio and libaio ~= "" and exists(libaio) then
    -- Ubuntu 24.04 renamed the soname even though MySQL still requests libaio.so.1.
    run("ln -s " .. shell_quote(libaio) .. " " .. shell_quote(root .. "/lib/private/libaio.so.1"))
  end
end

function util.get_versions()
  local seen = {}
  local versions = {}
  for _, version in ipairs(current_versions_or_empty()) do
    seen[version] = true
    table.insert(versions, { version = version })
  end
  for _, record in ipairs(mysql_records_or_empty()) do
    if not seen[record.version] then
      seen[record.version] = true
      table.insert(versions, { version = record.version })
    end
  end
  table.sort(versions, version_gt)
  return versions
end

function util.record_for_version(version)
  local records = {}
  for _, record in ipairs(mysql_records_or_empty()) do
    if record.version == version then
      table.insert(records, record)
    end
  end
  if #records > 0 then
    local arch = target_arch()
    for _, record in ipairs(records) do
      if record.arch == arch then
        return record
      end
    end
    return records[1]
  end

  local record = current_record(version)
  if record then
    return record
  end
  error("No MySQL URL found for " .. version)
end

function util.download_url(record)
  if record.official then
    return "https://cdn.mysql.com/Downloads/MySQL-" .. record.series .. "/" .. record.filename
  end
  local url = record.url
  local series, filename = url:match("^https://dev%.mysql%.com/get/Downloads/MySQL%-([^/]+)/(.+)$")
  if series and filename then
    return "https://cdn.mysql.com/archives/mysql-" .. series .. "/" .. filename
  end
  return url
end

function util.sha512(record)
  if record.checksum then
    return record.checksum:match("^SHA512:(.+)$")
  end
  return nil
end

function util.post_install(root)
  if not exists(root .. "/bin/mysql") then
    local handle = io.popen("find " .. shell_quote(root) .. " -mindepth 1 -maxdepth 1 -type d | head -1")
    local extracted = handle:read("*l")
    handle:close()
    if extracted == nil or extracted == "" or not exists(extracted .. "/bin/mysql") then
      error("Could not find extracted MySQL directory under " .. root)
    end

    run("cp -R " .. shell_quote(extracted .. "/.") .. " " .. shell_quote(root))
    run("rm -rf " .. shell_quote(extracted))
  end

  link_libaio_t64(root)
  run("test -x " .. shell_quote(root .. "/bin/mysql"))
end

return util
