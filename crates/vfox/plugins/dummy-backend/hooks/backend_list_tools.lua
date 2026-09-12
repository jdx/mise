function PLUGIN:BackendListTools(ctx)
	return {
		tools = {
			{ name = "demo", description = "A tool exposed by a backend plugin" },
			{ name = "other" },
		},
	}
end
