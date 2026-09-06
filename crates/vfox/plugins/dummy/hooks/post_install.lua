local file = require("file")

local windows = package.config:sub(1, 1) == "\\"

--- Single-quote `s` for a POSIX shell, including any apostrophe it contains.
local function sh_quote(s)
	return "'" .. s:gsub("'", "'\\''") .. "'"
end

--- Create the directory `path`, including any missing parents.
--- Lua has no mkdir, so this shells out; cmd.exe has no `-p` and wants backslashes.
--- The exit status is not a usable signal here -- this runs on Lua 5.1, where os.execute returns
--- system()'s raw status, and cmd.exe's mkdir fails when the directory already exists -- so the
--- result is checked against the filesystem instead.
local function mkdirp(path)
	if windows then
		os.execute('mkdir "' .. path:gsub("/", "\\") .. '" 2>nul')
	else
		os.execute("mkdir -p " .. sh_quote(path))
	end
	if not file.exists(path) then
		error("post_install: could not create " .. path)
	end
end

function PLUGIN:PostInstall(ctx)
	--- SDK installation root path
	local rootPath = ctx.rootPath
	local runtimeVersion = ctx.runtimeVersion

	-- Create the installation directory structure for dummy plugin
	mkdirp(rootPath .. "/bin")

	local version_file = assert(io.open(rootPath .. "/VERSION", "w"))
	assert(version_file:write(runtimeVersion))
	assert(version_file:close())

	-- Create a dummy executable
	local dummy_path = rootPath .. "/bin/dummy"
	local dummy_file = assert(io.open(dummy_path, "w"))
	assert(dummy_file:write("#!/bin/sh\necho 'dummy version 1.0.0'\n"))
	assert(dummy_file:close())
	if not windows then
		-- On Lua 5.1 os.execute returns system()'s status, so 0 is the success case.
		if os.execute("chmod +x " .. sh_quote(dummy_path)) ~= 0 then
			error("post_install: could not chmod " .. dummy_path)
		end
	end
end
