function PLUGIN:EnvKeys(ctx)
  return {
    {
      key = "PATH",
      value = ctx.path .. "/bin",
    },
    {
      key = "JAVA_HOME",
      value = ctx.path,
    },
    {
      key = "GRAALVM_HOME",
      value = ctx.path,
    },
  }
end
