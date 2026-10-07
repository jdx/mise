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

-- System prerequisites for compiling PHP from source. mise checks these before
-- installing and (per the `system_deps` setting) reports or offers to install
-- anything missing. Detection is the source of truth: a check that already
-- passes is satisfied regardless of how it was installed, so the `packages`
-- hints are only used to offer remediation.
--
-- Only the build tools are marked required: they are always needed and can be
-- detected reliably. The libraries are marked optional (informational only)
-- because this plugin adds each one to PKG_CONFIG_PATH from `brew --prefix` at
-- build time — many are keg-only on macOS, so a bare `pkg-config` probe would
-- report them missing even though the build finds them. Flagging them as
-- required would false-alarm on working machines; as optional they still tell
-- the user what to install without blocking or prompting.
PLUGIN.systemDependencies = {
    -- build toolchain (required)
    { bin = "cc", packages = { apt = "build-essential", dnf = "gcc" } },
    { bin = "make", packages = { brew = "make", apt = "build-essential", dnf = "make" } },
    { bin = "autoconf", packages = { brew = "autoconf", apt = "autoconf", dnf = "autoconf" } },
    -- macOS ships bison 2.3; PHP's parser generator needs >= 3.0
    { bin = "bison", version = ">=3.0",
      packages = { brew = "bison", apt = "bison", dnf = "bison" } },
    { bin = "re2c", packages = { brew = "re2c", apt = "re2c", dnf = "re2c" } },
    { bin = "pkg-config",
      packages = { brew = "pkg-config", apt = "pkg-config", dnf = "pkgconfig" } },

    -- libraries (optional/informational — see note above)
    { pkgconfig = "libxml-2.0", optional = "xml support",
      packages = { brew = "libxml2", apt = "libxml2-dev", dnf = "libxml2-devel" } },
    { pkgconfig = "openssl", optional = "openssl / TLS support",
      packages = { brew = "openssl@3", apt = "libssl-dev", dnf = "openssl-devel" } },
    { pkgconfig = "oniguruma", optional = "mbstring support",
      packages = { brew = "oniguruma", apt = "libonig-dev", dnf = "oniguruma-devel" } },
    { pkgconfig = "icu-uc", optional = "intl support",
      packages = { brew = "icu4c", apt = "libicu-dev", dnf = "libicu-devel" } },
    { pkgconfig = "zlib", optional = "zlib support",
      packages = { brew = "zlib", apt = "zlib1g-dev", dnf = "zlib-devel" } },
    { pkgconfig = "libzip", optional = "zip support",
      packages = { brew = "libzip", apt = "libzip-dev", dnf = "libzip-devel" } },
    { pkgconfig = "libcurl", optional = "curl support",
      packages = { brew = "curl", apt = "libcurl4-openssl-dev", dnf = "libcurl-devel" } },
    { pkgconfig = "sqlite3", optional = "sqlite support",
      packages = { brew = "sqlite", apt = "libsqlite3-dev", dnf = "sqlite-devel" } },
    { pkgconfig = "libpng", optional = "gd image support",
      packages = { brew = "libpng", apt = "libpng-dev", dnf = "libpng-devel" } },
    { pkgconfig = "freetype2", optional = "gd freetype support",
      packages = { brew = "freetype", apt = "libfreetype6-dev", dnf = "freetype-devel" } },
    { pkgconfig = "libjpeg", optional = "gd jpeg support",
      packages = { brew = "jpeg", apt = "libjpeg-dev", dnf = "libjpeg-devel" } },
    { pkgconfig = "libwebp", optional = "gd webp support",
      packages = { brew = "webp", apt = "libwebp-dev", dnf = "libwebp-devel" } },
    { pkgconfig = "libsodium", optional = "sodium extension",
      packages = { brew = "libsodium", apt = "libsodium-dev", dnf = "libsodium-devel" } },
    { pkgconfig = "gmp", optional = "gmp extension",
      packages = { brew = "gmp", apt = "libgmp-dev", dnf = "gmp-devel" } },
    { pkgconfig = "readline", optional = "readline line editing",
      packages = { brew = "readline", apt = "libreadline-dev", dnf = "readline-devel" } },
}
