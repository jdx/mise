# Plugin Lua Modules

mise's embedded Lua 5.1 runtime provides modules for plugin hooks, including backend, tool,
environment, and package plugins. This reference describes mise's implementations; upstream
vfox may differ. Load modules with `require` and use `RUNTIME` for the target platform.

Use direct HTTP and file operations when possible. `cmd.exec` runs a shell, so command
quoting and external prerequisites still depend on the selected platform.

## Available Modules

### Core Modules

- **`cmd`** - Execute shell commands
- **`json`** - Parse and generate JSON
- **`http`** - Make HTTP requests and downloads
- **`file`** - File system operations
- **`env`** - Environment variable operations
- **`strings`** - String manipulation utilities
- **`semver`** - Numeric-component comparison and sorting (not full SemVer precedence)
- **`html`** - HTML parsing and manipulation
- **`archiver`** - Archive extraction
- **`log`** - Structured logging

## HTTP Module

The HTTP module makes web requests and downloads files. `get` and `head` return a response
or raise on a transport failure; a non-2xx HTTP response is still a response, so check
`status_code`. `download_file` raises on transport and HTTP error status and returns no
value on success. Use the non-raising `try_*` variants for fallback logic.

### Basic HTTP Requests

```lua
local http = require("http")

-- GET request
local resp = http.get({
    url = "https://api.github.com/repos/owner/repo/releases",
    headers = {
        ['User-Agent'] = "mise-plugin",
        ['Accept'] = "application/json"
    }
})


if resp.status_code ~= 200 then
    error("HTTP error: " .. resp.status_code)
end

local body = resp.body
```

### HEAD Requests

```lua
local http = require("http")

-- HEAD request to check file info
local resp = http.head({
    url = "https://example.com/file.tar.gz"
})


local content_length = resp.headers['content-length']
local content_type = resp.headers['content-type']
```

### File Downloads

```lua
local http = require("http")

-- Download file
local err = http.download_file({
    url = "https://github.com/owner/repo/archive/v1.0.0.tar.gz",
    headers = {
        ['User-Agent'] = "mise-plugin"
    }
}, "/path/to/download.tar.gz")

if err ~= nil then
    error("Download failed: " .. err)
end
```

### Non-Raising Variants (`try_*`)

The standard `http.get`, `http.head`, and `http.download_file` methods raise a Lua error on transport failures (timeouts, DNS errors, connection refused, etc.). Since `pcall()` cannot catch errors from async functions in this environment, non-raising variants are provided:

```lua
local http = require("http")

-- try_get: returns (resp, nil) on success, (nil, err_string) on failure
local resp, err = http.try_get({
    url = "https://primary.example.com/index"
})
if err ~= nil then
    -- fall back to another source
    resp, err = http.try_get({ url = "https://fallback.example.com/index" })
end

-- try_head: same return convention as try_get
local resp, err = http.try_head({ url = "https://example.com/file.tar.gz" })

-- try_download_file: returns (true, nil) on success, (nil, err_string) on failure
local ok, err = http.try_download_file({
    url = "https://example.com/archive.tar.gz"
}, "/path/to/download.tar.gz")
if err ~= nil then
    error("Download failed: " .. err)
end
```

### Response Object

HTTP responses contain the following fields:

```lua
{
    status_code = 200,
    headers = {
        ['content-type'] = "application/json",
        ['content-length'] = "1234"
    },
    body = "response content"
}
```

## JSON Module

The JSON module encodes and decodes JSON.

### Basic Usage

```lua
local json = require("json")

-- Encode table to JSON string
local obj = {
    name = "mise-plugin",
    version = "1.0.0",
    tools = {"prettier", "eslint"}
}
local jsonStr = json.encode(obj)
-- Result: '{"name":"mise-plugin","version":"1.0.0","tools":["prettier","eslint"]}'

-- Decode JSON string to table
local decoded = json.decode(jsonStr)
print(decoded.name)  -- "mise-plugin"
print(decoded.tools[1])  -- "prettier"
```

### Error Handling (Lua)

```lua
local json = require("json")

-- Safe JSON parsing
local success, result = pcall(json.decode, response_body)
if not success then
    error("Failed to parse JSON: " .. result)
end

-- Use the parsed data
for _, item in ipairs(result) do
    print(item.version)
end
```

## Strings Module

The strings module provides string manipulation utilities.

### String Operations

```lua
local strings = require("strings")

-- Split string into parts
local parts = strings.split("hello,world,test", ",")
print(parts[1])  -- "hello"
print(parts[2])  -- "world"
print(parts[3])  -- "test"

-- Join strings
local joined = strings.join({"hello", "world", "test"}, " - ")
print(joined)  -- "hello - world - test"

-- Trim whitespace
local trimmed = strings.trim_space("  hello world  ")
print(trimmed)  -- "hello world"
```

### String Checks

```lua
local strings = require("strings")

-- Check prefixes and suffixes
local text = "hello world"
print(strings.has_prefix(text, "hello"))  -- true
print(strings.has_suffix(text, "world"))  -- true
print(strings.contains(text, "lo wo"))    -- true

-- Remove repeated exact suffixes (not a character set)
local trimmed = strings.trim("hello world", "world")
print(trimmed)  -- "hello "
```

### Version String Utilities

Use Lua patterns to remove a known publisher prefix. The module has no `trim_prefix`
function, and stripping a prerelease suffix would change the requested version:

```lua
local function normalize_version(version)
    return (version:gsub("^v", ""))
end
local version = normalize_version("v1.2.3-beta.1") -- "1.2.3-beta.1"
```

## Semver Module

Despite its name, this module compares **numeric components extracted from strings**, not
full Semantic Versioning precedence. It ignores non-digit text and treats missing numeric
components as zero. For example, `1.0.0-beta` compares equal to `1.0.0`, and `1.0.0-beta.1`
compares greater. Do not use it to choose the newest arbitrary tool version or order channels.

Use it only when a tool's documented version scheme matches this numeric comparison.
Otherwise preserve the publisher's order or implement that tool's actual policy.

### Version Comparison

```lua
local semver = require("semver")

-- Compare two versions
-- Returns: -1 if v1 < v2, 0 if equal, 1 if v1 > v2
local result = semver.compare("1.2.3", "1.2.4")  -- -1
local result = semver.compare("2.0.0", "1.9.9")  -- 1
local result = semver.compare("1.0.0", "1.0.0")  -- 0

-- Handles numeric comparison correctly
local result = semver.compare("9.6.9", "9.6.24")   -- -1 (not lexicographic!)
local result = semver.compare("10.0.0", "9.6.24") -- 1
```

### Parse Version

```lua
local semver = require("semver")

-- Parse version string into numeric parts
local parts = semver.parse("1.2.3")
print(parts[1])  -- 1
print(parts[2])  -- 2
print(parts[3])  -- 3

-- Non-digit text is discarded; this is not a SemVer parser
local parts = semver.parse("v1.2.3-beta")  -- {1, 2, 3}
```

### Sort Version Strings

```lua
local semver = require("semver")

-- Sort array of version strings (ascending order)
local versions = {"1.10.0", "1.2.0", "1.9.0", "2.0.0"}
local sorted = semver.sort(versions)
-- Result: {"1.2.0", "1.9.0", "1.10.0", "2.0.0"}
```

### Sort Tables by Version Field

```lua
local semver = require("semver")

-- Sort array of tables by a version field (ascending order)
local releases = {
    {version = "1.10.0", url = "..."},
    {version = "1.2.0", url = "..."},
    {version = "1.9.0", url = "..."},
}
local sorted = semver.sort_by(releases, "version")
-- Result: sorted by version ascending
```

### Real-World Example: Available Hook

This sketch applies only to releases made of three numeric components, with no prereleases
or channels. Prefer a structured release API over scraping text when one is available.

```lua
local http = require("http")
local semver = require("semver")

function PLUGIN:Available(ctx)
    local resp = http.get({
        url = "https://example.com/releases/"
    })


    assert(resp.status_code == 200, "Release request failed")
    local result = {}
    -- Parse versions from response...
    for version in string.gmatch(resp.body, 'v([0-9]+%.[0-9]+%.[0-9]+)') do
        table.insert(result, {version = version})
    end

    -- Available() must return newest-first. semver.sort_by() sorts ascending,
    -- so reverse that result before returning it.
    local sorted = semver.sort_by(result, "version")
    local newest_first = {}
    for i = #sorted, 1, -1 do
        table.insert(newest_first, sorted[i])
    end
    return newest_first
end
```

### Using Compare in Custom Sort

```lua
local semver = require("semver")

-- Sort with custom comparator (descending order - newest first)
table.sort(versions, function(a, b)
    return semver.compare(a.version, b.version) > 0
end)

-- Sort ascending (oldest first); reverse this before returning from Available()
table.sort(versions, function(a, b)
    return semver.compare(a.version, b.version) < 0
end)
```

## HTML Module

The HTML module returns selection objects, not Lua arrays. Use `:each(function(index,
node) ... end)` to iterate a selection, `:first()` for its first element, and `:eq(0)` for
its zero-based first position. `:text()` reads the first selected node's inner content
(which can include markup); `:attr(name)` reads its attribute.

### Basic HTML Parsing

```lua
local html = require("html")

-- Parse HTML document
local doc = html.parse([[
    <html>
        <body>
            <div id="version" class="info">1.2.3</div>
            <ul class="downloads">
                <li><a href="/download/v1.2.3.tar.gz">Source</a></li>
                <li><a href="/download/v1.2.3.zip">Windows</a></li>
            </ul>
        </body>
    </html>
]])

-- Extract text content
local version = doc:find("#version"):text()  -- "1.2.3"

-- Extract attributes
local links = doc:find("a")
links:each(function(index, link)
    local href = link:attr("href")
    print(index, link:text(), href)
end)
```

### CSS Selectors

```lua
local html = require("html")

local doc = html.parse(html_content)

-- Find by ID
local element = doc:find("#version")

-- Find by class
local elements = doc:find(".download-link")

-- Find by tag
local links = doc:find("a")

-- Complex selectors
local specific_links = doc:find("ul.downloads a[href$='.tar.gz']")
```

### Real-World Example: Scraping Releases

This illustrates selection traversal. Website HTML and duplicate links can change; prefer
a release API when available and deduplicate identifiers before returning a hook result.

```lua
local html = require("html")
local http = require("http")

function get_github_releases(owner, repo)
    local resp = http.get({
        url = "https://github.com/" .. owner .. "/" .. repo .. "/releases"
    })


    assert(resp.status_code == 200, "Release page request failed")
    local doc = html.parse(resp.body)
    local releases = {}

    -- Find all release tags
    local release_elements = doc:find("a[href*='/releases/tag/']")
    release_elements:each(function(index, element)
        local href = element:attr("href")
        local version = href:match("/releases/tag/(.+)")
        if version then
            table.insert(releases, {
                version = version,
                url = "https://github.com" .. href
            })
        end
    end)

    return releases
end
```

## Archiver Module

The archiver module extracts archives based on their filename suffix. It does not download
or authenticate the archive; verify the artifact before extracting it.

### Supported Formats

- **tar.gz** - Gzipped tar archives
- **tar.xz** - XZ compressed tar archives
- **tar.bz2** - Bzip2 compressed tar archives
- **zip** - ZIP archives

### Basic Extraction

```lua
local archiver = require("archiver")

-- Extract archive to directory
archiver.decompress("archive.tar.gz", "extracted/")

-- Failures raise Lua errors and stop the hook.
archiver.decompress("package.zip", "destination/")
```

To flatten versioned directories at the root of an archive, pass
`strip_components = 1`. Files already at the archive root are retained, matching
mise's built-in archive backends. Only `0` and `1` are supported; higher values raise an error.

```lua
archiver.decompress("node-v24.18.1-linux-x64.tar.gz", "destination/", {
    strip_components = 1,
})
```

### Real-World Example: Plugin Installation

```lua
local archiver = require("archiver")
local http = require("http")

function install_from_archive(download_url, install_path)
    -- Download the archive
    local archive_path = install_path .. "/download.tar.gz"
    http.download_file({
        url = download_url
    }, archive_path)

    -- Extract to installation directory
    archiver.decompress(archive_path, install_path)

    -- Clean up archive
    os.remove(archive_path)
end
```

## File Module

The file module provides file system operations.

### Path Joining

```lua
local file = require("file")

-- Join path segments using the OS-specific separator
local full_path = file.join_path("/foo", "bar", "baz.txt")
print(full_path)  -- On Unix: /foo/bar/baz.txt
```

`file.join_path` joins nonempty segments with the host path separator. It does not normalize
existing separators, resolve `..`, expand `~`, or make an untrusted path safe. Pass relative
segments after the base directory. For environment plugins, use `ctx.config_root` as the
base for project-relative options.

### Read File Contents

```lua
local file = require("file")
print(file.read("/path/to/file"))
```

`file.read` returns UTF-8 text or raises an error; it does not return `nil` for a missing file.

### Create Symbolic Links

```lua
local file = require("file")
file.symlink("/path/to/source", "/path/to/new-symlink")
```

### Check if file exists

```lua
local file = require("file")
if file.exists("important_file.txt") then
    print("File exists")
else
    print("File does not exist")
end
```

### List and match files

```lua
local file = require("file")

-- Immediate entries, returned in sorted order
local entries = file.list("/path/to/directory")

-- Paths matching a glob, returned in sorted order
local executables = file.glob(file.join_path("/path/to/bin", "mytool-*"))
```

### Move files and directories

`file.move` moves either a file or an entire directory. Parent directories for
the destination are created automatically.

```lua
local file = require("file")
file.move(
    file.join_path("/path/to/bin", "mytool-linux-amd64"),
    file.join_path("/path/to/bin", "mytool")
)
```

### File Metadata

`file.stat(path)` returns `nil` when the path is missing. Otherwise it returns `size`,
`is_file`, `is_dir`, `is_symlink`, and available `modified`, `accessed`, and `created` Unix
timestamps. It inspects the link itself. `mode` is an octal permission string on Unix and
`nil` on other platforms.

## Environment Module

`env.setenv` changes the mise process environment. It does not return a variable to the
user's shell, and it does not update an already-constructed hook environment. Prefer
returning values from `MiseEnv`, `EnvKeys`, or `BackendExecEnv`. For one child command, use
`cmd.exec(..., {env = {...}})` to avoid process-wide mutations.

### Set Environment Variable

```lua
local env = require("env")

-- Set environment variable
env.setenv("MY_VAR", "my_value")
```

### Get Environment Variable

> To read variables in Lua, use `os.getenv("MY_VAR")`.

### Path Operations

Return separate PATH entries from an environment hook. Use `file.join_path` to construct
paths and let mise merge them using the host's PATH separator. Do not prepend a Unix
colon-separated string to PATH in code that also runs on Windows.

## Command Module

`cmd.exec` runs a command through mise's configured default inline shell. It returns stdout
on success and raises an error containing stderr on failure. Successful stderr is not part
of the returned string. `pcall(cmd.exec, ...)` can intercept the error.

The string is shell code, not an argument array. Use `cwd` for the working directory and
quote external values for that shell; interpolating tool options into shell text can execute
unintended commands. `os.execute` streams output and returns the exit status using Lua 5.1
conventions (`0` for success), with the same mise-constructed environment.

### Basic Command Execution

```lua
local cmd = require("cmd")

-- Execute command and get output
local output = cmd.exec("ls -la")
print("Directory listing:", output)

-- Execute command with error handling
local success, output = pcall(cmd.exec, "some-command")
if not success then
    error("Command failed: " .. output)
end
```

### Command Execution with Options

```lua
local cmd = require("cmd")

-- Execute command in a specific directory
local output = cmd.exec("pwd", {cwd = "/tmp"})
print("Current directory:", output)

-- Execute command with custom environment variables
local result = cmd.exec("echo $TEST_VAR", {
    cwd = "/path/to/project",
    env = {TEST_VAR = "hello", NODE_ENV = "production"}
})

-- Install package in specific directory
local result = cmd.exec("npm install package-name", {cwd = "/path/to/project"})
```

### Available Options

The options table supports the following keys:

- **`cwd`** (string): Set the working directory for the command
- **`env`** (table): Set environment variables for the command. These are merged on top of the inherited environment (see below).
- **`timeout`**: Currently ignored. Do not rely on it to terminate a command.

### Environment Inheritance in Env Module Hooks

When `cmd.exec()` is called from environment module hooks (`MiseEnv`, `MisePath`), the command automatically inherits the mise-constructed environment instead of the process environment. This includes environment variables set by preceding directives and `_.path` entries accumulated so far.

When the module directive has `tools = true`, the inherited environment also includes the bin paths of installed tools, so mise-managed tools can be called directly:

```toml
[env]
_.my-plugin = { tools = true }
```

```lua
function PLUGIN:MiseEnv(ctx)
    local cmd = require("cmd")
    -- With tools=true, mise-managed tools are on PATH
    local version = cmd.exec("node --version")
    return {
        {key = "NODE_VERSION", value = version:gsub("%s+", "")}
    }
end
```

Without `tools = true`, only `_.path` directive entries and the original system PATH are available to `cmd.exec()`.

Any explicit `env` options passed to `cmd.exec()` are merged on top of the inherited environment, allowing selective overrides.

### Platform-Specific Commands

```lua
local cmd = require("cmd")

-- Cross-platform command execution
local function is_windows()
    return package.config:sub(1,1) == '\\'
end

local function get_os_info()
    if is_windows() then
        return cmd.exec("systeminfo")
    else
        return cmd.exec("uname -a")
    end
end

local os_info = get_os_info()
print("OS Info:", os_info)
```

## Practical Examples

### Version Fetching from API

This helper collects version identifiers. An unordered JSON object does not establish
oldest/newest order, and lexicographic sorting misorders `1.10.0` and `1.2.0`.

```lua
local http = require("http")
local json = require("json")

function fetch_npm_versions(package_name)
    local resp = http.get({
        url = "https://registry.npmjs.org/" .. package_name,
        headers = {
            ['User-Agent'] = "mise-plugin"
        }
    })


    assert(resp.status_code == 200, "Package metadata request failed")
    local package_info = json.decode(resp.body)
    local versions = {}

    for version, _ in pairs(package_info.versions) do
        table.insert(versions, version)
    end

    -- The JSON object has no release order. Return the collected identifiers;
    -- callers must apply npm's actual release policy before using this as a hook.
    return versions
end
```

### Download and Verification {#file-download-with-progress}

`http.download_file` downloads bytes; checking that the destination exists is not checksum
verification. A tool plugin should return the trusted digest in `PreInstall.sha256` or
`PreInstall.sha512` so mise verifies before extraction. A backend plugin performing its own
download must implement verification explicitly. Do not accept an `expected_sha256` argument
and then ignore it.

### Configuration File Parsing

```lua
local file = require("file")
local json = require("json")
local strings = require("strings")

function parse_config_file(config_path)
    if not file.exists(config_path) then
        return {}  -- Return empty config
    end

    local content = file.read(config_path)
        -- Trim whitespace
    content = strings.trim_space(content)

    -- Parse JSON
    local success, config = pcall(json.decode, content)
    if not success then
        error("Invalid JSON in config file: " .. config_path)
    end

    return config
end
```

### Web Scraping for Versions

```lua
local http = require("http")
local html = require("html")
local strings = require("strings")

function scrape_versions_from_releases(base_url)
    local resp = http.get({
        url = base_url .. "/releases"
    })


    assert(resp.status_code == 200, "Release page request failed")
    local doc = html.parse(resp.body)
    local versions = {}

    -- Find version tags
    local version_elements = doc:find("h2 a[href*='/releases/tag/']")
    version_elements:each(function(index, element)
        local version_text = element:text()
        local version = strings.trim_space(version_text)

        -- Remove 'v' prefix if present
        version = version:gsub("^v", "")

        if version and version ~= "" then
            table.insert(versions, {
                version = version,
                url = base_url .. element:attr("href")
            })
        end
    end)

    return versions
end
```

## Log Module

The log module provides structured logging that routes through Rust's `log` crate and respects the `MISE_DEBUG` and `MISE_TRACE` environment variables.

### Log Levels

```lua
local log = require("log")

log.trace("detailed tracing info")   -- only visible with MISE_TRACE=1
log.debug("debugging info")          -- visible with MISE_DEBUG=1
log.info("status message")           -- visible by default
log.warn("warning message")          -- visible by default
log.error("error message")           -- visible by default
```

### Variadic Arguments

All log functions accept multiple arguments of any type. Arguments are converted to strings via `tostring()` and joined with tab characters (`\t`), matching Lua's `print()` behavior:

```lua
log.info("version", version, "installed to", path)
-- Output: [plugin-name] version<TAB>1.0.0<TAB>installed to<TAB>/path
```

### Plugin Name Prefix

All log messages are automatically prefixed with `[plugin_name]`:

```
mise [INFO] [my-plugin] Installing version 1.0.0
```

### Print Override

`print()` is overridden to route through `info!()` level logging. This means:

- `print()` output goes to stderr instead of stdout
- Messages are prefixed with `[plugin_name]`
- Output respects log level filtering

```lua
-- These are equivalent:
print("hello", "world")
log.info("hello", "world")
```

### Accessing via vfox Namespace

The log module is also available as `vfox.log`:

```lua
local log = require("vfox").log
log.info("message")
```

## Best Practices

### Error Handling

Always handle errors gracefully:

```lua
local http = require("http")
local json = require("json")

function safe_api_call(url)
    local resp = http.get({url = url})


    if resp.status_code ~= 200 then
        error("API returned error: " .. resp.status_code)
    end

    local success, data = pcall(json.decode, resp.body)
    if not success then
        error("Failed to parse JSON response: " .. data)
    end

    return data
end
```

### Caching

A local Lua table can avoid repeated work within one runtime. It does not persist between
separate mise invocations. mise already caches tool version and environment results;
environment plugins can also return [cache metadata](/env-plugin-development.html#hooks-mise-env-lua).
The example below is only an in-memory cache:

```lua
local cache = {}
local cache_ttl = 3600  -- 1 hour

function cached_http_get(url)
    local now = os.time()
    local cache_key = url

    -- Check cache
    if cache[cache_key] and (now - cache[cache_key].timestamp) < cache_ttl then
        return cache[cache_key].data
    end

    -- Fetch fresh data
    local http = require("http")
    local resp = http.get({url = url})


    assert(resp.status_code == 200, "Request failed")
    -- Cache the result
    cache[cache_key] = {
        data = resp,
        timestamp = now
    }

    return resp
end
```

### Platform Detection

Use runtime metadata instead of subprocesses or ambient host variables:

```lua
local platform = {
    os = RUNTIME.osType,
    arch = RUNTIME.archType,
    libc = RUNTIME.envType,
}
```

`RUNTIME` may describe another target during lockfile generation. Shelling out to `uname`
would report the host and can produce the wrong artifact URL for that target.

## Next Steps

- [Backend Plugin Development](backend-plugin-development.md)
- [Tool Plugin Development](tool-plugin-development.md)
- [Publishing your plugin](plugin-publishing.md)
