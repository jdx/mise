PLUGIN = {}
PLUGIN.name = "postgres"
PLUGIN.version = "0.1.0"
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-postgres"
PLUGIN.license = "MIT"
PLUGIN.description = "PostgreSQL database - compiles from source"
PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
	"Compiles PostgreSQL from source. Requires: C compiler, make, readline, zlib, openssl.",
	"Automatically runs initdb unless POSTGRES_SKIP_INITDB=1 is set.",
}
