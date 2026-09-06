--- Metadata for the CHICKEN Scheme plugin
--- CHICKEN is a compiler for the Scheme programming language
--- https://call-cc.org/

PLUGIN = {
    name = "chicken",
    version = "0.1.0",
    description = "CHICKEN Scheme compiler",
    homepage = "https://call-cc.org/",
    license = "BSD",
    minRuntimeVersion = "0.3.0",

    -- System prerequisites checked by mise before installing (see the
    -- `system_deps` setting). CHICKEN compiles generated C, so it needs a C
    -- compiler and make. Detection is the source of truth; the `packages` map
    -- only provides remediation hints.
    systemDependencies = {
        { bin = "cc", packages = { apt = "build-essential", dnf = "gcc" } },
        { bin = "make", packages = { brew = "make", apt = "build-essential", dnf = "make" } },
    },
}
