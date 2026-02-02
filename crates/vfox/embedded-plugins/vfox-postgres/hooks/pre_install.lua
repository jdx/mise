--- Returns pre-install information for PostgreSQL
--- @param ctx table Context provided by vfox
--- @return table Pre-install info
function PLUGIN:PreInstall(ctx)
	local version = ctx.version

	return {
		version = version,
		url = "https://ftp.postgresql.org/pub/source/v" .. version .. "/postgresql-" .. version .. ".tar.gz",
	}
end
