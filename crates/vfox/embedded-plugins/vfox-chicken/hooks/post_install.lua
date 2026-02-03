--- Post-installation hook for CHICKEN
--- Fixes library paths for macOS binaries

function PLUGIN:PostInstall(ctx)
    local file = require("file")
    local root_path = ctx.rootPath
    local os_type = RUNTIME.osType

    -- On macOS, we need to fix up library paths using install_name_tool
    if os_type == "darwin" then
        local lib_path = file.join_path(root_path, "lib")
        local bin_path = file.join_path(root_path, "bin")
        local libchicken = file.join_path(lib_path, "libchicken.dylib")

        -- Create a shell script to fix library paths
        local script = string.format(
            [[
#!/bin/bash
set -e
LIB="%s"
BIN="%s"

# Fix the library's own id
install_name_tool -id "$LIB/libchicken.dylib" "$LIB/libchicken.dylib" 2>/dev/null || true

# Fix all binaries
for bin in chicken csc csi chicken-install chicken-uninstall chicken-status chicken-profile chicken-do feathers; do
    if [ -f "$BIN/$bin" ]; then
        # Get the old library path (the xxx placeholder)
        OLD_PATH=$(otool -L "$BIN/$bin" 2>/dev/null | grep libchicken | head -1 | awk '{print $1}')
        if [ -n "$OLD_PATH" ]; then
            install_name_tool -change "$OLD_PATH" "$LIB/libchicken.dylib" "$BIN/$bin" 2>/dev/null || true
        fi
    fi
done
]],
            lib_path,
            bin_path
        )

        -- Write and execute the script
        local script_path = root_path .. "/fix_libs.sh"
        local f = io.open(script_path, "w")
        if f then
            f:write(script)
            f:close()
            os.execute("chmod +x " .. script_path .. " && " .. script_path)
            os.execute("rm -f " .. script_path)
        end
    end
end
