PLUGIN = {}
PLUGIN.name = "lua"
PLUGIN.version = "0.1.0"
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-lua"
PLUGIN.license = "MIT"
PLUGIN.description = "Lua version manager - compiles from source"
PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
    "Compiles Lua from source. Requires a C compiler (gcc/clang).",
    "Automatically installs LuaRocks for Lua 5.x versions.",
}

-- System prerequisites checked by mise before installing (see the `system_deps`
-- setting). Detection is the source of truth; the `packages` map only provides
-- remediation hints.
PLUGIN.systemDependencies = {
    { bin = "cc", packages = { apt = "build-essential", dnf = "gcc" } },
    { bin = "make", packages = { brew = "make", apt = "build-essential", dnf = "make" } },
    -- LuaRocks is downloaded with curl in post_install
    { bin = "curl", packages = { brew = "curl", apt = "curl", dnf = "curl" } },
    -- Lua links libreadline for interactive line editing on the default build
    { pkgconfig = "readline", optional = "readline line editing",
      packages = { brew = "readline", apt = "libreadline-dev", dnf = "readline-devel" } },
}
