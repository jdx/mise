--- !!! DO NOT EDIT OR RENAME !!!
PLUGIN = {}

--- !!! MUST BE SET !!!
--- Plugin name
PLUGIN.name = "test-nodejs"
--- Plugin version
PLUGIN.version = "1.0.0"
--- Plugin repository
PLUGIN.homepage = "https://nodejs.org"
--- Plugin license
PLUGIN.license = "MIT"
--- Plugin description
PLUGIN.description = "Test Node.js plugin for vfox tests"

--- !!! OPTIONAL !!!
--- minimum compatible vfox version
PLUGIN.minRuntimeVersion = "0.3.0"
--- Some things that need user to be attention!
PLUGIN.notes = {}

--- List legacy configuration filenames for determining the specified version of the tool.
--- such as ".node-version", ".nvmrc", etc.
PLUGIN.legacyFilenames = {
    ".node-version",
    ".nvmrc"
}