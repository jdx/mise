-- POSIX post-install: handles the .tar.gz payload, which ships bash launchers
-- ("clj", "clojure"), man pages and the tools jars.
--
-- Files here are shell scripts that consume the install path as a POSIX string
-- (see patch_script), so every path stays "/"-separated on purpose. Only shell
-- command arguments are quoted for the shell.
local util = require("util")

local posix = {}

local function shell_quote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

local function mkdir(path)
  util.run("mkdir -p " .. shell_quote(path))
end

local function copy_file(source, destination)
  util.run("cp " .. shell_quote(source) .. " " .. shell_quote(destination))
end

-- POSIX sh expands the glob, so only the directory part may be quoted.
local function copy_glob(directory, pattern, destination)
  util.run("cp " .. shell_quote(directory) .. "/" .. pattern .. " " .. shell_quote(destination))
end

local function chmod_executable(...)
  local paths = {}
  for _, path in ipairs({ ... }) do
    table.insert(paths, shell_quote(path))
  end
  util.run("chmod +x " .. table.concat(paths, " "))
end

-- PREFIX/BINDIR are substituted into the bash launchers, where a backslash is
-- an escape character rather than a separator, so the value must stay POSIX.
local function patch_script(path, root)
  local prefix = (root:gsub("\\", "/"))
  local content = util.read_file(path)
  content = content:gsub("PREFIX", prefix)
  content = content:gsub("BINDIR", prefix .. "/bin")
  util.write_file(path, content)
end

function posix.run(ctx)
  local root = ctx.rootPath
  local src = root
  if util.exists(root .. "/clojure-tools/clojure") then
    src = root .. "/clojure-tools"
  end

  mkdir(root .. "/bin")
  mkdir(root .. "/libexec")
  mkdir(root .. "/share/man/man1")

  if src ~= root then
    copy_file(src .. "/deps.edn", root .. "/deps.edn")
    copy_file(src .. "/example-deps.edn", root .. "/example-deps.edn")
    copy_file(src .. "/tools.edn", root .. "/tools.edn")
  end
  copy_glob(src, "*.jar", root .. "/libexec")
  copy_file(src .. "/clojure.1", root .. "/share/man/man1/clojure.1")
  copy_file(src .. "/clj.1", root .. "/share/man/man1/clj.1")
  copy_file(src .. "/clojure", root .. "/bin/clojure")
  copy_file(src .. "/clj", root .. "/bin/clj")

  patch_script(root .. "/bin/clojure", root)
  patch_script(root .. "/bin/clj", root)
  chmod_executable(root .. "/bin/clojure", root .. "/bin/clj")
end

return posix
