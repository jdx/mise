---
description: "Use, maintain and test shell-script asdf plugins with mise, and port them to Lua tool plugins."
---

# asdf plugins (legacy)

asdf plugins are Git repositories of shell scripts (`bin/list-all`,
`bin/install` and others) that follow the
[asdf plugin interface](https://asdf-vm.com/plugins/create.html). mise runs
them through its [asdf backend](/dev-tools/backends/asdf.html) on Linux and
macOS, so existing plugins keep working.

asdf plugins are a legacy format: the mise registry accepts no new tools backed
by them, and mise does not run them on Windows. To write a new plugin, write a
[tool plugin](/tool-plugin-development.html) in Lua. The sections below cover
using and maintaining an existing asdf plugin, and porting one to Lua.

## Install an asdf plugin {#installing-asdf-legacy-plugins}

Use a plugin by its repository, with no separate install step:

```sh
mise use asdf:owner/asdf-tool@1.2.3
```

Or install it under a name of your own and use that name:

```sh
mise plugins install my-tool https://github.com/owner/asdf-tool
mise use my-tool@1.2.3
```

`mise plugins add` is an alias of `mise plugins install`. To have
`mise install` fetch the plugin for everyone who works on a project, declare it
in `mise.toml`, pinned to a full commit SHA (mise rejects abbreviated ones):

```toml
[plugins]
my-tool = "https://github.com/owner/asdf-tool#c532b140abd4ca00d3e76651b9bd32a980bd483c"

[tools]
my-tool = "1.2.3"
```

`mise registry <tool>` lists the backends a registry shorthand can use, in
order of preference, and `mise tool <tool>` shows the one mise selected. Use
the full `asdf:owner/repo` identifier when you need a particular plugin. Find
existing plugins in [asdf-plugins](https://github.com/asdf-vm/asdf-plugins)
and the [mise-plugins](https://github.com/mise-plugins) organization, and see
[Plugins](/plugins.html) to update or remove them.

## Limitations

- mise does not run asdf plugins on Windows; it skips `asdf:` tools there.
- [`mise.lock`](/dev-tools/mise-lock.html) records only the version of an
  asdf tool. The scripts download the tool themselves, so mise has no URL or
  checksum to lock, and cannot verify what they fetch.
- Each script runs in a new process; see [bin/exec-env](/asdf-legacy-plugins.html#bin-exec-env) for
  caching.
- In [safe mode](/security.html#safe-mode), mise refuses to run asdf plugin
  scripts.

## asdf and Lua plugins compared {#feature-comparison-asdf-vs-vfox}

|                           | asdf plugins                              | Lua tool plugins                                                                                                                                                      |
| ------------------------- | ----------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Written in                | Executable scripts, usually Bash          | Lua hooks that mise runs in its embedded interpreter                                                                                                                  |
| Platforms                 | Linux and macOS                           | Linux, macOS and Windows, given a build of the tool for each                                                                                                          |
| External programs         | Usually `curl`, `git`, `tar` and similar  | Built-in [HTTP, JSON, HTML and archive modules](/plugin-lua-modules.html#module-index)                                                                                |
| Download and verification | The scripts download and install the tool | `PreInstall` returns a URL and checksum; mise downloads, verifies and extracts                                                                                        |
| `mise.lock`               | The version only                          | The version, each platform's download URL, and the attestation method when `PreInstall` returns one. mise checks the checksum at install time but does not record it. |

Neither format is a sandbox: both run commands with your permissions.

## Scripts {#plugin-structure}

mise runs these scripts from the plugin's `bin/` directory. Mark them
executable.

| Script                                                                   | Required | mise uses it to                                                                                              |
| ------------------------------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------------ |
| `bin/list-all`                                                           | yes      | list versions, oldest first                                                                                  |
| `bin/install`                                                            | yes      | install `$ASDF_INSTALL_VERSION` into `$ASDF_INSTALL_PATH`                                                    |
| `bin/download`                                                           | no       | fetch into `$ASDF_DOWNLOAD_PATH` before `install` runs                                                       |
| `bin/latest-stable`                                                      | no       | resolve `latest`                                                                                             |
| `bin/list-bin-paths`                                                     | no       | name the directories under the install to add to `PATH` (default `bin`)                                      |
| `bin/exec-env`                                                           | no       | export variables while the tool is active (`PATH` changes are ignored; use `list-bin-paths`)                 |
| `bin/list-aliases`                                                       | no       | provide [version aliases](/dev-tools/aliases.html#aliased-versions)                                          |
| `bin/list-legacy-filenames`, `bin/parse-legacy-file`                     | no       | support idiomatic version files                                                                              |
| `bin/uninstall`                                                          | no       | clean up outside the install directory; runs before mise deletes the install, download and cache directories |
| `bin/post-plugin-add`, `bin/post-plugin-update`, `bin/pre-plugin-remove` | no       | run on plugin lifecycle events                                                                               |

### bin/list-all

Print every version, separated by spaces or newlines, oldest first in the
publisher's order. mise strips a leading `v` before a digit, so `v1.2.3` becomes
`1.2.3`.

```bash
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' 1.0.0 1.1.0 1.10.0
```

Do not re-sort the list with `sort -V`: it is not available on macOS, and it
misorders prereleases and channel names.

### bin/download and bin/install

`bin/download` is optional. Without it, `bin/install` fetches the tool itself.
This pair downloads a source archive, checks it against the checksum file
published next to it, and builds it with `make`:

```bash
#!/usr/bin/env bash
# bin/download
set -euo pipefail
file="tool-${ASDF_INSTALL_VERSION}.tar.gz"
url="https://github.com/owner/tool/releases/download/v${ASDF_INSTALL_VERSION}/$file"
mkdir -p "$ASDF_DOWNLOAD_PATH"
cd "$ASDF_DOWNLOAD_PATH"
curl -fsSL -o "$file" "$url"
curl -fsSL -o "$file.sha256" "$url.sha256"
# The checksum file holds "<digest>  <file>". macOS has shasum, not sha256sum.
if command -v sha256sum >/dev/null; then
  sha256sum -c "$file.sha256"
else
  shasum -a 256 -c "$file.sha256"
fi
```

```bash
#!/usr/bin/env bash
# bin/install
set -euo pipefail
cd "$ASDF_DOWNLOAD_PATH"
tar -xzf "tool-${ASDF_INSTALL_VERSION}.tar.gz" --strip-components=1
make install PREFIX="$ASDF_INSTALL_PATH"
```

mise does not verify what these scripts download, so the checksum check in
`bin/download` is what stops a corrupted or altered archive: a mismatch makes
the script exit non-zero, and mise fails the install. mise also fails it when
`bin/install` exits successfully but leaves `$ASDF_INSTALL_PATH` empty. After
a successful install, mise deletes the download directory unless
[`always_keep_download`](/configuration/settings.html#always_keep_download) is
set, so do not leave files there that the tool needs.

### bin/list-bin-paths

Print the directories to add to `PATH`, relative to `$ASDF_INSTALL_PATH` and
separated by spaces. `.` means the install directory itself. Without this
script, mise adds `bin`.

```bash
#!/usr/bin/env bash
echo "bin libexec/tool/bin"
```

### bin/exec-env

mise sources this script with Bash and keeps the variables it exports:

```bash
#!/usr/bin/env bash
export TOOL_HOME="$ASDF_INSTALL_PATH"
```

mise ignores `PATH` changes made here. It adds `bin`, or the directories that
`bin/list-bin-paths` prints, to `PATH` itself. To add an absolute directory,
export `MISE_ADD_PATH` instead.

mise caches the output of this script and of `bin/list-bin-paths` until the
plugin or the install directory changes. A cache miss runs them while mise
builds the shell environment, so keep them fast. When the output depends on
tool options or the project, set a [cache key](#mise-plugin-toml).

### bin/latest-stable

Print the single version that `latest` should resolve to. Without this script,
mise resolves `latest` from the `bin/list-all` output.

### bin/list-aliases

Print one alias and its version per line, separated by whitespace:

```text
lts 24.1.0
current 25.0.0
```

### bin/list-legacy-filenames and bin/parse-legacy-file

`bin/list-legacy-filenames` prints the names of the idiomatic version files the
plugin can read, such as `.example-version`. mise reads those files only for
tools listed in
[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools).
Do not list `.tool-versions`: mise parses that file itself.

`bin/parse-legacy-file` receives the file's path as `$1` and prints the
version. Without it, mise uses the file's contents.

```bash
#!/usr/bin/env bash
head -n 1 "$1"
```

### bin/uninstall

mise runs `bin/uninstall` before it deletes the version's install, download and
cache directories. Use it to remove files the tool created elsewhere.

### Plugin lifecycle scripts

`bin/post-plugin-add` runs after mise installs the plugin, from Git, a zip
archive or a local path under `[plugins]`; `mise plugins link` does not run it.
`bin/pre-plugin-remove` runs before mise removes the plugin, and
`bin/post-plugin-update` after an update that changed the plugin's Git ref.

## Environment variables

Every script receives:

| Variable                               | Value                                                                                                   |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `ASDF_PLUGIN_PATH`, `MISE_PLUGIN_PATH` | The plugin's directory                                                                                  |
| `MISE_PLUGIN_NAME`                     | The name the plugin was installed under                                                                 |
| `ASDF_CONCURRENCY`, `MISE_CONCURRENCY` | The number of CPUs, for `make -j`                                                                       |
| `GITHUB_TOKEN`, `GITHUB_API_TOKEN`     | The first of `MISE_GITHUB_TOKEN`, `GITHUB_API_TOKEN` and `GITHUB_TOKEN` that is set; empty when none is |
| `MISE_DATA_DIR`, `MISE_CACHE_DIR`      | mise's [directories](/directories.html)                                                                 |

`bin/list-all` and `bin/latest-stable` also receive the project's `[env]`
values and `_.path` entries, so a private plugin can use credentials or helper
programs from the project while it lists versions. mise keeps a separate
version cache for each resolved environment, without writing those values to
the cache.

The scripts that act on one version (`download`, `install`, `uninstall`,
`list-bin-paths` and `exec-env`) also receive:

| Variable                                                                     | Value                                                                                                                       |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `ASDF_INSTALL_TYPE`, `MISE_INSTALL_TYPE`                                     | `version`, `ref` for `ref:` requests, `sub` for `sub-` requests, or `path` for `path:` requests                             |
| `ASDF_INSTALL_VERSION`, `MISE_INSTALL_VERSION`                               | The resolved version, or the Git ref for a `ref:` request                                                                   |
| `ASDF_INSTALL_PATH`, `MISE_INSTALL_PATH`                                     | The install directory                                                                                                       |
| `ASDF_DOWNLOAD_PATH`, `MISE_DOWNLOAD_PATH`                                   | The download directory                                                                                                      |
| `MISE_TOOL_OPTS__<KEY>`                                                      | Each option on the tool's entry in `[tools]`, with the key in upper case: `mirror = "eu"` gives `MISE_TOOL_OPTS__MIRROR=eu` |
| `MISE_PROJECT_ROOT`                                                          | The project's root directory, when there is one                                                                             |
| The project's `[env]` values                                                 | As set in `mise.toml`                                                                                                       |
| The tool's [`install_env`](/dev-tools/backends/asdf.html#install-env) values | As set on the tool's entry                                                                                                  |

`bin/post-plugin-update` also receives `ASDF_PLUGIN_PREV_REF` and
`ASDF_PLUGIN_POST_REF` (and `MISE_PLUGIN_PREV_REF` and `MISE_PLUGIN_POST_REF`),
the Git refs before and after the update.

## How mise runs your scripts

- Scripts get `/dev/null` on stdin unless the user sets
  [`raw`](/configuration/settings.html#raw), so a prompt cannot wait for input.
- mise reads the output of `list-all`, `latest-stable`, `list-aliases`,
  `list-legacy-filenames` and `parse-legacy-file`, and hides their stderr
  unless the user passes `--verbose`. Write diagnostics to stderr so they do
  not end up in the version list. `list-bin-paths` is read the same way, but
  its stderr reaches the terminal.
- An `asdf` command on `PATH` runs mise in its place: `asdf install`,
  `asdf list` and `asdf reshim` call the matching mise commands, and other
  subcommands pass through to mise.
- The tools listed in the tool's [`depends`](/dev-tools/backends/asdf.html#install-dependencies)
  option come first on `PATH` for `bin/download` and `bin/install`.

### mise.plugin.toml {#mise-plugin-toml}

An optional `mise.plugin.toml` at the plugin's root changes how mise caches or
replaces some scripts:

```toml
[exec-env]
cache-key = ["{{ opts.mirror | default(value='') }}"]

[list-legacy-filenames]
data = ".example-version"
```

`[exec-env]` and `[list-bin-paths]` accept `cache-key`, a list of
[templates](/templates.html) whose rendered values become part of the cache
key, so output that depends on them is cached separately. The templates can
use `opts` (the tool's options) and `project_root`. Give every lookup a
default, as above: a template that fails to render crashes mise.
`[list-aliases]` and `[list-legacy-filenames]` accept `data`, a fixed string
that mise uses instead of running the script.

## A minimal plugin {#example-plugin}

This plugin needs no network or compiler. Create two executable files in
`my-plugin/bin/`:

```bash
#!/usr/bin/env bash
# bin/list-all
set -euo pipefail
printf '%s\n' 1.0.0
```

```bash
#!/usr/bin/env bash
# bin/install
set -euo pipefail
mkdir -p "$ASDF_INSTALL_PATH/bin"
cat >"$ASDF_INSTALL_PATH/bin/my-plugin" <<'SCRIPT'
#!/usr/bin/env sh
printf '%s\n' 'my-plugin 1.0.0'
SCRIPT
chmod +x "$ASDF_INSTALL_PATH/bin/my-plugin"
```

Replace the installer with your real download, verification and build steps.
The [asdf plugin template](https://github.com/asdf-vm/asdf-plugin-template)
shows common patterns for error handling and platform detection.

## Test it {#testing-plugins}

From a separate project directory, link the plugin and exercise it:

```sh
chmod +x /path/to/my-plugin/bin/*
mise plugins link my-plugin /path/to/my-plugin
mise ls-remote my-plugin
mise use my-plugin@1.0.0
mise exec -- my-plugin --version
# my-plugin 1.0.0
```

Run `MISE_DEBUG=1 mise install --force my-plugin@1.0.0` to run the install
scripts again and see each one mise runs, and `mise cache clear my-plugin`
after you change `bin/list-all`, so mise lists versions again. To keep the
test away from your own plugins and config, use the
[isolated test setup](/plugin-publishing.html#testing-before-publication).

## Security {#security-considerations}

An asdf plugin's scripts run with your permissions whenever mise lists
versions, installs the tool or builds its environment. Read the scripts before
you install a plugin, and pin it to a full commit SHA (`#<sha>`) so an update
cannot change them without your noticing.

## Migrate to a Lua plugin {#migration-path}

First check whether the tool needs a plugin at all: many tools install directly
with a [backend](/dev-tools/backends/#which-backend-to-use), such as
`mise use github:owner/repo`. If it does need one, start from the
[tool plugin template](https://github.com/jdx/mise-tool-plugin-template) and
port each script to the hook that replaces it.

### Hook migration {#hook-migration-asdf-to-vfox}

| asdf script                                                              | Lua hook                 | Notes                                                                                               |
| ------------------------------------------------------------------------ | ------------------------ | --------------------------------------------------------------------------------------------------- |
| `bin/list-all`                                                           | `Available`              | Return `{ version = "..." }` tables, newest first; `list-all` prints oldest first                   |
| `bin/latest-stable`                                                      | none                     | mise resolves `latest` from the `Available` list                                                    |
| `bin/download`                                                           | `PreInstall`             | Return the URL and a `sha256` or `sha512`; mise downloads, verifies and extracts the file           |
| `bin/install`                                                            | `PostInstall`            | Optional; runs after mise has extracted the download, for steps such as building                    |
| `bin/list-bin-paths`, `bin/exec-env`                                     | `EnvKeys`                | Return `{ key = "...", value = "..." }` tables, including `PATH` entries, instead of `export` lines |
| `bin/list-legacy-filenames`                                              | `PLUGIN.legacyFilenames` | A list in [`metadata.lua`](/plugin-lua-modules.html#metadata) instead of a script                   |
| `bin/parse-legacy-file`                                                  | `ParseLegacyFile`        | Return `{ version = "..." }`                                                                        |
| `bin/uninstall`                                                          | `PreUninstall`           | Runs before mise removes the version                                                                |
| `bin/list-aliases`                                                       | none                     | Users define [version aliases](/dev-tools/aliases.html#aliased-versions) in their config            |
| `bin/post-plugin-add`, `bin/post-plugin-update`, `bin/pre-plugin-remove` | none                     |                                                                                                     |

Tool options reach Lua hooks as `ctx.options` as well as `MISE_TOOL_OPTS__`
variables; see [tool options](/tool-plugin-development.html#tool-options).
