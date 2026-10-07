local util = require("util")

function PLUGIN:PostInstall(ctx)
  util.install(ctx.rootPath, ctx.sdkInfo["v"].version)
end
