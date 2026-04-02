function PLUGIN:EnvKeys(ctx)
    return {
        { key = "MISE_DUMMY_ENV_VAR", value = ctx.version },
    }
end
