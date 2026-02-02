--- Called before installation to return the download URL.
--- Chez Scheme only provides source tarballs, which need to be compiled.
--- @param ctx {version: string}  (Version to install)
--- @return table File info with URL
function PLUGIN:PreInstall(ctx)
	local version = ctx.version

	-- Source tarball URL pattern: csv{version}.tar.gz
	local url = "https://github.com/cisco/ChezScheme/releases/download/v" .. version .. "/csv" .. version .. ".tar.gz"

	return {
		url = url,
		version = version,
	}
end
