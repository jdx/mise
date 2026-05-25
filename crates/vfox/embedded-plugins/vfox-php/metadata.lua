PLUGIN = {}

PLUGIN.name = "php"
PLUGIN.version = "0.1.0"
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-php"
PLUGIN.license = "MIT"
PLUGIN.description = "PHP - popular general-purpose scripting language for web development"
PLUGIN.minRuntimeVersion = "0.3.0"
PLUGIN.notes = {
    "Compiles PHP from source. Requires: C compiler, make, autoconf, bison, re2c.",
    "macOS: brew install autoconf bison re2c libxml2 openssl@3 icu4c pkg-config",
    "Linux: apt install build-essential autoconf bison re2c libxml2-dev libssl-dev libicu-dev",
    "Automatically installs Composer after PHP.",
}
