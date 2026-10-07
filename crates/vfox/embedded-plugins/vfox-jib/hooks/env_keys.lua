function PLUGIN:EnvKeys(ctx)
  return {
    {
      key = "PATH",
      value = ctx.path .. "/bin",
    },
  }
end
