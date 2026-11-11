local util = require("util")

function PLUGIN:PreInstall(ctx)
  if RUNTIME.osType == "windows" then
    error(
      "the Clojure CLI has no native Windows executables: its Windows release is a PowerShell module "
        .. "(ClojureTools.psm1), not clj/clojure programs a plugin can put on PATH. "
        .. "Use WSL, or see https://clojure.org/guides/install_clojure#_windows_instructions"
    )
  end
  return {
    version = ctx.version,
    url = util.download_url(ctx.version),
  }
end
