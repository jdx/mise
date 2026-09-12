--- Returns pre-installed information including GitHub artifact attestation metadata.
--- @param ctx {version: string} Context information
--- @return table Version information with attestation
function PLUGIN:PreInstall(ctx)
	return {
		version = ctx.version,
		url = "https://example.com/download/" .. ctx.version .. ".tar.gz",
		attestation = {
			github_owner = "test-owner",
			github_repo = "test-repo",
		},
	}
end
