local function exists(path)
  local f = io.open(path, "r")
  if f then
    f:close()
    return true
  end
  return false
end

local function shell_quote(value)
  return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

local function read_file(path)
  local f = assert(io.open(path, "r"))
  local content = f:read("*a")
  f:close()
  return content
end

local function write_file(path, content)
  local f = assert(io.open(path, "w"))
  f:write(content)
  f:close()
end

local function patch_script(path, root)
  local content = read_file(path)
  content = content:gsub("PREFIX", root)
  content = content:gsub("BINDIR", root .. "/bin")
  write_file(path, content)
end

function PLUGIN:PostInstall(ctx)
  local root = ctx.rootPath
  local src = root
  if exists(root .. "/clojure-tools/clojure") then
    src = root .. "/clojure-tools"
  end

  os.execute("mkdir -p " .. shell_quote(root .. "/bin"))
  os.execute("mkdir -p " .. shell_quote(root .. "/libexec"))
  os.execute("mkdir -p " .. shell_quote(root .. "/share/man/man1"))

  if src ~= root then
    os.execute("cp " .. shell_quote(src .. "/deps.edn") .. " " .. shell_quote(root .. "/deps.edn"))
    os.execute("cp " .. shell_quote(src .. "/example-deps.edn") .. " " .. shell_quote(root .. "/example-deps.edn"))
    os.execute("cp " .. shell_quote(src .. "/tools.edn") .. " " .. shell_quote(root .. "/tools.edn"))
  end
  os.execute("cp " .. shell_quote(src) .. "/*.jar " .. shell_quote(root .. "/libexec/"))
  os.execute("cp " .. shell_quote(src .. "/clojure.1") .. " " .. shell_quote(root .. "/share/man/man1/clojure.1"))
  os.execute("cp " .. shell_quote(src .. "/clj.1") .. " " .. shell_quote(root .. "/share/man/man1/clj.1"))
  os.execute("cp " .. shell_quote(src .. "/clojure") .. " " .. shell_quote(root .. "/bin/clojure"))
  os.execute("cp " .. shell_quote(src .. "/clj") .. " " .. shell_quote(root .. "/bin/clj"))

  patch_script(root .. "/bin/clojure", root)
  patch_script(root .. "/bin/clj", root)
  os.execute("chmod +x " .. shell_quote(root .. "/bin/clojure") .. " " .. shell_quote(root .. "/bin/clj"))
end
