--- Returns information about the version to install
--- Constructs the download URL based on OS and architecture

function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    local os_type = RUNTIME.osType
    local arch_type = RUNTIME.archType

    -- Map OS and arch to the filename components
    -- Pattern: chicken-VERSION-ARCH-OS-VARIANT.tar.gz
    local arch, os_name, variant

    if os_type == "darwin" then
        if arch_type ~= "amd64" then
            error("CHICKEN only has x86_64 binaries for macOS. ARM64 is not supported.")
        end
        arch = "x86_64"
        os_name = "macosx"
        variant = "macosx"
    elseif os_type == "linux" then
        if arch_type ~= "amd64" then
            error("CHICKEN only has x86_64 binaries for Linux. ARM64 is not supported.")
        end
        arch = "x86_64"
        os_name = "linux"
        variant = "gnu" -- Use GNU libc version by default
    elseif os_type == "freebsd" then
        if arch_type ~= "amd64" then
            error("CHICKEN only has amd64 binaries for FreeBSD.")
        end
        arch = "amd64"
        os_name = "bsd"
        variant = "freebsd"
    elseif os_type == "openbsd" then
        if arch_type ~= "amd64" then
            error("CHICKEN only has amd64 binaries for OpenBSD.")
        end
        arch = "amd64"
        os_name = "bsd"
        variant = "openbsd"
    else
        error("Unsupported operating system: " .. os_type)
    end

    local filename = string.format("chicken-%s-%s-%s-%s.tar.gz", version, arch, os_name, variant)
    local url = "https://foldling.org/dust/" .. filename

    return {
        version = version,
        url = url,
    }
end
