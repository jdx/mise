PLUGIN = {}

PLUGIN.name = "emsdk"
PLUGIN.version = "1.0.0"
PLUGIN.homepage = "https://github.com/rektide/vfox-emsdk"
PLUGIN.license = "Apache 2.0"
PLUGIN.description = "Emscripten SDK - compiler toolchain to WebAssembly"

PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
    "Requires git, python3, and cmake for building from source",
    "First install may take several minutes to compile tools",
}

PLUGIN.legacyFilenames = {
    ".emsdk-version"
}
