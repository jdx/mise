-- Post-install dispatcher. The real work lives in lib/post_install_posix.lua
-- and lib/post_install_win.lua; pick one from the platform separator.
--
-- Clojure publishes two archives per release and vfox/mise extract whichever
-- `util.download_url` returns:
--   * .tar.gz -> POSIX: bash launchers + man pages  (post_install_posix)
--   * .zip    -> Windows: PowerShell module          (post_install_win)
local util = require("util")
local impl = util.is_windows() and require("post_install_win") or require("post_install_posix")

function PLUGIN:PostInstall(ctx)
  impl.run(ctx)
end
