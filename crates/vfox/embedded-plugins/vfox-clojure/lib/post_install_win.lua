-- Windows post-install: handles the .zip payload, which ships a PowerShell
-- module (ClojureTools.psm1/.psd1) instead of bash launchers.
--
-- The module locates itself with `$PSScriptRoot` and expects its jars
-- ("clojure-tools-<version>.jar", "exec.jar") alongside it, so the extracted
-- files must stay together in one directory. We do not copy the module around;
-- we generate small .cmd shims that import it, because PATH resolution on
-- Windows needs a .cmd/.exe, not a module function.
local util = require("util")

local win = {}

local CRLF = "\r\n"

-- cmd.exe has no quoting that survives arbitrary characters, but the install
-- root normally contains none of these; refuse loudly rather than build a
-- silently broken command.
local function quote(value)
  value = tostring(value)
  if value:find('[%c"]') then
    error("cannot quote path containing control characters or quotes: " .. value)
  end
  return '"' .. value .. '"'
end

local function path_join(...)
  return table.concat({ ... }, "\\"):gsub("\\\\+", "\\")
end

local function mkdir(path)
  path = quote(path)
  util.run("if not exist " .. path .. " mkdir " .. path)
end

-- PowerShell string literal (single-quoted), backslashes are literal.
local function ps_quote(value)
  return "'" .. tostring(value):gsub("'", "''") .. "'"
end

function win.run(ctx)
  local root = ctx.rootPath:gsub("/", "\\")

  -- The archive's single top-level folder ("ClojureTools") is stripped by the
  -- extractor, but stay tolerant in case an engine keeps it.
  local module_dir = root
  if util.exists(path_join(root, "ClojureTools", "ClojureTools.psm1")) then
    module_dir = path_join(root, "ClojureTools")
  end

  local bin = path_join(root, "bin")
  mkdir(bin)

  -- deps.edn / example-deps.edn / tools.edn and the jars already sit next to
  -- the module, which is exactly where it looks for them, so nothing to move.

  local shim = path_join(bin, "clojure-shim.ps1")
  local shim_body = table.concat({
    "$ErrorActionPreference = 'Stop'",
    "Import-Module -Name " .. ps_quote(path_join(module_dir, "ClojureTools.psd1")) .. " -Force",
    "Invoke-Clojure @args",
    "exit $LASTEXITCODE",
    "",
  }, CRLF)
  util.write_file(shim, shim_body, true)

  local cmd_body = table.concat({
    "@echo off",
    "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"%~dp0clojure-shim.ps1\" %*",
    "",
  }, CRLF)

  for _, name in ipairs({ "clj", "clojure" }) do
    util.write_file(path_join(bin, name .. ".cmd"), cmd_body, true)
  end
end

return win
