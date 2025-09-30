# Plugin Lua Modules

mise plugins have access to a comprehensive set of built-in Lua modules that provide common functionality. These modules are available in both backend plugins and tool plugins, making it easy to perform common operations like HTTP requests, JSON parsing, file operations, and more.

## Available Modules

### Core Modules

- **`cmd`** - Execute shell commands
- **`json`** - Parse and generate JSON
- **`http`** - Make HTTP requests and downloads
- **`file`** - File system operations
- **`env`** - Environment variable operations
- **`strings`** - String manipulation utilities
- **`html`** - HTML parsing and manipulation
- **`archiver`** - Archive extraction

## HTTP Module

The HTTP module provides functionality for making web requests and downloading files.

### Basic HTTP Requests

```lua
local http = require("http")

-- GET request
local resp, err = http.get({
    url = "https://api.github.com/repos/owner/repo/releases",
    headers = {
        ['User-Agent'] = "mise-plugin",
        ['Accept'] = "application/json"
    }
})

if err ~= nil then
    error("Request failed: " .. err)
end

if resp.status_code ~= 200 then
    error("HTTP error: " .. resp.status_code)
end

local body = resp.body
```

### HEAD Requests

```lua
local http = require("http")

-- HEAD request to check file info
local resp, err = http.head({
    url = "https://example.com/file.tar.gz"
})

if err ~= nil then
    error("HEAD request failed: " .. err)
end

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

The JSON module provides encoding and decoding functionality.

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

The strings module provides various string manipulation utilities.

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

-- Trim specific characters
local trimmed = strings.trim("hello world", "world")
print(trimmed)  -- "hello "
```

### Version String Utilities

```lua
local strings = require("strings")

-- Common version string operations
local function normalize_version(version)
    -- Remove 'v' prefix if present
    version = strings.trim_prefix(version, "v")

    -- Remove pre-release suffixes
    local parts = strings.split(version, "-")
    return parts[1]
end

local version = normalize_version("v1.2.3-beta.1")  -- "1.2.3"
```

## HTML Module

The HTML module provides HTML parsing capabilities.

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
for _, link in ipairs(links) do
    local href = link:attr("href")
    local text = link:text()
    print(text .. ": " .. href)
end
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

```lua
local html = require("html")
local http = require("http")

function get_github_releases(owner, repo)
    local resp, err = http.get({
        url = "https://github.com/" .. owner .. "/" .. repo .. "/releases"
    })

    if err ~= nil then
        error("Failed to fetch releases: " .. err)
    end

    local doc = html.parse(resp.body)
    local releases = {}

    -- Find all release tags
    local release_elements = doc:find("a[href*='/releases/tag/']")
    for _, element in ipairs(release_elements) do
        local href = element:attr("href")
        local version = href:match("/releases/tag/(.+)")
        if version then
            table.insert(releases, {
                version = version,
                url = "https://github.com" .. href
            })
        end
    end

    return releases
end
```

## Archiver Module

The archiver module provides functionality for extracting compressed archives.

### Supported Formats

- **tar.gz** - Gzipped tar archives
- **tar.xz** - XZ compressed tar archives
- **tar.bz2** - Bzip2 compressed tar archives
- **zip** - ZIP archives

### Basic Extraction

```lua
local archiver = require("archiver")

-- Extract archive to directory
local err = archiver.decompress("archive.tar.gz", "extracted/")
if err ~= nil then
    error("Extraction failed: " .. err)
end

-- Extract ZIP file
local err = archiver.decompress("package.zip", "destination/")
if err ~= nil then
    error("ZIP extraction failed: " .. err)
end
```

### Real-World Example: Plugin Installation

```lua
local archiver = require("archiver")
local http = require("http")

function install_from_archive(download_url, install_path)
    -- Download the archive
    local archive_path = install_path .. "/download.tar.gz"
    local err = http.download_file({
        url = download_url
    }, archive_path)

    if err ~= nil then
        error("Download failed: " .. err)
    end

    -- Extract to installation directory
    local err = archiver.decompress(archive_path, install_path)
    if err ~= nil then
        error("Extraction failed: " .. err)
    end

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
print(full_path)  -- On Unix: /foo/bar/baz.txt, on Windows: \foo\bar\baz.txt
```

The `file.join_path(...)` function joins any number of path segments using the correct separator for the current operating system. This is the recommended way to construct file paths in cross-platform plugins.

### Read File Contents

```lua
local file = require("file")
print(file.read("/path/to/file"))
```

### Create Symbolic Links

```lua
local file = require("file")
file.symlink("/path/to/source", "/path/to/new-symlink")
```

## Environment Module

The env module provides environment variable operations.

### Set Environment Variable

```lua
local env = require("env")

-- Set environment variable
env.setenv("MY_VAR", "my_value")
```

### Get Environment Variable

> To read variables in Lua, use `os.getenv("MY_VAR")`.

### Path Operations

```lua
local env = require("env")

-- Get current PATH
local current_path = os.getenv("PATH")

-- Add to PATH
local new_path = "/usr/local/bin:" .. current_path
env.setenv("PATH", new_path)

-- Platform-specific PATH separator
local separator = package.config:sub(1,1) == '\\' and ";" or ":"
local paths = {"/usr/local/bin", "/opt/bin", current_path}
env.setenv("PATH", table.concat(paths, separator))
```

## Command Module

The cmd module provides shell command execution.

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
- **`env`** (table): Set environment variables for the command execution
- **`timeout`** (number): Set a timeout for command execution (future feature)

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

```lua
local http = require("http")
local json = require("json")

function fetch_npm_versions(package_name)
    local resp, err = http.get({
        url = "https://registry.npmjs.org/" .. package_name,
        headers = {
            ['User-Agent'] = "mise-plugin"
        }
    })

    if err ~= nil then
        error("Failed to fetch package info: " .. err)
    end

    local package_info = json.decode(resp.body)
    local versions = {}

    for version, _ in pairs(package_info.versions) do
        table.insert(versions, version)
    end

    -- Sort versions (simple string sort)
    table.sort(versions)

    return versions
end
```

### File Download with Progress

```lua
local http = require("http")
local file = require("file")

function download_with_verification(url, dest_path, expected_sha256)
    -- Download file
    local err = http.download_file({
        url = url,
        headers = {
            ['User-Agent'] = "mise-plugin"
        }
    }, dest_path)

    if err ~= nil then
        error("Download failed: " .. err)
    end

    -- Verify file exists
    if not file.exists(dest_path) then
        error("Downloaded file not found")
    end

    -- Note: SHA256 verification would need additional implementation
    -- This is a simplified example
    print("Downloaded successfully to: " .. dest_path)
end
```

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
    if not content then
        error("Failed to read config file: " .. config_path)
    end

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
    local resp, err = http.get({
        url = base_url .. "/releases"
    })

    if err ~= nil then
        error("Failed to fetch releases page: " .. err)
    end

    local doc = html.parse(resp.body)
    local versions = {}

    -- Find version tags
    local version_elements = doc:find("h2 a[href*='/releases/tag/']")
    for _, element in ipairs(version_elements) do
        local version_text = element:text()
        local version = strings.trim_space(version_text)

        -- Remove 'v' prefix if present
        version = strings.trim_prefix(version, "v")

        if version and version ~= "" then
            table.insert(versions, {
                version = version,
                url = base_url .. element:attr("href")
            })
        end
    end

    return versions
end
```

## Best Practices

### Error Handling

Always handle errors gracefully:

```lua
local http = require("http")
local json = require("json")

function safe_api_call(url)
    local resp, err = http.get({url = url})

    if err ~= nil then
        error("HTTP request failed: " .. err)
    end

    if resp.status_code ~= 200 then
        error("API returned error: " .. resp.status_code .. " " .. resp.body)
    end

    local success, data = pcall(json.decode, resp.body)
    if not success then
        error("Failed to parse JSON response: " .. data)
    end

    return data
end
```

### Caching

Implement caching for expensive operations:

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
    local resp, err = http.get({url = url})

    if err ~= nil then
        error("HTTP request failed: " .. err)
    end

    -- Cache the result
    cache[cache_key] = {
        data = resp,
        timestamp = now
    }

    return resp
end
```

### Platform Detection

Handle cross-platform differences:

```lua
local function get_platform_info()
    local is_windows = package.config:sub(1,1) == '\\'
    local cmd = require("cmd")

    if is_windows then
        return {
            os = "windows",
            arch = os.getenv("PROCESSOR_ARCHITECTURE") or "x64",
            path_sep = "\\",
            env_sep = ";"
        }
    else
        local uname = cmd.exec("uname -s"):lower()
        local arch = cmd.exec("uname -m")

        return {
            os = uname,
            arch = arch,
            path_sep = "/",
            env_sep = ":"
        }
    end
end
```

## Next Steps

- [Backend Plugin Development](backend-plugin-development.md)
- [Tool Plugin Development](tool-plugin-development.md)
- [Publishing your plugin](plugin-publishing.md)
