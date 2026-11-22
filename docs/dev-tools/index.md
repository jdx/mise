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

Each version is installed once, under
`~/.local/share/mise/installs/<tool>/<version>`, and every project that selects
it uses that copy; see [Directories](/directories.html#local-share-mise-installs).

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
mise upgrade --bump --no-global # bump only tools in project config
mise upgrade --global       # upgrade only tools in global config
```

`--global` upgrades the global config's requests even inside a project that
sets its own version of a tool, and never changes project config or
lockfiles. `mise outdated --global` reports what it would install. Tools that
update themselves use these two commands; see
[Self-updating tools](/dev-tools/self-updating-tools.html).

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

In a shell with [`mise activate`](/shell-setup.html), tools run straight from
`PATH`, so nothing sits between you and the command. Instead, when the shell
prompt appears and a tool's check is due, mise starts the update in the
background and the prompt does not wait. Commands use the new version once the
update has finished and the shell has refreshed its environment, which happens
at a prompt. Its output is discarded; if it fails, `mise doctor` shows the
error.

`auto_update = true` checks every
[`tool_update.check_duration`](/configuration/settings.html#tool_update.check_duration).
A duration such as `"6h"` sets that tool's own interval. Intervals under one
hour are raised to one hour.

To have the tools update with the newest mise, set
[`tool_update.self_update_first`](/configuration/settings.html#tool_update.self_update_first)
along with [`self_update.auto`](/configuration/settings.html#self_update.auto).
Each automatic update pass then updates mise first, and skips the tools, with
the error in `mise doctor`, if that fails.

To update every tool in your global config without marking each one, set
[`tool_update.global_auto`](/configuration/settings.html#tool_update.global_auto)
to `true` or an interval. A tool's own `auto_update` still wins, so
`auto_update = false` opts it out:

```toml [~/.config/mise/config.toml]
[settings]
tool_update.global_auto = "12h"

[tools]
claude = "latest"
node = { version = "24", auto_update = "6h" }
python = { version = "3.13", auto_update = false }
```

::: tip A 24h delay can mean up to 48h
Updates respect [`minimum_release_age`](/security.html#minimum-release-age),
which defaults to `24h` for
[every backend](/security.html#which-backends-have-a-default). A release becomes
eligible 24 hours after publishing, and mise only notices it at the next check,
so an update lands 24 to 48 hours after a release if you launch the tool at
least once per check interval, and later if you don't. This holds only for a
backend that reports release dates. A release it cannot date is not held back,
and mise warns when it installs one.
:::

To wait longer, set `minimum_release_age` on the tool:

```toml [~/.config/mise/config.toml]
[tools]
node = { version = "24", auto_update = "6h", minimum_release_age = "3d" }
```

### When updates run and what they change {#automatic-update-rules}

- Only global config can turn this on. A project config cannot, and when a
  project sets its own version of the tool, runs in that project do not update
  it.
- A launch checks only the tool being run: `mise exec -- npm test` does not
  update `claude`. In an activated shell, a prompt checks every opted-in tool
  whose interval has elapsed. Tasks never update tools.
- Exact versions such as `node = "24.11.1"` are never updated. If a global
  lockfile (`mise lock --global`) pins the tool, the update moves the lock
  entry to the new version. A project's config and lockfile are never changed.
- Only installed tools are updated. A tool with no installed version, such as a
  [lazy tool](/dev-tools/shims.html#lazy-tools) nobody has run yet, is left
  alone until something installs it.
- No updates run offline, in CI, or with `locked = true`.
- The previous version is pruned on the same schedule as after `mise upgrade`.
- If the last update of a tool failed, `mise doctor` shows the error.

### Update on a schedule {#scheduled-tool-updates}

To check on a schedule instead of at launches and prompts, so tools stay
current even when you are not using a shell, declare the `tool-update` service
in your global config and run `mise bootstrap services apply`:

```toml [~/.config/mise/config.toml]
[bootstrap.services.mise-tool-update]
builtin = "tool-update"
```

The service checks once an hour and updates each tool when its interval is
due. While it runs, launches and prompts do not update tools themselves. See
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

A recorded file that is deleted stops protecting its versions straight away.
mise forgets it a week later unless it comes back, or at once when you run
`mise prune`. mise also forgets a file that no mise command has loaded or run
for
[`prune.forget_unused_after`](/configuration/settings.html#prune.forget_unused_after)
(90 days by default). A project you come back to after that is recorded again
the next time you run mise in it; run `mise install` there to reinstall any
tools that were pruned meanwhile.

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

Write a tool as a table instead of a version string to set options on it. Some
options work with every tool, such as `os`, `depends`, and `postinstall`;
others belong to the tool's backend:

```toml [mise.toml]
[tools]
# install ripgrep only on Linux and macOS
ripgrep = { version = "15", os = ["linux", "macos"] }
# run a command each time mise installs a Node.js version
node = { version = "24", postinstall = "corepack enable" }
```

[Tool options](/dev-tools/tool-options.html) lists the options every tool
accepts and covers the command-line form, templates, tool dependencies, and
what mise does with an option a backend does not read. Options that belong to
one backend are on each [backend page](/dev-tools/backends/).

## System installations {#system-installations}

`mise install --system` installs a tool into a shared directory,
`/usr/local/share/mise/installs` by default, so every user account on the
machine can use one copy. Run it as your normal user; on Unix, mise calls
`sudo` only to write into the protected directory. See
[System installs](/dev-tools/system-installs.html) for supported backends,
directories, and sudo settings.

## Caching {#caching-and-performance}

mise reuses a tool's list of remote versions for a while: up to the tool's
[`minimum_release_age`](/configuration/settings.html#minimum_release_age) when
installing, and for
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache)
when reporting what is newest. Run `mise cache clear <tool>` to refresh it. See
[Caches](/cache-behavior.html), and
[slow shell prompts](/troubleshooting.html#slow-shell-prompts) if activation
feels slow.

## Next steps {#next-steps}

- [Version requests and version files](/dev-tools/versions.html) covers request
  syntax, how a request resolves, and `.tool-versions`, `.nvmrc`, and similar
  files.
- [Registry](/registry.html) lists the short tool names mise knows, and
  [Backends](/dev-tools/backends/) explains how to install a tool that has no
  short name, such as `github:owner/repo`.
- [Core tools](/core-tools.html) links to the guides for Node.js, Python, Go,
  and the other languages mise installs itself.
- [Lockfile](/dev-tools/mise-lock.html) records resolved versions and checksums
  so everyone gets the same builds.
- [Shims](/dev-tools/shims.html) covers editors and other programs that never
  see an activated shell, lazy tools, and command wrappers.
- [Git provider tokens](/dev-tools/github-tokens.html) covers GitHub rate
  limits and installs from private repositories.
- [Tool aliases](/dev-tools/aliases.html) point a tool name at another backend
  or give a version request a name.
- [Tool stubs](/dev-tools/tool-stubs.html) are committed executable files that
  each install and run one tool.
