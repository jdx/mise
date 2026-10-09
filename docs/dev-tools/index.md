---
description: "Install tools such as Node.js and Python, pick their versions per project, and upgrade or remove them."
---

# Dev tools

mise installs tools such as Node.js, Python, and Go, and selects their versions
for each project. Declare the versions a project uses in `mise.toml`, and mise
puts them on `PATH` while you work in that directory.

## Add a tool to a project {#add-a-tool-to-a-project}

From the project directory:

```sh
mise use node@24 python@3.14
```

This installs both tools and records the version requests in the project's
config:

```toml [mise.toml]
[tools]
node = "24"
python = "3.14"
```

Run a command with those tools:

```sh
mise exec -- node --version
# v24.x.x
```

With [shell activation](/shell-setup.html), you can run `node --version`
directly, and mise switches versions as you move between projects. After you
clone a repository or edit its `mise.toml`, run `mise install` to install the
tools it declares.

## Everyday commands {#choose-the-right-command}

| Goal                                   | Command                               |
| -------------------------------------- | ------------------------------------- |
| Add or change a project's tool version | `mise use node@24`                    |
| Set a personal default                 | `mise use --global node@24`           |
| Install the tools a project declares   | `mise install`                        |
| Also install the tools its tasks need  | `mise install --include-task-tools`   |
| Run a version once without saving it   | `mise exec node@24 -- node --version` |
| Show the tools this directory uses     | `mise ls --current`                   |
| List the versions you can install      | `mise ls-remote node`                 |
| Show which executable a command runs   | `mise which node`                     |
| Show tools with newer versions         | `mise outdated`                       |
| Upgrade within the configured request  | `mise upgrade node`                   |
| Upgrade and rewrite the request        | `mise upgrade --bump node`            |
| Remove a tool from the project         | `mise unuse node`                     |
| Delete an installed version            | `mise uninstall node@22`              |

[`mise use`](/cli/use.html) installs a version and writes the request to the
project's `mise.toml`. Add `--pin` to write the resolved version instead,
`--global` to write a personal default to `~/.config/mise/config.toml`, or
`--path` to choose the file; see
[which file mise writes to](/configuration.html#target-file-for-write-operations).
`mise use` cannot change the shell it runs in: activation applies the change
at the next prompt, and `mise exec` and tasks read it each time.

[`mise install`](/cli/install.html) installs tools without changing any config:

- `mise install node@24.11.1` installs that version.
- `mise install node@24` installs the newest 24.x release.
- `mise install node` installs the version the config selects.
- `mise install` installs every configured tool except tools marked
  [`lazy = true`](/dev-tools/shims.html#lazy-tools); add `--include-lazy` to
  install those too.
- `mise install --include-task-tools` also installs the tools that tasks in the
  current scope need, without running them, which warms CI, container, or
  offline caches. Add `--monorepo` to include every configured monorepo root.

[`mise exec`](/cli/exec.html) reads the same config files, so without
activation or shims you can prefix any command with `mise exec --`. An alias
such as `alias mx='mise exec --'` saves typing. [`mise run`](/tasks/) loads the
same tools and environment for tasks.

## How mise selects a tool {#how-tools-are-selected}

1. mise reads `mise.toml` and other config files from the current directory,
   its parents, and your global config. A file closer to the current directory
   overrides one further up; see [config file locations](/configuration.html#mise-toml).
2. The [registry](/registry.html) maps a short name such as `node` to a
   [backend](/dev-tools/backends/), which lists versions and installs the tool.
3. The backend resolves the [version request](/dev-tools/versions.html), such
   as `24` or `latest`. mise reuses an installed version that matches before it
   looks for a newer one, and a [lockfile](/dev-tools/mise-lock.html) can fix
   the result.
4. mise puts the selected tools on `PATH` for the command, task, or shell.
   `mise exec` and `mise run` install missing tools first.

mise also reads `.tool-versions` files. Version files from other tools, such as
`.nvmrc` and `.python-version`, need
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files)
turned on. If you are coming from asdf, see
[Migrating from asdf](/dev-tools/comparison-to-asdf.html). Run
`mise config ls` to see which config files apply in a directory.

## Shells, editors, and scripts {#shells-editors-and-scripts}

- In an interactive shell, [activate mise](/shell-setup.html) so `PATH` and
  project environment variables update at each prompt.
- In editors and IDEs, use [shims](/dev-tools/shims.html) or an
  [editor integration](/ide-integration.html) where a program needs a stable
  executable path.
- In scripts and CI, run `mise exec -- <command>` or `mise run <task>`; see
  [Continuous integration](/continuous-integration.html).

[Shims](/dev-tools/shims.html#overview) compares the three approaches.

## Upgrade tools {#upgrade-tools}

[`mise outdated`](/cli/outdated.html) lists configured tools with a newer
release that matches their request. Add `--bump` to compare against the newest
release overall.

[`mise upgrade`](/cli/upgrade.html) installs the newest release within each
request. With `node = "24"` in `mise.toml`, `mise upgrade node` installs the
newest 24.x and leaves `mise.toml` alone. When the project uses a
[lockfile](/dev-tools/mise-lock.html), mise updates `mise.lock` to the new
version.

`--bump` upgrades to the newest release and rewrites the request with the same
precision: `node = "24"` becomes `node = "26"`, and `node = "24.11.1"` becomes
the newest exact version. When you name a request, as in
`mise upgrade --bump node@latest`, mise writes that request instead.

```sh
mise upgrade --dry-run      # show what would change
mise upgrade --interactive  # pick tools from a list
mise upgrade --bump --local # bump only tools in project config
```

After an upgrade, mise schedules the version it replaced for removal once
[`upgrade.prune_after`](/configuration/settings.html#upgrade.prune_after) has
passed, so running programs can keep using it. Pass `--prune` to remove it now,
or `--no-prune` to keep it; set
[`upgrade.auto_prune`](/configuration/settings.html#upgrade.auto_prune) to
`false` to keep replaced versions by default.

## Automatic tool updates {#automatic-tool-updates}

A tool in your global config (`~/.config/mise/config.toml`) can keep itself up
to date. Set `auto_update` on its entry:

```toml [~/.config/mise/config.toml]
[tools]
claude = { version = "latest", auto_update = true }
node = { version = "24", auto_update = "6h" }
```

When a shim or `mise exec` is about to run the tool and mise has not checked
for an update within the interval, it runs `mise upgrade` for that tool first,
showing the usual install progress, then runs the new version. Updates stay
within the request: `node = "24"` gets the newest 24.x, never 26. If the update
fails or you are offline, mise warns and runs the version you have.

`auto_update = true` checks every
[`tool_update.check_duration`](/configuration/settings.html#tool_update.check_duration).
A duration such as `"6h"` sets that tool's own interval. Intervals under one
hour are raised to one hour.

::: tip A 24h delay can mean up to 48h
Updates respect [`minimum_release_age`](/security.html#minimum-release-age),
which defaults to `24h` for
[most backends](/security.html#which-backends-have-a-default). A release becomes
eligible 24 hours after publishing, and mise only notices it at the next check,
so an update lands 24 to 48 hours after a release if you launch the tool at
least once per check interval, and later if you don't.
:::

To wait longer, set `minimum_release_age` on the tool:

```toml [~/.config/mise/config.toml]
[tools]
node = { version = "24", auto_update = "6h", minimum_release_age = "3d" }
```

- Only global config can turn this on. A project config cannot, and when a
  project sets its own version of the tool, runs in that project do not update
  it.
- Only the tool being run is checked: `mise exec -- npm test` does not update
  `claude`. Tasks, `mise hook-env`, and shell activation never update tools.
- Exact versions such as `node = "24.11.1"` are never updated. If a global
  lockfile (`mise lock --global`) pins the tool, the update moves the lock
  entry to the new version. A project's config and lockfile are never changed.
- No updates run offline, in CI, or with `locked = true`.
- The previous version is pruned on the same schedule as after `mise upgrade`.
- If the last update of a tool failed, `mise doctor` shows the error.

To update in the background instead, so launches never wait and tools run
directly from `PATH` with shell activation stay current too, declare the
`tool-update` service in your global config and run
`mise bootstrap services apply`:

```toml [~/.config/mise/config.toml]
[bootstrap.services.mise-tool-update]
builtin = "tool-update"
```

The service checks once an hour and updates each tool when its interval is
due. While it runs, launches do not update tools themselves. See
[user services](/bootstrap/services.html#user-services).

## Remove tools {#remove-tools}

| Command                  | Edits config        | Deletes installed versions                                    |
| ------------------------ | ------------------- | ------------------------------------------------------------- |
| `mise unuse node`        | Removes the request | Versions that no tracked config or tool stub still needs      |
| `mise uninstall node@22` | No                  | That version; `--all` deletes every version of the tool       |
| `mise prune`             | No                  | Every version that no tracked config or tool stub still needs |

[`mise unuse`](/cli/unuse.html) edits the first loaded config that declares the
tool, or the file you pick with `--path`, `--global`, or `--env`. A version
argument matches the request as written, so `node = "24"` is removed with
`mise unuse node@24`, not with the resolved version. Add `--no-prune` to keep
the installations.

[`mise uninstall`](/cli/uninstall.html) deletes installations and leaves config
alone, so a tool that is still configured installs again on the next
`mise install`.

[`mise prune`](/cli/prune.html) deletes versions that no config file mise has
used, and no tool stub that has run, still needs. mise records those files in
`~/.local/state/mise/tracked-configs` and `tracked-stubs`. It keeps versions
that a running process started from. Run `mise ls --prunable` or
`mise prune --dry-run` to see what it would delete.

## Automatic installation {#auto-install-mechanisms}

When a configured tool is missing, mise can install it instead of failing.
Setting [`auto_install`](/configuration/settings.html#auto_install) to `false`
turns off the first four cases below, and
[`auto_install_disable_tools`](/configuration/settings.html#auto_install_disable_tools)
lists tools they skip. A lazy tool still installs on first use.

| When you run                                                              | mise installs                                                  | Controlled by                                                                                                     |
| ------------------------------------------------------------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `mise exec`                                                               | Missing tools the config selects                               | [`exec_auto_install`](/configuration/settings.html#exec_auto_install)                                             |
| `mise run`                                                                | Missing tools the task needs                                   | [`task.run_auto_install`](/configuration/settings.html#task.run_auto_install)                                     |
| An unknown command in an activated shell, or a shim for a missing version | The configured tool that provides the command                  | [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install)                                   |
| An unknown command that no config mentions                                | The one registry tool that provides it, added to global config | [`not_found_auto_install_registry`](/configuration/settings.html#not_found_auto_install_registry), off by default |
| A command of a [lazy tool](/dev-tools/shims.html#lazy-tools)              | That tool, on first use                                        | `lazy = true` on the tool                                                                                         |

With auto-install off, `mise exec` warns that the tool is missing and runs
whatever copy of the command is on `PATH`. The command-not-found handler finds
tools through the registry's command names, so it cannot install a tool
declared with a raw backend such as `cargo:some-crate` until a version of it
is installed; see
[troubleshooting](/troubleshooting.html#auto-install-on-command-not-found-does-not-trigger).

## Tool options {#tool-options}

Write a tool as a table instead of a version string to set options. The
options below work with any tool entry, except where a row names the backends
that support them. Backend-specific options, such as `matching` for GitHub,
are listed on each [backend page](/dev-tools/backends/).

| Option                | What it does                                                                                                                                                                                                                                                                                      |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`             | The [version request](/dev-tools/versions.html). Use `prefix`, `ref`, or `path` instead for [those kinds of request](/dev-tools/versions.html#scopes).                                                                                                                                            |
| `os`                  | Install and use the tool only on these platforms; see [OS-specific tools](#os-specific-tools).                                                                                                                                                                                                    |
| `depends`             | Install these tools first; see [Tool dependencies](#tool-dependencies).                                                                                                                                                                                                                           |
| `install_env`         | Environment variables for the commands that install the tool (build scripts, package managers, plugin hooks) and for its `postinstall`. mise also reads GitHub, GitLab, and Forgejo token variables from it when it resolves and downloads the tool; see [tokens](/dev-tools/github-tokens.html). |
| `postinstall`         | A command to run after the tool installs; see [Tool postinstall commands](#tool-postinstall-commands).                                                                                                                                                                                            |
| `lazy`, `lazy_bins`   | Install the tool the first time one of its commands runs; see [Lazy tools](/dev-tools/shims.html#lazy-tools).                                                                                                                                                                                     |
| `auto_update`         | In global config only, update the tool before it runs; see [Automatic tool updates](#automatic-tool-updates).                                                                                                                                                                                     |
| `minimum_release_age` | Select only versions released at least this long ago, where the backend reports release dates; see [minimum release age](/security.html#minimum-release-age).                                                                                                                                     |
| `prerelease`          | Include prereleases in `mise ls-remote` and when resolving `latest` or a prefix. Every backend honors it except the core `java` plugin; see [`prereleases`](/configuration/settings.html#prereleases).                                                                                            |
| `version_order`       | Order versions by `semver` or by `source` on aqua, github, gitlab, forgejo, and http tools; see [Version ordering](/dev-tools/versions.html#version-ordering).                                                                                                                                    |

mise's HTTP client does not read proxy variables from `install_env`, so set
`HTTPS_PROXY` and similar variables in the environment that starts mise.

### Inline or table syntax {#table-format}

Short options fit on one line; use a table when a tool has several. These are
equivalent:

::: code-group

```toml [Inline]
[tools]
ripgrep = { version = "15", os = ["linux", "macos"] }
```

```toml [Table]
[tools.ripgrep]
version = "15"
os = ["linux", "macos"]
```

:::

mise reads TOML 1.1, so an inline table can span several lines and end with a
trailing comma. Nested options, such as the HTTP backend's per-platform
`platforms`, can use dotted keys or one line per platform:

```toml [mise.toml]
[tools."http:my-tool"]
version = "1.0.0"
platforms.macos-arm64.url = "https://example.com/my-tool-macos-arm64.tar.gz"
platforms.linux-x64.url = "https://example.com/my-tool-linux-x64.tar.gz"
```

See the [HTTP backend](/dev-tools/backends/http.html) for checksums and
executable selection.

### Options on the command line {#on-the-command-line}

Append options in brackets to the tool name, separated by commas, before the
version. Quote the argument so the shell does not expand the brackets:

```sh
mise use 'github:jqlang/jq[version_prefix=jq-,rename_exe=jq]@1.8.2'
```

The same form works with `mise install` and `mise exec`. `mise use` writes the
options to `mise.toml`:

```toml [mise.toml]
[tools]
"github:jqlang/jq" = { version = "1.8.2", version_prefix = "jq-", rename_exe = "jq" }
```

`mise use --tool-option KEY=VALUE` sets an option for the tool that follows
it. `mise use --tool-option mr_boxington=true rust mr-boxington` sets the
option on `rust` and adds the `mr-boxington` tool, which the option needs; see
[Cache Cargo builds with Mr Boxington](/lang/rust.html#share-cargo-builds-with-mr-boxington).

### Variables in tool options {#templates-in-tool-configuration}

Versions and option values can use [templates](/templates.html) with
environment variables and [`vars`](/configuration/vars.html), including values
that `[env]` loads with `_.source`, `_.file`, or environment modules. For
example, point a Go install at your company's module proxy:

```toml [mise.toml]
[vars]
goproxy = "https://goproxy.example.com"

[tools]
go = "1.27"
"go:github.com/mikefarah/yq/v4" = { version = "latest", install_env = { GOPROXY = "{{ vars.goproxy }}" } }
```

### OS-specific tools {#os-specific-tools}

Set `os` to install and use a tool only on some platforms. Everywhere else,
mise skips the tool:

```toml [mise.toml]
[tools]
# Linux and macOS only
ripgrep = { version = "latest", os = ["linux", "macos"] }

# Windows only
"github:PowerShell/PowerShell" = { version = "latest", os = ["windows"] }

# Linux, and macOS on Apple silicon
hk = { version = "latest", os = ["linux", "macos/arm64"] }
```

Values are `linux`, `macos` (or `darwin`), `windows` (or `win`), and `unix`
(every platform except Windows). Add an architecture to narrow a value, as in
`macos/arm64` or `linux/x64`; architectures are `arm64` (or `aarch64`) and
`x64` (or `x86_64`, `amd64`). A value without an architecture matches every
architecture on that platform.

### Tool dependencies {#tool-dependencies}

`depends` makes one tool wait for others to finish installing. Use it when a
tool's install or `postinstall` runs another tool that its backend does not
already require:

```toml [mise.toml]
[tools]
node = "24"
# postinstall runs npm, so node must finish installing first
# (github:example/acme is a placeholder)
"github:example/acme" = { version = "1.2.3", depends = ["node"], postinstall = "npm install --prefix ~/.acme acme-plugins" }
```

`depends` takes one tool name or a list. It only orders installation; it does
not add or install the listed tools. Each one must be in `[tools]`, where it
installs first, or already be on `PATH`. While the dependent tool installs and
runs its `postinstall`, the listed tools are on `PATH`. Plugin authors declare
a plugin's own requirements with `PLUGIN.depends`; see
[Tool plugin development](/tool-plugin-development.html#depends).

### Tool postinstall commands {#tool-postinstall-commands}

`postinstall` runs a command after this tool installs. It is separate from
the [`[hooks].postinstall`](/hooks.html) hook, which runs once after a whole
`mise install`:

```toml [mise.toml]
[tools]
node = { version = "24", postinstall = "corepack enable" }
```

By default the command runs only when mise installs or repairs the tool, never
after a failed install, and never with `--dry-run`. To run it on every
`mise install` and `mise use`, even when the version is already installed, set
`when = "always"`:

```toml [mise.toml]
[tools]
node = { version = "24", postinstall = { run = "corepack enable", when = "always" } }
```

The command runs with the tool's `bin` directory and any
[dependencies](#tool-dependencies) on `PATH`, the tool's `install_env`, and the
project's `[env]` values. Templates such as
<code v-pre>{{ tools.ripgrep.path }}</code> are rendered first. It also
receives:

- `MISE_TOOL_NAME`: the tool's short name, such as `node`.
- `MISE_TOOL_VERSION`: the version that was installed, such as `24.11.1`.
- `MISE_TOOL_INSTALL_PATH`: the directory the tool was installed to.
- `MISE_CONFIG_FILE`: the config file that declared the tool.
- `MISE_CONFIG_ROOT`: that file's [config root](/configuration.html#config-root).
- `MISE_PROJECT_ROOT`: the active project root, or the config root when no
  project is active.

A failing `postinstall` fails the install.

## System installations {#system-installations}

`mise install --system` installs a tool into a shared directory,
`/usr/local/share/mise/installs` by default, so every user account on the
machine can use one copy. Run it as your normal user; on Unix, mise calls
`sudo` only to write into the protected directory. See
[System installs](/dev-tools/system-installs.html) for supported backends,
directories, and sudo settings.

## Caching {#caching-and-performance}

mise reuses a tool's list of remote versions for the period set by
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache);
run `mise cache clear <tool>` to refresh it. See
[Caches](/cache-behavior.html), and
[slow shell prompts](/troubleshooting.html#slow-shell-prompts) if activation
feels slow.
