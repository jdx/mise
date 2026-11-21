local util = require("util")

function PLUGIN:PreInstall(ctx)
  return {
    version = ctx.version,
    url = util.download_url(ctx.version),
  }
end
