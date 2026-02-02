--!nonstrict
--- !!! DO NOT EDIT OR RENAME !!!
PLUGIN = {
	--- !!! MUST BE SET !!!
	name = "test-nodejs",
	version = "1.0.0",
	homepage = "https://nodejs.org",
	license = "MIT",
	description = "Test Node.js plugin for vfox tests",

	--- !!! OPTIONAL !!!
	minRuntimeVersion = "0.3.0",
	notes = {},

	--- List legacy configuration filenames for determining the specified version of the tool.
	legacyFilenames = {
		".node-version",
		".nvmrc",
	},
}
