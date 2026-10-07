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

local function move_binary(src_dir, bin_dir, name)
  local app = src_dir .. "/" .. name .. ".app"
  local binary = src_dir .. "/" .. name
  if exists(app .. "/Contents/MacOS/" .. name) then
    os.execute(
      "mv "
        .. shell_quote(app)
        .. " "
        .. shell_quote(bin_dir .. "/" .. name .. ".app")
        .. " && ln -sf "
        .. shell_quote(name .. ".app/Contents/MacOS/" .. name)
        .. " "
        .. shell_quote(bin_dir .. "/" .. name)
    )
  elseif exists(binary) then
    os.execute("mv " .. shell_quote(binary) .. " " .. shell_quote(bin_dir .. "/" .. name))
  end
end

function PLUGIN:PostInstall(ctx)
  local root = ctx.rootPath
  local src = root
  if exists(root .. "/teleport/VERSION") then
    src = root .. "/teleport"
  end

  local bin = root .. "/bin"
  os.execute("mkdir -p " .. shell_quote(bin))

  move_binary(src, bin, "teleport")
  move_binary(src, bin, "tbot")
  move_binary(src, bin, "tctl")
  move_binary(src, bin, "tsh")

  os.execute("chmod +x " .. shell_quote(bin .. "/teleport") .. " " .. shell_quote(bin .. "/tbot") .. " " .. shell_quote(bin .. "/tctl") .. " " .. shell_quote(bin .. "/tsh"))
end
