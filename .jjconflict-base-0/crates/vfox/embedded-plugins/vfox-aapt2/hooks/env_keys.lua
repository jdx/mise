--- Each SDK may have different environment variable configurations.
--- This allows plugins to define custom environment variables (including PATH settings)
--- @param ctx {path: string}  Context information (SDK installation directory)
function PLUGIN:EnvKeys(ctx)
	local mainPath = ctx.path
	return {
		{
			key = "PATH",
			value = mainPath,
		},
	}
end
