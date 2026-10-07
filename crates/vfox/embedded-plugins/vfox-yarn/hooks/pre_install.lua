--- Pre-installation hook

local http = require("http")
local json = require("json")

-- Yarn 6+ (ZPM) ships as prebuilt Rust binaries in per-platform npm packages.
-- Only these targets are published.
local ZPM_TARGETS = {
    ["darwin/arm64"] = "aarch64-apple-darwin",
    ["linux/arm64"] = "aarch64-unknown-linux-musl",
    ["linux/x86"] = "i686-unknown-linux-musl",
    ["linux/amd64"] = "x86_64-unknown-linux-musl",
}

local function zpm_target()
    local key = RUNTIME.osType .. "/" .. RUNTIME.archType
    local target = ZPM_TARGETS[key]
    if not target then
        error("Yarn 6+ does not publish binaries for " .. key)
    end
    return target
end

function PLUGIN:PreInstall(ctx)
    local version = ctx.version
    local major_version = tonumber(version:match("^(%d+)")) or 0

    if major_version == 1 then
        -- Yarn Classic (v1.x) - return tarball URL for mise to handle
        local archive_url = "https://classic.yarnpkg.com/downloads/" .. version .. "/yarn-v" .. version .. ".tar.gz"

        -- Note about GPG verification (skip on Windows)
        local is_windows = package.config:sub(1, 1) == "\\"
        if os.getenv("MISE_YARN_SKIP_GPG") == nil and not is_windows then
            local stderr_redirect = " 2>/dev/null"

            local gpg_check = io.popen("command -v gpg" .. stderr_redirect)
            local has_gpg = gpg_check and gpg_check:read("*a"):match("%S")
            if gpg_check then
                gpg_check:close()
            end

            if not has_gpg then
                print(
                    "⚠️  Note: GPG verification skipped (gpg not found). Set MISE_YARN_SKIP_GPG=1 to suppress this message"
                )
            end
            -- Note: We can't do GPG verification when mise handles the download
            -- This is a tradeoff for simpler code
        end

        -- Return URL for mise to download and extract
        return {
            version = version,
            url = archive_url,
        }
    elseif major_version >= 6 then
        -- Yarn ZPM (v6+) - the platform package is a tarball holding one
        -- native binary; mise downloads, verifies, and extracts it and
        -- post-install moves the binary into bin/
        local target = zpm_target()
        local url = "https://registry.npmjs.org/@yarnpkg/yarn-" .. target .. "/" .. version
        -- try_get reports transport failures as a return value; http.get raises
        local resp, err = http.try_get({ url = url })
        if err ~= nil or resp.status_code ~= 200 then
            error("Failed to look up Yarn " .. version .. " at " .. url .. ": " .. tostring(err or resp.status_code))
        end
        local dist = json.decode(resp.body).dist
        return {
            version = version,
            url = dist.tarball,
            sha1 = dist.shasum,
        }
    else
        -- Yarn Berry (v2.x+) - single JS file, handled in post-install
        return {
            version = version,
        }
    end
end

return PLUGIN
