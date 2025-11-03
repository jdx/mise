--- !!! DO NOT EDIT OR RENAME !!!
PLUGIN = {}

--- !!! MUST BE SET !!!
--- Plugin name
PLUGIN.name = "dummy"
--- Plugin version
PLUGIN.version = "0.3.0"
--- Plugin repository
PLUGIN.homepage = "https://github.com/version-fox/vfox-nodejs"
--- Plugin license
PLUGIN.license = "Apache 2.0"
--- Plugin description
PLUGIN.description = "Dummy plugin for testing."

--- !!! OPTIONAL !!!
--- minimum compatible vfox version
PLUGIN.minRuntimeVersion = "0.3.0"
--- Some things that need user to be attention!
PLUGIN.notes = {}

--- List legacy configuration filenames for determining the specified version of the tool.
--- such as ".node-version", ".nvmrc", etc.
PLUGIN.legacyFilenames = {
    ".dummy-version"
}