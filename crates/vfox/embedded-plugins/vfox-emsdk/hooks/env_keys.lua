local function shell_quote(value)
    return "'" .. value:gsub("'", "'\\''") .. "'"
end

local function dir_exists(path)
    if RUNTIME.osType == "windows" then
        return os.execute('if exist "' .. path .. '\\*" exit /b 0 else exit /b 1') == true
    end
    local ret = os.execute("test -d " .. shell_quote(path))
    return ret == true or ret == 0
end

function PLUGIN:EnvKeys(ctx)
    local mainPath = ctx.path
    local result = {}

    table.insert(result, { key = "EMSDK", value = mainPath })

    local em_config = mainPath .. "/.emscripten"
    local em_config_f = io.open(em_config, "r")
    if em_config_f then
        em_config_f:close()
        table.insert(result, { key = "EM_CONFIG", value = em_config })
    end

    local emscripten_root = mainPath .. "/upstream/emscripten"
    if dir_exists(emscripten_root) then
        table.insert(result, { key = "EMSCRIPTEN_ROOT", value = emscripten_root })
        table.insert(result, { key = "EMCC_CACHE", value = emscripten_root .. "/cache" })
    end

    local llvm_bin = mainPath .. "/upstream/bin"
    if dir_exists(llvm_bin) then
        table.insert(result, { key = "LLVM", value = llvm_bin })
        table.insert(result, { key = "LLVM_ROOT", value = llvm_bin })
        table.insert(result, { key = "EM_LLVM_ROOT", value = llvm_bin })
    end

    local binaryen_root = mainPath .. "/upstream"
    if dir_exists(binaryen_root) then
        table.insert(result, { key = "BINARYEN", value = binaryen_root })
        table.insert(result, { key = "BINARYEN_ROOT", value = binaryen_root })
        table.insert(result, { key = "EM_BINARYEN_ROOT", value = binaryen_root })
    end

    local node_root = mainPath .. "/node"
    if RUNTIME.osType ~= "windows" and dir_exists(node_root) then
        local handle = io.popen("find " .. shell_quote(node_root) .. " -mindepth 3 -maxdepth 3 -type f -name node | head -n 1")
        if handle then
            local node = handle:read("*l")
            handle:close()
            if node and node ~= "" then
                table.insert(result, { key = "EMSDK_NODE", value = node })
                table.insert(result, { key = "NODE_JS", value = node })
                table.insert(result, { key = "PATH", value = node:match("(.+)/[^/]+$") })
            end
        end
    end

    if RUNTIME.osType == "windows" then
        table.insert(result, { key = "PATH", value = mainPath })
        table.insert(result, { key = "PATH", value = mainPath .. "\\upstream\\bin" })
        table.insert(result, { key = "PATH", value = mainPath .. "\\upstream\\emscripten" })
    else
        table.insert(result, { key = "PATH", value = mainPath })
        table.insert(result, { key = "PATH", value = mainPath .. "/upstream/bin" })
        table.insert(result, { key = "PATH", value = mainPath .. "/upstream/emscripten" })
    end

    return result
end
