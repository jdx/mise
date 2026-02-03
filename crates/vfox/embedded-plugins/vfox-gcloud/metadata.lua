--- !!! DO NOT EDIT OR RENAME !!!
PLUGIN = {}

--- !!! MUST BE SET !!!
--- Plugin name
PLUGIN.name = "gcloud"
--- Plugin version
PLUGIN.version = "0.1.0"
--- Plugin homepage
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-gcloud"
--- Plugin license
PLUGIN.license = "MIT"
--- Plugin description
PLUGIN.description = "Google Cloud SDK (gcloud CLI)"

--- !!! OPTIONAL !!!
--- Minimum compatible vfox version
PLUGIN.minRuntimeVersion = "0.3.0"
--- Some things that need user attention
PLUGIN.notes = {
    "After installation, you may need to run 'gcloud init' to configure your account.",
    "Additional components can be installed with 'gcloud components install <component>'.",
}

--- Legacy configuration filenames for determining tool version
PLUGIN.legacyFilenames = {}
