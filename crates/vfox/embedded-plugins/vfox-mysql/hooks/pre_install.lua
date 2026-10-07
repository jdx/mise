local util = require("util")

function PLUGIN:PreInstall(ctx)
  local record = util.record_for_version(ctx.version)
  local install = {
    version = ctx.version,
    url = util.download_url(record),
  }
  local sha512 = util.sha512(record)
  if sha512 then
    install.sha512 = sha512
  end
  return install
end
