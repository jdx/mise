local util = require("util")

function PLUGIN:Available(ctx)
  return util.get_versions()
end
