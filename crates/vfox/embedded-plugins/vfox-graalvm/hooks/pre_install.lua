local util = require("util")

function PLUGIN:PreInstall(ctx)
  local url = util.download_url(ctx.version)
  local install = {
    version = ctx.version,
    url = url,
  }
  local sha256 = util.sha256(url)
  if sha256 then
    install.sha256 = sha256
  end
  return install
end
