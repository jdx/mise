--- Returns environment variables to set for this tool.
--- @param ctx {path: string}  (Installation path)
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
	return {
		{
			key = "PATH",
			value = ctx.path .. "/bin",
		},
	}
end
