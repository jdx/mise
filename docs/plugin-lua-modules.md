---
description: "Look up the globals, metadata fields, hook files and Lua modules that mise provides to plugin hooks."
---

# Plugin Lua reference

mise runs every Lua plugin hook (tool, backend, environment and package
plugins) in an embedded Lua 5.1 interpreter. It provides globals,
`metadata.lua` fields, hook files, and modules you load with `require`.

```lua
local http = require("http")
```

These are mise's implementations; a module with the same name in upstream vfox
can behave differently. Prefer the `http`, `file` and `archiver` modules to
shelling out: `cmd.exec` runs shell code, so quoting and the programs it needs
differ by platform.

## Globals

| Global                 | What it is                                                                                                                                                                                                     |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PLUGIN`               | The table `metadata.lua` defines. Hooks are methods on it, such as `function PLUGIN:PreInstall(ctx)`.                                                                                                          |
| `RUNTIME`              | The platform the hook targets. See [`RUNTIME`](#runtime).                                                                                                                                                      |
| `print(...)`           | The same function as `log.info`. It writes a log line to stderr, not to stdout. See [`print`](#print-override).                                                                                                |
| `os.execute(command)`  | Runs shell code in the [hook environment](#hook-environment) and returns its exit code. See [cmd](#command-module).                                                                                            |
| `os.getenv(name)`      | Reads the [hook environment](#hook-environment) in hooks that have one, and the environment of the mise process in the others.                                                                                 |
| `OS_TYPE`, `ARCH_TYPE` | The OS and architecture, with the same values as `RUNTIME.osType` and `RUNTIME.archType`. During `mise lock` they describe the target platform, as `RUNTIME` does. Prefer `RUNTIME`, which also has `envType`. |

The rest of the Lua 5.1 standard library behaves as usual, except that the
`debug` library is not loaded and `require` searches only the plugin's
directories (see [Loading your own code](#loading-your-own-code)). `io.popen`
always uses the environment of the mise process rather than the hook
environment, so run commands with `cmd.exec` instead.

### `RUNTIME` {#runtime}

| Field                   | Value                                                                                                                       |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `RUNTIME.osType`        | `linux`, `darwin` or `windows`                                                                                              |
| `RUNTIME.archType`      | `amd64`, `arm64`, or another architecture name such as `x86` or `riscv64`                                                   |
| `RUNTIME.envType`       | `gnu` or `musl` on Linux, from the [`libc`](/configuration/settings.html#libc) setting or detection; `nil` on other systems |
| `RUNTIME.version`       | The vfox API version mise implements (`0.6.0`), not the mise version                                                        |
| `RUNTIME.pluginDirPath` | The plugin's installed directory                                                                                            |

```lua
local platform = {
    os = RUNTIME.osType,
    arch = RUNTIME.archType,
    libc = RUNTIME.envType,
}
```

When [`mise lock`](/cli/lock.html) records other platforms, it runs `PreInstall`
for each of them, and `RUNTIME` describes that platform, with `envType` set to
`nil`. Use `RUNTIME` to pick an artifact. Running `uname` from a hook reports
the host and can select the wrong download for the platform being locked.

### Loading your own code

`require` looks for `NAME.lua` in the plugin's root directory, then in `hooks/`
and `lib/`, so `require("util")` loads `lib/util.lua`. Put shared helpers in
`lib/`.

The built-in modules also load under their upstream vfox names:
`require("vfox.cmd")`, `require("vfox.env")`, `require("vfox.semver")`,
`require("vfox.strings")` and `require("vfox").log`.

### The hook environment {#hook-environment}

mise builds an environment for each hook from your environment and the
project's `[env]` values. It adds the tool's `install_env` values during an
install, and the `PATH` entries of the tools the plugin
[depends on](/tool-plugin-development.html#depends). In the install, uninstall
and environment hooks, it also adds each [tool option](#tool-options) as a
`MISE_TOOL_OPTS__<KEY>` variable, which never reaches the user's shell;
`Available` and `BackendListVersions` do not get them. In tool and backend
hooks, mise removes its shims directory from that `PATH`, so a hook cannot reach
another tool through its shim. Package manager plugin hooks keep it; see
[package manager plugins](/package-plugin-development.html#mise-plugin-toml).

`os.getenv`, `os.execute`, `cmd.exec` and `cmd.stream` use this
environment. Values you pass in the `env` option of `cmd.exec` or `cmd.stream`
are merged over it.

`ParseLegacyFile`, `BackendListTools` and `BackendSearchTools` get no hook
environment: in them, `os.getenv` and commands see the environment of the mise
process, including values set with [`env.setenv`](#environment-module).

## metadata.lua {#metadata}

`metadata.lua` sits at the plugin's root and assigns the `PLUGIN` table:

```lua
PLUGIN = {
    name = "my-tool",
    version = "1.2.0",
    description = "Install Example Tool",
    homepage = "https://github.com/your-org/my-tool-plugin",
    license = "MIT",
    legacyFilenames = { ".my-tool-version" },
}
```

| Field                                          | Required | What mise does with it                                                                                                                                                                                                                                                                  |
| ---------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                                         | yes      | Becomes the key of `ctx.sdkInfo` in `PostInstall`, `EnvKeys` and `PreUninstall`. Commands and log lines use the name the plugin was installed under.                                                                                                                                    |
| `version`                                      | yes      | Nothing beyond the check that it is set. Users choose a release by its Git ref; see [Publishing plugins](/plugin-publishing.html#tag-a-release).                                                                                                                                        |
| `description`, `author`, `license`, `homepage` | no       | Nothing; they describe the plugin to readers of the file.                                                                                                                                                                                                                               |
| `legacyFilenames`                              | no       | Lists the [idiomatic version files](/dev-tools/versions.html#idiomatic-version-files) the tool plugin reads with `ParseLegacyFile`. mise reads them only for tools listed in [`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools). |
| `depends`                                      | no       | Lists tools to install before this one and put on `PATH` for its install hooks. See [`PLUGIN.depends`](/tool-plugin-development.html#depends).                                                                                                                                          |
| `systemDependencies`                           | no       | Lists system programs and libraries that mise checks for before an install. See [System dependencies](/tool-plugin-development.html#system-dependencies).                                                                                                                               |

mise stops with an error when `name` or `version` is missing. It runs the whole
file each time it loads the plugin, so keep it to assignments: do not run
commands or read the host there.

## Hook files

Each hook lives in its own file under `hooks/`, named after the hook in snake
case, and defines that method on `PLUGIN`: `hooks/pre_install.lua` defines
`PLUGIN:PreInstall`. mise runs every hook file when it loads the plugin, so
keep their top level to function definitions.

| Plugin type                                         | Required hook files                                                        | Optional hook files                                                                            |
| --------------------------------------------------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| [Tool](/tool-plugin-development.html)               | `available.lua`, `pre_install.lua`, `env_keys.lua`                         | `post_install.lua`, `pre_uninstall.lua`, `parse_legacy_file.lua`, `mise_install_satisfied.lua` |
| [Backend](/backend-plugin-development.html)         | `backend_list_versions.lua`, `backend_install.lua`, `backend_exec_env.lua` | `backend_uninstall.lua`, `backend_list_tools.lua`, `backend_search_tools.lua`                  |
| [Environment](/env-plugin-development.html)         | `mise_env.lua`, `mise_path.lua`, or both                                   | none                                                                                           |
| [Package manager](/package-plugin-development.html) | `package_installed.lua`, `package_install.lua`                             | `package_upgrade.lua`, `package_uninstall.lua`                                                 |

mise decides a plugin's type from these files. A plugin with
`hooks/backend_install.lua` is a backend plugin. One with both
`hooks/package_install.lua` and `hooks/package_installed.lua` is a package
manager plugin. Any other plugin with a `metadata.lua` is a tool or environment
plugin. Package manager plugins can also declare their capabilities in a
`mise.plugin.toml` file. mise does not call upstream vfox's `PreUse` hook.

## Tool options in hooks {#tool-options}

Tool and backend hooks read the options on a tool's entry in `mise.toml` from
`ctx.options`, where top-level booleans and numbers arrive as strings:
`bundled = false` arrives as `"false"`, which is truthy in Lua. See
[tool options](/tool-plugin-development.html#tool-options) for the hooks that
receive them, the options mise keeps to itself and the `MISE_TOOL_OPTS__`
environment variables, and [backend plugins](/backend-plugin-development.html#tool-options)
for the backend hooks. An environment plugin's `MiseEnv` and `MisePath` hooks
receive the options of its `[env]` directive with their TOML types kept; see
[environment plugins](/env-plugin-development.html).

## Module index

| Module                         | Functions                                                                     |
| ------------------------------ | ----------------------------------------------------------------------------- |
| [`http`](#http-module)         | `get`, `head`, `download_file`, `try_get`, `try_head`, `try_download_file`    |
| [`json`](#json-module)         | `encode`, `decode`                                                            |
| [`file`](#file-module)         | `read`, `exists`, `stat`, `list`, `glob`, `join_path`, `move`, `symlink`      |
| [`archiver`](#archiver-module) | `decompress`                                                                  |
| [`cmd`](#command-module)       | `exec`, `stream`                                                              |
| [`strings`](#strings-module)   | `split`, `join`, `trim`, `trim_space`, `has_prefix`, `has_suffix`, `contains` |
| [`semver`](#semver-module)     | `compare`, `parse`, `sort`, `sort_by`                                         |
| [`html`](#html-module)         | `parse`                                                                       |
| [`env`](#environment-module)   | `setenv`                                                                      |
| [`log`](#log-module)           | `trace`, `debug`, `info`, `warn`, `error`                                     |

## Errors

A Lua error stops the hook, and mise reports it as the failure of the operation
that ran the hook, such as the install. Raise one with `error("message")`.

`pcall` catches the errors that the `json`, `file`, `archiver`, `cmd`,
`strings` and `semver` functions raise. It does not work with the `http`
functions: a call to any of them inside `pcall`, directly or through a function
you pass to `pcall`, fails with
`attempt to yield across metamethod/C-call boundary`, even when the request
would succeed. When a request may fail and you have a fallback, call
`http.try_get`, `http.try_head` or `http.try_download_file`, which return the
error instead of raising it.

```lua
local json = require("json")

local ok, data = pcall(json.decode, body)
if not ok then
    error("unexpected response: " .. tostring(data))
end
```

## `http` {#http-module}

| Function                                     | Returns                                               | On failure                                                                      |
| -------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------- |
| `http.get({ url, headers })`                 | `{ status_code, headers, body }`                      | Raises on a network error. Any HTTP status is returned, so check `status_code`. |
| `http.head({ url, headers })`                | `{ status_code, headers }`, with no body              | Same as `get`                                                                   |
| `http.download_file({ url, headers }, path)` | nothing                                               | Raises on a network error or an HTTP error status                               |
| `http.try_get(...)`, `http.try_head(...)`    | `response, nil`, or `nil, message` on a network error | Does not raise                                                                  |
| `http.try_download_file(...)`                | `true, nil`, or `nil, message`                        | Does not raise                                                                  |

`headers` is optional. Response header names are in lower case, such as
`resp.headers["content-length"]`. `download_file` creates the directories above
`path` if they are missing.

```lua
local http = require("http")
local json = require("json")

local resp = http.get({
    url = "https://api.github.com/repos/owner/repo/releases",
    headers = { ["Accept"] = "application/json" },
})
if resp.status_code ~= 200 then
    error("GET releases returned " .. resp.status_code)
end
local releases = json.decode(resp.body)

-- Raises on failure; use http.try_download_file to handle errors without stopping the hook.
http.download_file({
    url = "https://github.com/owner/repo/archive/v1.0.0.tar.gz",
}, "/path/to/download.tar.gz")

-- Fall back to a mirror when the primary host cannot be reached.
local index, err = http.try_get({ url = "https://primary.example.com/index.json" })
if err ~= nil then
    index, err = http.try_get({ url = "https://mirror.example.com/index.json" })
end
```

Requests retry transient failures (connection errors, timeouts, and HTTP 408,
429 and 5xx responses) up to `MISE_HTTP_RETRIES` times, 3 by default. The
plugin HTTP client reads only that environment variable, not an
[`http_retries`](/configuration/settings.html#http_retries) value from a config
file. Requests follow mise's [`url_replacements`](/url-replacements.html), and
credentials from [`netrc`](/configuration/settings.html#netrc) are added for the
destination host. A request to `api.github.com`, or to a GitHub Enterprise Cloud
`api.*.ghe.com` host, that has no `Authorization` header gets the
[GitHub token](/dev-tools/github-tokens.html) mise resolves for `github.com`
(even for a `ghe.com` host) and an `X-GitHub-Api-Version` header. For a GitHub
Enterprise Cloud host that needs its own token, set `Authorization` yourself.
A plugin that calls `api.github.com` therefore needs no token handling, and no
plugin needs a retry loop of its own. Do not attach a GitHub token to
`github.com` release-download URLs: GitHub then redirects to a host that
rejects the request.

`download_file` does not verify what it downloads. In a tool plugin, return
`sha256` or `sha512` from `PreInstall` instead of downloading, and mise
downloads, verifies and extracts the file. A backend plugin that downloads in
`BackendInstall` must check the digest itself. The Lua modules have no hash
function, so run the platform's checksum tool through `cmd.exec`, such as
`sha256sum` on Linux or `shasum -a 256` on macOS.

## `json` {#json-module}

| Function             | Returns       | On failure                                                                               |
| -------------------- | ------------- | ---------------------------------------------------------------------------------------- |
| `json.encode(value)` | a JSON string | Raises when the value holds a function, or userdata other than the `null` sentinel below |
| `json.decode(text)`  | a Lua value   | Raises on invalid JSON; `pcall` catches it                                               |

```lua
local json = require("json")

local text = json.encode({ name = "mise-plugin", tools = { "prettier", "eslint" } })
-- {"tools":["prettier","eslint"],"name":"mise-plugin"}; key order is not guaranteed

local data = json.decode(text)
print(data.tools[1]) -- prettier
```

JSON `null` decodes to a sentinel value, not `nil`, and the sentinel is truthy.
Compare against it explicitly; encoding the sentinel produces `null`:

```lua
local NULL = json.decode("null")
local release = json.decode(resp.body)
if release.body == nil or release.body == NULL then
    -- the field is missing or null
end
```

An empty Lua table encodes as `{}`, not `[]`.

## `file` {#file-module}

| Function                 | Returns                                                        | On failure                                   |
| ------------------------ | -------------------------------------------------------------- | -------------------------------------------- |
| `file.join_path(...)`    | the non-empty arguments joined with the host's separator       | Does not fail                                |
| `file.read(path)`        | the file's contents as text                                    | Raises when the file is missing or not UTF-8 |
| `file.exists(path)`      | `true` or `false`                                              | Raises when the path cannot be checked       |
| `file.stat(path)`        | a table described below, or `nil` when the path is missing     | Raises on other errors                       |
| `file.list(dir)`         | full paths of the directory's immediate entries, sorted        | Raises when the directory is missing         |
| `file.glob(pattern)`     | paths matching the pattern, sorted                             | Raises on an invalid pattern                 |
| `file.move(from, to)`    | nothing; moves a file or directory and creates parents of `to` | Raises                                       |
| `file.symlink(src, dst)` | nothing; creates the link `dst` pointing at `src`              | Raises                                       |

A relative path resolves against the directory mise runs in, not against the
plugin or install directory. Build paths from the absolute paths in `ctx`:

```lua
local file = require("file")

-- In BackendInstall. Tool plugin hooks call this path ctx.rootPath or ctx.path.
local install_path = ctx.install_path
local bin = file.join_path(install_path, "bin", "mytool")

local matches = file.glob(file.join_path(install_path, "bin", "mytool-*"))
if #matches == 1 then
    file.move(matches[1], bin)
end
if not file.exists(bin) then
    error("mytool was not installed to " .. bin)
end
```

`file.move` renames the path, so the source and destination must be on the
same file system.

`file.join_path` does not normalize separators, resolve `..`, expand `~`, or
make an untrusted path safe. Pass relative segments after the base directory.
In environment plugins, use `ctx.config_root` as the base for paths that come
from the project.

`file.stat` inspects the link itself, not its target. Its table has `size`,
`is_file`, `is_dir`, `is_symlink`, and the Unix timestamps `modified`,
`accessed` and `created` when the system provides them. `mode` is an octal
permission string such as `"644"` on Unix and `nil` elsewhere.

## `archiver` {#archiver-module}

`archiver.decompress(archive, destination[, options])` extracts an archive. It
picks the format from the file name: `.tar.gz`, `.tar.xz`, `.tar.bz2` or
`.zip`. Any other name, including `.tgz`, raises an error. It does not download
or verify the archive.

```lua
local archiver = require("archiver")

archiver.decompress("/path/to/tool.tar.gz", "/path/to/destination") -- raises on failure

-- Move the contents of the archive's top-level directory into the destination.
archiver.decompress("/path/to/node-v24.18.1-linux-x64.tar.gz", "/path/to/destination", {
    strip_components = 1,
})
```

`strip_components = 1` keeps files that sit at the archive's root, as mise's
own archive extraction does. Only `0` and `1` are accepted.

## `cmd` {#command-module}

`cmd.exec` runs a command and returns what it printed:

```lua
local cmd = require("cmd")
local log = require("log")

-- src_dir and install_path are directories your hook has worked out.

-- Returns stdout, including the trailing newline; raises with stderr on a non-zero exit.
local tag = cmd.exec("git describe --tags", { cwd = src_dir })

-- Catch the error when you have a fallback.
local ok, err = pcall(cmd.exec, "example --version")
if not ok then
    log.debug("example not available:", err)
end

-- Extra variables for this command only.
cmd.exec("make install", { cwd = src_dir, env = { PREFIX = install_path } })
```

The command is shell code, not an argument list. mise runs it with the shell
set by [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args),
in the [hook environment](#hook-environment). Quote any value you interpolate
for that shell: a tool option pasted into the command can run commands you did
not intend.

### Options {#available-options}

`cmd.exec` and `cmd.stream` accept an options table. `os.execute` takes none.

| Option    | Type   | Effect                                                                                                  |
| --------- | ------ | ------------------------------------------------------------------------------------------------------- |
| `cwd`     | string | Working directory for the command                                                                       |
| `env`     | table  | Variables merged over the [hook environment](#hook-environment)                                         |
| `timeout` | number | Seconds the command may run, greater than 0 (fractions allowed); then mise kills it and the call raises |

### Output, stdin and the terminal

mise installs tools in parallel, so the three ways to run a command differ in
how they share the terminal:

| Function                       | Output                                   | stdin                           | Returns                                                   | Terminal lock |
| ------------------------------ | ---------------------------------------- | ------------------------------- | --------------------------------------------------------- | ------------- |
| `cmd.exec(command, options)`   | captured; stderr is discarded on success | `/dev/null` unless `raw` is set | stdout as a string; raises with stderr on a non-zero exit | none          |
| `os.execute(command)`          | written to the terminal                  | `/dev/null` unless `raw` is set | the exit code (`0` on success)                            | shared        |
| `cmd.stream(command, options)` | written to the terminal                  | connected to the terminal       | the exit code                                             | exclusive     |

Only the exclusive side of the lock makes other work wait. While a
`cmd.stream` child (or a command run with `--raw`) runs, every command mise
starts for other installs (a core tool's build, an asdf plugin's script,
`os.execute` in another plugin) waits until it exits, and `cmd.stream` also
pauses the progress display. `os.execute` takes the shared side, like mise's
own commands, so it does not hold up other installs, but a pending
`cmd.stream` waits for it to finish. `cmd.exec` takes no lock.

Use `cmd.exec` unless the user must watch the output as it happens
(`os.execute`) or answer a prompt (`cmd.stream`). Visibility is not a reason to
stream: report progress with `print()` or `log.info()`, which mise writes above
the progress display (see [`print`](#print-override)).

### Hooks and stdin

Write hooks that do not prompt. Take what you need from tool options, the
environment or the lockfile, and pass child processes their non-interactive
flag (`--yes`, `--non-interactive`, `-n`) where they have one. Unless the user
sets `raw`, a child that reads stdin under `cmd.exec` or `os.execute` sees end
of file at once rather than hanging or taking input meant for another install.

When a hook must interact with the user, for example to enter a credential or
accept a license, use `cmd.stream`:

```lua
local cmd = require("cmd")

local code = cmd.stream("some-tool login")
if code ~= 0 then
    error("login failed with status " .. tostring(code))
end
```

While the child runs, the other installs continue but cannot start commands of
their own, so a long `cmd.stream` call stalls them. Prefer a non-interactive
path when the tool offers one.

The [`raw`](/configuration/settings.html#raw) setting (`mise install --raw`,
`MISE_RAW=1`) connects stdin for every child and runs installs one at a time.
It is a user's escape hatch. A hook that works only with `raw` set is broken for
everyone else; use `cmd.stream` instead.

### Timeouts

A command runs until it exits unless you pass `timeout`. Set one when a
command could hang, such as a request to a network service that may not answer:

```lua
local cmd = require("cmd")

local ok, err = pcall(cmd.exec, "some-tool sync", { timeout = 30 })
if not ok then
    error("sync did not finish: " .. tostring(err))
end
```

When the time runs out, mise kills the command, and the call raises an error
that `pcall` can catch; `cmd.exec` discards the output collected so far. Only
the shell mise started is killed, so background processes it spawned keep
running. Prefer the tool's own timeout flag when it has one.

### Commands in environment hooks {#environment-inheritance-in-env-module-hooks}

In an environment plugin's `MiseEnv` and `MisePath` hooks, commands run with
the environment mise has built so far, including tools when the directive sets
`tools = true`. See [environment plugins](/env-plugin-development.html).

## `strings` {#strings-module}

| Function                                                                     | Returns                                                          |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| `strings.split(s, sep)`                                                      | a list of the parts of `s` between each `sep`                    |
| `strings.join(list, sep)`                                                    | the values in `list`, converted to strings and joined with `sep` |
| `strings.trim(s, suffix)`                                                    | `s` with every trailing copy of `suffix` removed                 |
| `strings.trim_space(s)`                                                      | `s` without leading and trailing whitespace                      |
| `strings.has_prefix(s, prefix)`, `has_suffix(s, suffix)`, `contains(s, sub)` | `true` or `false`                                                |

`strings.trim` treats `suffix` as a literal string, not a set of characters,
and leaves the start of the string alone.

```lua
local strings = require("strings")

strings.split("a,b,c", ",")            --> { "a", "b", "c" }
strings.join({ "a", "b", "c" }, " - ") --> "a - b - c"
strings.trim("hello worldworld", "world") --> "hello "
strings.trim_space("  1.2.3\n")         --> "1.2.3"
```

## `semver` {#semver-module}

`semver` compares the numbers in a version string and ignores everything else,
so `1.0.0-beta` equals `1.0.0` and `v2` equals `2.0.0`. A missing part counts
as `0`, so `1.0.0-beta.1` is greater than `1.0.0`. This is not SemVer
precedence. Use it only for tools whose versions are plain dotted numbers, and
otherwise keep the order the publisher's release list gives you.

| Function                      | Returns                                                   |
| ----------------------------- | --------------------------------------------------------- |
| `semver.compare(a, b)`        | `-1`, `0` or `1`                                          |
| `semver.parse(v)`             | the numbers in `v`: `"v1.2.3-beta"` gives `{ 1, 2, 3 }`   |
| `semver.sort(list)`           | a new list of version strings, in ascending order         |
| `semver.sort_by(list, field)` | a new list of tables, ascending by the version in `field` |

```lua
local semver = require("semver")

semver.compare("1.2.3", "1.2.4")  --> -1
semver.compare("2.0.0", "1.9.9")  --> 1
semver.compare("9.6.9", "9.6.24") --> -1 (numeric, not string, comparison)
semver.sort({ "1.10.0", "1.2.0" }) --> { "1.2.0", "1.10.0" }
```

`Available` must return versions newest first, but `sort` and `sort_by` sort
ascending. Sort with a comparator instead:

```lua
table.sort(versions, function(a, b)
    return semver.compare(a.version, b.version) > 0
end)
```

## `html` {#html-module}

`html.parse(text)` returns a document. `find` accepts CSS selectors, including
descendant and attribute selectors such as `ul.downloads a[href$='.tar.gz']`.

| Method                                     | Returns                                                                                               |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| `doc:find(selector)`, `sel:find(selector)` | a selection of the matching elements                                                                  |
| `sel:each(function(index, el) ... end)`    | nothing; calls the function for each element, with a zero-based index                                 |
| `sel:first()`, `sel:eq(n)`                 | a selection of the first element, or of the element at zero-based position `n`                        |
| `sel:text()`                               | the inner content of the first element, which can include markup; `""` when empty                     |
| `sel:attr(name)`                           | an attribute of the first element; `nil` when that element lacks it, `""` when the selection is empty |

`sel:find()` searches below the first element of a selection only, so
`doc:find("ul"):find("a")` misses links in a second `ul`. To search every
element, write one descendant selector such as `doc:find("ul a")`, or call
`:find()` on each element inside `:each()`. Calling `:find()` on an empty
selection raises an error.

### Collect links from a download page

```lua
local http = require("http")
local html = require("html")

-- Lists versions linked from a page such as https://example.com/downloads/
-- whose links look like example-1.2.3.tar.gz.
local function list_versions(index_url)
    local resp = http.get({ url = index_url })
    if resp.status_code ~= 200 then
        error("GET " .. index_url .. " returned " .. resp.status_code)
    end
    local seen, versions = {}, {}
    html.parse(resp.body):find("a[href$='.tar.gz']"):each(function(_, a)
        local v = a:attr("href"):match("example%-(.+)%.tar%.gz$")
        if v and not seen[v] then
            seen[v] = true
            table.insert(versions, v)
        end
    end)
    return versions
end
```

Prefer a release API or a JSON index when the publisher has one: HTML layouts
change, and paginated release pages list only recent versions.

## `env` {#environment-module}

`env.setenv(key, value)` sets a variable in the mise process. It never reaches
the user's shell. In hooks that have a [hook environment](#hook-environment),
`os.getenv`, `os.execute` and the `cmd` functions use that environment and do
not see it. To give the user a variable, return it from `MiseEnv`, `EnvKeys` or
`BackendExecEnv`. To give one command a variable, pass it in the `env` option
of `cmd.exec`:

```lua
local cmd = require("cmd")

cmd.exec("make", { env = { CC = "clang" } })
```

To add directories to the user's `PATH`, return each one as a separate entry,
built with `file.join_path`, from `MisePath` or as a `PATH` key from `EnvKeys`
or `BackendExecEnv`. mise joins them with the host's separator, so do not
build a colon-separated string in a plugin that also runs on Windows.

## `log` {#log-module}

`log` writes to mise's log on stderr, prefixed with the plugin's name. `info`,
`warn` and `error` show by default, `debug` with `MISE_DEBUG=1`, and `trace`
with `MISE_TRACE=1`. [`MISE_LOG_LEVEL`](/configuration/environment-variables.html#mise-log-level)
changes the default, and `--quiet` hides everything below `error`.

```lua
local log = require("log")

log.debug("resolved download URL", url)
log.info("version", version, "installed to", path)
log.warn("no checksum published for", version)
```

The functions take any number of arguments, convert each with `tostring`, and
join them with tabs. The `log.info` call above prints:

```text
mise [my-plugin] version  1.0.0  installed to  /path/to/install
```

### `print` {#print-override}

`print()` is `log.info()`. Its output goes to stderr as an info line prefixed
with the plugin's name, printed above the progress display, and is hidden when
the log level is above `info`. Use it to report progress.

## Caching

mise caches version lists and the environment a tool's hooks return, so hooks
rarely need a cache of their own; see [Caches](/cache-behavior.html). A Lua
table lasts for one mise command only. Environment plugins can return cache
settings with their variables; see
[environment plugins](/env-plugin-development.html).

## Related pages

- [Tool plugins](/tool-plugin-development.html)
- [Backend plugins](/backend-plugin-development.html)
- [Environment plugins](/env-plugin-development.html)
- [Package manager plugins](/package-plugin-development.html)
- [Publishing plugins](/plugin-publishing.html)
