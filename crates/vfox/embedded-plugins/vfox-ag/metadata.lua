--- !!! DO NOT EDIT OR RENAME !!!
PLUGIN = {}

--- !!! MUST BE SET !!!
--- Plugin name
PLUGIN.name = "ag"
--- Plugin version
PLUGIN.version = "0.1.0"
--- Plugin homepage
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-ag"
--- Plugin license, please choose a correct license according to your needs.
PLUGIN.license = "Apache-2.0"
--- Plugin description
PLUGIN.description = "The Silver Searcher - A code searching tool similar to ack, with a focus on speed"

--- !!! OPTIONAL !!!
--- Minimum compatible vfox version
PLUGIN.minRuntimeVersion = "0.3.0"
--- Manifest URL, if set, it will be used to check for updates
-- PLUGIN.manifestUrl = "https://github.com/mise-plugins/vfox-ag/releases/download/manifest/manifest.json"
--- User attention
PLUGIN.notes = {
    "ag requires build dependencies: automake, pkg-config, pcre, xz, and a C compiler",
    "On macOS: brew install automake pkg-config pcre xz",
    "On Debian/Ubuntu: apt-get install automake pkg-config libpcre3-dev zlib1g-dev liblzma-dev",
}
