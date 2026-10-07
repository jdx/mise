local util = require("util")

function PLUGIN:PostInstall(ctx)
  util.verify_install(ctx.rootPath)
end
