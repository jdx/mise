--- Returns information about the version to install
--- Poetry is installed via install.python-poetry.org script
--- No URL is returned as the installation is handled in post_install

function PLUGIN:PreInstall(ctx)
    return {
        version = ctx.version,
    }
end
