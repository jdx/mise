--- Returns information about where to download clickhouse
--- @param ctx {version: string}  Context containing version info (The version to install)
--- @return table Installation info including download URL
function PLUGIN:PreInstall(ctx)
    local util = require("util")
    local version = ctx.version
    local arch = util.get_arch()
    local trimmed_version = util.trim_version_suffix(version)
    local url

    if RUNTIME.osType == "darwin" then
        -- macOS: single binary file
        -- clickhouse-macos (x86_64) or clickhouse-macos-aarch64 (arm64)
        if arch == "arm64" then
            url = string.format(
                "https://github.com/ClickHouse/ClickHouse/releases/download/v%s/clickhouse-macos-aarch64",
                version
            )
        else
            url = string.format(
                "https://github.com/ClickHouse/ClickHouse/releases/download/v%s/clickhouse-macos",
                version
            )
        end
    else
        -- Linux: tarball
        -- clickhouse-common-static-{trimmed_version}-{arch}.tgz
        url = string.format(
            "https://github.com/ClickHouse/ClickHouse/releases/download/v%s/clickhouse-common-static-%s-%s.tgz",
            version,
            trimmed_version,
            arch
        )
    end

    return {
        version = version,
        url = url,
    }
end
