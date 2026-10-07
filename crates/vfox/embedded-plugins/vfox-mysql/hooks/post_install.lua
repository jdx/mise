local util = require("util")

function PLUGIN:PostInstall(ctx)
  util.post_install(ctx.rootPath)
end
