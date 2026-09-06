# Tool Plugin Development

A tool plugin manages one versioned tool using Lua lifecycle hooks. Use a
[backend plugin](/backend-plugin-development.html) for an integration that manages several
tools, or an [environment plugin](/env-plugin-development.html) for variables without an
installation. Check the built-in [backends](/dev-tools/backends/) before writing an installer.

The [tool plugin template](https://github.com/jdx/mise-tool-plugin-template) supplies a
starting layout and development tooling. mise embeds Lua 5.1; its supported vfox hooks
and extensions are described here. Sharing a plugin with upstream vfox requires testing
there as well, particularly when using mise-specific modules or metadata.

## What are Tool Plugins?

Tool plugins can download archives, compile sources, return environment entries, and parse
idiomatic version files. The Lua runtime runs on Windows, macOS, and Linux; each plugin
must implement the artifact selection and external commands needed for those targets.

Plugins run with the user's permissions. Keep metadata free of host probes and avoid
changing global package-manager configuration from an installation hook.

## Plugin Architecture

```mermaid
flowchart LR
    A[Available: list versions] --> B[Resolve a version]
    B --> C[PreInstall: describe artifact]
    C --> D[mise: download, verify, extract]
    D --> E[PostInstall: optional setup]
    E --> F[EnvKeys: return environment]
```

A pinned or already-installed version can skip parts of this flow. Environment construction
can happen again on later invocations; it is not a one-time installation callback.

## Hook Functions

### Required Hooks

#### Available Hook

Return an array **newest first**, ordered by the publisher's release policy. mise reverses
this list for its internal oldest-first version listing. This differs from
`BackendListVersions`, which already returns oldest first.

```lua
-- hooks/available.lua
function PLUGIN:Available(ctx)
    return {
        {version = "1.10.0", note = "Current stable release"},
        {version = "1.2.0"},
    }
end
```

Do not discard prerelease suffixes or sort arbitrary versions with a shared SemVer parser.
`ctx.args` is available but mise does not supply interactive vfox arguments here.

##### Rolling Releases

For a channel whose contents change without changing its name, return `rolling = true`
and an asset checksum that changes with the channel's artifact:

```lua
function PLUGIN:Available(ctx)
    return {
        {
            version = "nightly",
            rolling = true,
            checksum = "REPLACE_WITH_CURRENT_PLATFORM_ASSET_SHA256",
        },
    }
end
```

The checksum above is a placeholder. Fetch the actual checksum for the selected platform.
`mise upgrade` compares rolling checksums, and `mise upgrade --bump` preserves the channel
name. This update marker is separate from the artifact checksum returned by `PreInstall`.

#### PreInstall Hook

Return the URL and verification metadata for `ctx.version`. mise downloads and extracts
the main artifact. `ctx.options` contains typed tool options; use `RUNTIME` for platform
information, including when mise asks for another platform's lockfile entry.

```lua
-- hooks/pre_install.lua: an illustrative Linux x64 release layout
function PLUGIN:PreInstall(ctx)
    if RUNTIME.osType ~= "linux" or RUNTIME.archType ~= "amd64" then
        error("This example artifact supports Linux x64 only")
    end
    local filename = "example-" .. ctx.version .. "-linux-x64.tar.gz"
    return {
        version = ctx.version,
        url = "https://downloads.example.com/" .. filename,
        sha256 = ctx.options.sha256 or error("sha256 option is required"),
    }
end
```

Replace the publisher URL and provide its trusted SHA-256 digest. Never put an ellipsis or
dummy checksum in a working installer. A URL with no strong checksum or supported
attestation does not establish artifact integrity. SHA-256 and SHA-512 are supported;
legacy SHA-1/MD5 do not satisfy strong-verification requirements.

For supported attestations, return an `attestation` table. For example:

```lua
local attestation = {
    github_owner = "your-org",
    github_repo = "your-tool",
    -- Optional: constrain the publishing workflow.
    github_signer_workflow = "your-org/your-tool/.github/workflows/release.yml",
}
```

Assign this table to the `attestation` field in `PreInstall`'s response. Other supported
fields include `cosign_sig_or_bundle_path` with optional `cosign_public_key_path`, and
`slsa_provenance_path` with optional `slsa_min_level`. Supply real verification inputs for
the chosen method. Do not combine unrelated placeholder methods into one example.

mise's lifecycle processes the main artifact; do not rely on upstream vfox `addition`
entries to install a second SDK. Use tool dependencies or implement the additional work
explicitly when needed.

#### EnvKeys Hook

Return `{key, value}` entries. `ctx.path` is the installation path and `ctx.version` is the
selected version. `ctx.main`, `ctx.sdkInfo`, and typed `ctx.options` are also available;
`ctx.runtimeVersion` is not part of this hook's context.

```lua
-- hooks/env_keys.lua
function PLUGIN:EnvKeys(ctx)
    local file = require("file")
    return {
        {key = "EXAMPLE_HOME", value = ctx.path},
        {key = "PATH", value = file.join_path(ctx.path, "bin")},
    }
end
```

Multiple PATH entries are merged. Return directories, not a replacement containing the
entire inherited PATH. Avoid network access or other expensive work in this hook.

### Optional Hooks

#### PostInstall Hook

Use `ctx.rootPath` for the extracted installation directory. `ctx.sdkInfo` describes the
main SDK, and `ctx.options` contains tool options. The compatibility field
`ctx.runtimeVersion` holds the requested tool version, not the mise application version.

```lua
-- hooks/post_install.lua
function PLUGIN:PostInstall(ctx)
    local file = require("file")
    if not file.exists(file.join_path(ctx.rootPath, "bin", "example")) then
        error("Expected bin/example in the extracted archive")
    end
end
```

The check above assumes a Unix executable layout. Archives normally carry executable
permissions; only change them when the actual distribution requires it.

#### PreUse Hook

mise does not implement the upstream vfox `PreUse` hook. Do not rely on it to rewrite a
version, observe shell changes, or perform activation work. Resolve version requests using
supported version listing/aliases and return environment entries through `EnvKeys`.

#### ParseLegacyFile Hook

Declare filenames in `metadata.lua`, implement the parser, and ask users to enable
[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools)
for the plugin's installed name. Return a version request without changing its meaning:

```lua
-- hooks/parse_legacy_file.lua
function PLUGIN:ParseLegacyFile(ctx)
    local file = require("file")
    local contents = file.read(ctx.filepath)
    local version = contents:match("^%s*([^\r\n]+)")
    if version then
        version = version:match("^%s*(.-)%s*$")
    end
    return {version = version}
end
```

This parser supports a single-line version request, including channels and prereleases.
Adapt it to the file's real format. `ctx.filename` is the basename and `ctx.filepath` is the
full path. Despite its name, the compatibility method `ctx:getInstalledVersions()` calls
`Available`; it is not an inventory of installed versions and can perform network requests.

## Creating a Tool Plugin

### Using the Template Repository

Create a repository from the [tool template](https://github.com/jdx/mise-tool-plugin-template),
or clone it to inspect and customize its files. Choose a plugin name that does not collide
with a core tool or an existing plugin while testing.

### 1. Plugin Structure

```text
my-tool-plugin/
├── metadata.lua
├── hooks/
│   ├── available.lua
│   ├── pre_install.lua
│   ├── env_keys.lua
│   ├── post_install.lua       # optional
│   └── parse_legacy_file.lua  # optional
└── lib/
    └── helper.lua            # optional shared code
```

### 2. metadata.lua

```lua
PLUGIN = {
    name = "my-tool",
    version = "1.0.0", -- plugin release, separate from the tool version
    description = "Install Example Tool",
    author = "Plugin Author",
    legacyFilenames = {".example-version"},
    -- Add only real installation prerequisites, if any:
    -- depends = {"go", "make"},
}
```

`depends` exposes matching configured tools to installation hooks and orders their install
jobs. Users must configure those tools; the metadata does not choose their versions. Avoid
self-dependencies. This differs from a tool's `[tools]` `depends` option, which orders the
configured install graph.

#### System Dependencies

Plugins that compile from source (or otherwise rely on system libraries and build tools) can declare those prerequisites with `systemDependencies`. Before installing the tool, mise checks each one and — depending on the [`system_deps`](/configuration/settings.html#system_deps) setting — reports, offers to install, or auto-installs anything missing.

```lua
PLUGIN = {
    name = "php",
    version = "1.0.0",

    systemDependencies = {
        -- an executable on PATH, with an optional version constraint
        { bin = "bison", version = ">=3.0",
          packages = { brew = "bison", apt = "bison", dnf = "bison" } },
        { bin = "re2c",
          packages = { brew = "re2c", apt = "re2c", dnf = "re2c" } },

        -- a library discoverable via pkg-config
        { pkgconfig = "libxml-2.0",
          packages = { brew = "libxml2", apt = "libxml2-dev", dnf = "libxml2-devel" } },
        { pkgconfig = "openssl",
          packages = { brew = "openssl@3", apt = "libssl-dev", dnf = "openssl-devel" } },

        -- a runtime shared library, by soname (Linux). apt renamed this
        -- package in the 64-bit time_t transition, so list both names and
        -- let mise pick the one that exists.
        { sharedlib = "libaio.so.1",
          packages = { apt = { "libaio1t64", "libaio1" }, dnf = "libaio" } },

        -- an escape hatch: any shell command whose exit status 0 means "satisfied"
        { command = "xcode-select -p", optional = "macOS command line tools" },
    },
}
```

Each entry must set **exactly one** check:

| Check       | Detection                                          | Use for                                    |
| ----------- | -------------------------------------------------- | ------------------------------------------ |
| `bin`       | executable resolvable on `PATH`                    | compilers, build tools, `*-config` scripts |
| `pkgconfig` | `pkg-config --exists <name>`                       | C libraries that ship a `.pc` file         |
| `sharedlib` | dynamic linker can resolve the soname (Linux only) | runtime libraries for prebuilt binaries    |
| `command`   | the shell command exits `0`                        | anything the above can't express           |

Optional fields:

- **`version`** — a constraint (`>=3.0`, `>3`, `<=1.2`, `=3.0`, or a bare `3.0` meaning `>=3.0`) for `bin` and `pkgconfig`. mise runs `<bin> --version` / `pkg-config --modversion` and compares. If a version can't be extracted, the dependency is treated as satisfied (presence is enough) rather than blocking the install.
- **`optional`** — a short reason string. Missing optional dependencies never prompt or fail; they surface as a single informational line, letting users build without features they don't need (e.g. Erlang's `wxWidgets` GUI).
- **`packages`** — a map of package-manager name (`brew`, `brew-cask`, `apt`, `dnf`, `pacman`, `apk`, `flatpak`, `flatpak-user`, `mas`) to the package that provides the capability. A value is either a single package name (`apt = "bison"`) or a list of candidates (`apt = { "libaio1t64", "libaio1" }`) when the same capability is packaged under different names across distro releases. Order candidates newest-name-first: mise picks the first one the manager actually has, and falls back to the first listed if it cannot tell. Only managers that can be queried for package availability (currently `apt`) do this selection; the others always use the first candidate, so a single name remains the right choice for them.

**Never probe the host from `metadata.lua`.** Its top level runs every time mise loads the plugin's metadata, so a shell-out there (checking which package name exists, reading the distro version) is paid on many mise invocations, and its result is cached alongside the metadata — freezing a machine-specific answer that goes stale when the user upgrades their OS. Declare candidates instead and let mise resolve them, which it does lazily: only for a dependency that actually failed its check, at the point it is about to install packages anyway.

**Detection is the source of truth.** A check that passes is satisfied no matter how the capability was installed — Homebrew, apt, nix, MacPorts, or from source all pass without ceremony, and mise never asks _how_ it got there. The `packages` map is only consulted to _offer_ installing the missing subset; it is a remediation hint, not a declaration that the tool must come from that package manager.

These declarations are inert on older mise versions and on upstream vfox (both ignore unknown `PLUGIN` fields), so adding them is backward-compatible.

### 3. Helper Libraries

Use Lua helpers for publisher-specific platform naming. The runtime reports `darwin` for
macOS and `amd64` for x64; an upstream archive may spell those differently. Map only the
platforms the publisher actually supports and reject others explicitly.

```lua
-- lib/platform.lua
local M = {}
function M.archive_platform()
    local os_names = {darwin = "macos", linux = "linux", windows = "windows"}
    local arches = {amd64 = "x64", arm64 = "arm64"}
    local os_name = os_names[RUNTIME.osType] or error("Unsupported OS: " .. RUNTIME.osType)
    local arch = arches[RUNTIME.archType] or error("Unsupported architecture: " .. RUNTIME.archType)
    return os_name .. "-" .. arch
end
return M
```

## Real-World Example: vfox-nodejs

Study [vfox-nodejs](https://github.com/version-fox/vfox-nodejs) for an upstream implementation.
For normal Node use, prefer mise's [core Node backend](/lang/node.html). A Node plugin must
handle the following details rather than copying a fixed Linux archive URL.

### Available Hook Example

Node's release index contains version strings and release metadata. Check the HTTP status
before decoding it, preserve the index's release order, and remove only its known leading
`v`. Keep prerelease identifiers intact. The [HTTP and JSON modules](/plugin-lua-modules.html)
provide the request and decoding APIs.

### PreInstall Hook Example

Select the exact archive for the target OS/architecture, then match its filename exactly
in `SHASUMS256.txt`. A filename contains Lua pattern characters such as `.` and `-`, so
`line:match(filename)` is not an exact filename check. For example:

```lua
local function find_checksum(body, filename)
    for line in body:gmatch("[^\r\n]+") do
        local digest, name = line:match("^(%x+)%s+%*?(.+)$")
        if name == filename and #digest == 64 then
            return digest
        end
    end
    error("No SHA-256 entry for " .. filename)
end
```

Fail if the checksum is missing. Do not silently continue with `sha256 = nil` after a
failed request. Obtaining a checksum from the same server is an integrity check, not the
same guarantee as verifying Node's signed checksum manifest.

### EnvKeys Hook Example

Node archives place executables in `bin` on Unix and at the installation root on Windows.
Use the correct directory:

```lua
function PLUGIN:EnvKeys(ctx)
    local file = require("file")
    local bin = RUNTIME.osType == "windows" and ctx.path or file.join_path(ctx.path, "bin")
    return {
        {key = "NODE_HOME", value = ctx.path},
        {key = "PATH", value = bin},
    }
end
```

### PostInstall Hook Example

Avoid running `npm config set` without a deliberate configuration scope: it can change the
user's npm configuration outside the tool installation. Prefer returning environment
entries if the plugin needs a specific npm prefix or cache. Test the actual archive layout
before adding permission changes or setup commands.

### Legacy File Support

Node version files can contain aliases such as `lts/*`, prefixes, and prereleases. Do not
extract only digits and dots. A parser must preserve the request and the plugin must support
resolving it; otherwise report the unsupported value instead of selecting a different release.

## Testing Your Plugin

### Local Development

Use a separate test project and a plugin name that will not replace your normal tools:

```sh
mise plugin link my-tool /path/to/my-tool-plugin
mise ls-remote my-tool
mise use my-tool@1.0.0
mise exec -- example --version
```

Replace `1.0.0` with a published test version and `example` with the executable it provides.
For version-file tests, use another empty project with no competing `[tools]` pin:

```toml
[settings]
idiomatic_version_file_enable_tools = ["my-tool"]
```

Write a supported request to `.example-version`, run `mise install`, and verify
`mise exec -- example --version`. `mise use my-tool` would write a tool selection, so it
is not a test of whether the version file controls resolution.

### Debug Mode

```sh
MISE_DEBUG=1 mise install my-tool@1.0.0
mise cache clear my-tool
```

Clear the tool's cache when cached metadata or version results hide a local hook edit.

### Plugin Test Script

Use the isolated [publishing test workflow](/plugin-publishing.html#testing-before-publication).
Exercise version listing, a concrete install, executable lookup, and environment values.
Also test unsupported platforms, missing checksums, malformed metadata, paths with spaces,
and idiomatic files if supported. Run each advertised OS in CI.

## Best Practices

### Error Handling

Use `http.try_get` for a recoverable transport failure, and check HTTP status before parsing.
Synchronous operations such as `json.decode` and `cmd.exec` raise catchable Lua errors.
Never log tokens or a secret-bearing response body just to explain a failed request.

### Platform Detection

Use the injected runtime instead of spawning `uname`:

| Field                   | Values or meaning                                          |
| ----------------------- | ---------------------------------------------------------- |
| `RUNTIME.osType`        | `windows`, `linux`, `darwin`                               |
| `RUNTIME.archType`      | `amd64`, `arm64`, `x86`, and other supported architectures |
| `RUNTIME.envType`       | `gnu` or `musl` on detected Linux systems; otherwise `nil` |
| `RUNTIME.version`       | Embedded vfox runtime version                              |
| `RUNTIME.pluginDirPath` | Plugin source directory                                    |

### Version Normalization

Normalize only a documented publisher convention, such as a leading `v`. Treat the rest
of the version as opaque. Removing `-beta.1` changes a prerelease into a different request.

### Caching

mise caches remote version lists and environment results. A module-level Lua table only
lasts for that runtime and cannot provide a cache shared by separate mise invocations.
Keep metadata declarative and see [cache behavior](/cache-behavior.html) for refresh controls.

## Advanced Features

### Conditional Installation

Choose the archive using `RUNTIME` and the exact requested version. `PreInstall` can be
called for lockfile generation on another target platform; avoid probing the host or
installing dependencies just to calculate an artifact URL.

### Source Compilation

Compile only in the installation phase. Declare prerequisites, use `cmd.exec` with a `cwd`
option, and pass paths through correctly quoted arguments or environment variables.
Document whether the build needs a POSIX shell. Commands such as `nproc`, `chmod`, and
`./configure` do not form a portable Windows build recipe.

### Environment Configuration

Return only the variables the tool needs. PATH entries are directories; setting unrelated
variables such as `LD_LIBRARY_PATH` can affect every process launched in the environment.
For variables unrelated to a tool version, use an [environment plugin](/env-plugin-development.html).

## Next Steps

- [Backend Plugin Development](/backend-plugin-development.html).
- [Plugin Lua Modules](/plugin-lua-modules.html).
- [Plugin Publishing](/plugin-publishing.html).
