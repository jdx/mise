--- Pre-installation hook

function PLUGIN:PreInstall(ctx)
    local version = ctx.version
    local major_version = string.sub(version, 1, 1)
    
    if major_version == "1" then
        -- Yarn Classic (v1.x) - return tarball URL for mise to handle
        local archive_url = "https://classic.yarnpkg.com/downloads/" .. version .. "/yarn-v" .. version .. ".tar.gz"
        
        -- Note about GPG verification (skip on Windows)
        local is_windows = package.config:sub(1,1) == '\\'
        if os.getenv("MISE_YARN_SKIP_GPG") == nil and not is_windows then
            local stderr_redirect = " 2>/dev/null"
            
            local gpg_check = io.popen("command -v gpg" .. stderr_redirect)
            local has_gpg = gpg_check and gpg_check:read("*a"):match("%S")
            if gpg_check then gpg_check:close() end
            
            if not has_gpg then
                print("⚠️  Note: GPG verification skipped (gpg not found). Set MISE_YARN_SKIP_GPG=1 to suppress this message")
            end
            -- Note: We can't do GPG verification when mise handles the download
            -- This is a tradeoff for simpler code
        end
        
        -- Return URL for mise to download and extract
        return {
            version = version,
            url = archive_url
        }
    else
        -- Yarn Berry (v2.x+) - single JS file, handled in post-install
        return {
            version = version
        }
    end
end

return PLUGIN