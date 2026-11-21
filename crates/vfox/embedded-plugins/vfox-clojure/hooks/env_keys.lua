-- Expose the install's bin/ on PATH. Both platform payloads create that
-- directory and put the launchers there:
--   * POSIX   -> the bash scripts "clj" / "clojure"
--   * Windows -> the shims "clj.cmd" / "clojure.cmd" (resolved via PATHEXT)
local util = require("util")

function PLUGIN:EnvKeys(ctx)
  local bin = ctx.path .. "/bin"
  if util.is_windows() then
    bin = bin:gsub("/", "\\")
  end

  return {
    {
      key = "PATH",
      value = bin,
    },
  }
end
