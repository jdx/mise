PLUGIN = {}

PLUGIN.name = "redis"
PLUGIN.version = "0.1.0"
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-redis"
PLUGIN.license = "MIT"
PLUGIN.description = "Redis - in-memory data structure store, cache, and message broker"
PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
    "Compiles from source - requires C compiler (gcc/clang) and make",
}

-- System prerequisites checked by mise before installing (see the `system_deps`
-- setting). Detection is the source of truth; the `packages` map only provides
-- remediation hints.
PLUGIN.systemDependencies = {
    { bin = "cc", packages = { apt = "build-essential", dnf = "gcc" } },
    { bin = "make", packages = { brew = "make", apt = "build-essential", dnf = "make" } },
}
