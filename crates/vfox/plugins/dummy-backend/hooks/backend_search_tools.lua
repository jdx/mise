function PLUGIN:BackendSearchTools(ctx)
	return {
		tools = {
			{ name = "search-" .. ctx.query, description = "A dynamically discovered tool" },
		},
	}
end
