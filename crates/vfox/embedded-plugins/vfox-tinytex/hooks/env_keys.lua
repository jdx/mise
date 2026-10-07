local util = require("util")

function PLUGIN:EnvKeys(ctx)
  return {
    {
      key = "PATH",
      value = ctx.path .. "/" .. util.bin_path(),
    },
  }
end
