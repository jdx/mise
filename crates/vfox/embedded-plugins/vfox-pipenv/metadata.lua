--- !!! DO NOT EDIT OR RENAME !!!
PLUGIN = {}

--- !!! MUST BE SET !!!
--- Plugin name
PLUGIN.name = "pipenv"
--- Plugin version
PLUGIN.version = "0.1.0"
--- Plugin homepage
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-pipenv"
--- Plugin license
PLUGIN.license = "MIT"
--- Plugin description
PLUGIN.description = "Python Development Workflow for Humans - https://pipenv.pypa.io"

--- !!! OPTIONAL !!!
PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
	"Requires Python 3.7+ to be installed and available in PATH",
	"If the Python interpreter used during installation is removed, pipenv will stop working and needs to be reinstalled",
}
