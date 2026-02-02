--- Returns the environment variables that need to be set.
--- @param ctx {path: string}  (Installation directory)
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
	return {
		{
			key = "PATH",
			value = ctx.path,
		},
	}
end
