local json = require("json")
local http = require("http")

local versions = {}

local function get_data()
  if versions.data then
    return versions.data
  end

  local resp, err = http.get({
    url = "https://github.com/ciscoski/gcc-arm-none-eabi-dist/releases/latest/download/gcc-arm-none-eabi.json"
  })

  if err ~= nil or resp.status_code ~= 200 then
    versions.data = {}
  else
    versions.data = json.decode(resp.body)
  end

  return versions.data
end

--
local system_arch_compatibility_matrix = {
  windows = { amd64 = { "amd64", "i386" }, i386 = { "i386" }, arm64 = { "amd64", "arm64" } },
  darwin = { amd64 = { "amd64" }, i386 = {}, arm64 = { "arm64", "amd64" } },
  linux = { amd64 = { "amd64", "i386" }, i386 = { "i386" }, arm64 = { "arm64" } }
}

function versions:GetAvailableVersions()
  local host_os = RUNTIME.osType
  local host_arch = RUNTIME.archType
  local available_versions = {}
  local compatible_architecture = system_arch_compatibility_matrix[host_os][host_arch]
  for _, entry in ipairs(get_data()) do
    for _, arch in ipairs(compatible_architecture) do
      if (entry[host_os] and entry[host_os][arch]) then
        local version = entry.version .. "~" .. arch
        table.insert(available_versions, { version = version })
      end
    end
  end
  return available_versions
end

function versions:GetPreInstall(version, arch)
  local host_os = RUNTIME.osType
  local host_arch = RUNTIME.archType
  local archs

  if arch then
    archs = { arch }
  else
    archs = system_arch_compatibility_matrix[host_os][host_arch]
  end

  for _, entry in ipairs(get_data()) do
    if entry.version == version then
      for _, arch in ipairs(archs) do
        if (entry[host_os] and entry[host_os][arch]) then
          return {
            version = entry.version .. "~" .. arch,
            url = entry[host_os][arch].url
          }
        end
      end
    end
  end
end

return versions
