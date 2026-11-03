---
description: "Adopt mise in an existing repository, share the setup with your team, and run the project's commands as tasks."
socialDescription: "Adopt mise in an existing repository and share the setup with your team."
---

# Use mise in an existing project

Bring mise into a project you already work on: declare its tools, keep personal
settings out of the shared config, share the setup with your team, and run the
project's commands through mise. If you have not installed mise yet, start with
[Getting started](/getting-started.html).

The examples use `mise exec` and `mise run`, so shell activation is optional.
With [activation](/getting-started.html#activate-mise), the selected tools also
run directly at your prompt.

## Declare the project's tools {#installing-dev-tools}

Start from the version files the project already has. mise reads
`.tool-versions` without any setup:

```sh
cat .tool-versions
# nodejs 24
mise ls --current
# node  24.x.x  ~/src/app/.tool-versions  24
```

Files from other version managers, such as `.nvmrc`, `.node-version`,
`.python-version` and `.ruby-version`, are
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).
mise reads them only for the tools you enable, so enable them in the project's
config to keep those files as the source of truth:

```toml [mise.toml]
[settings]
idiomatic_version_file_enable_tools = ["node", "python"]
```

For tools the project does not declare yet, run [`mise use`](/cli/use.html)
from the project root. It installs the tools and writes the requests to
`mise.toml`:

```sh
mise use node@24 python@3.14
```

```toml [mise.toml]
[tools]
node = "24"
python = "3.14"
```

Choose how precise each request is. A prefix such as `node@24` accepts any
Node.js 24 release, so machines can resolve it differently. `mise use --pin`
writes the exact version instead, and a [lockfile](#share-the-setup) records
exact versions while keeping prefixes in `mise.toml`. Without a version,
`mise use node` writes `node = "latest"`. See
[Version requests](/dev-tools/versions.html) for how each form resolves, and
[Choose the right command](/dev-tools/#choose-the-right-command) for how
`mise use` differs from `mise install`.

Language package managers work as backends too. Declare the runtime they need
next to them:

```sh
mise use node@24 'npm:@antfu/ni'
mise use rust@stable cargo:starship
```

See [Backends](/dev-tools/backends/) for the other sources mise can install
from. [`mise edit`](/cli/edit.html) opens an interactive editor for `mise.toml`
that can search the registry for tools.

## Keep personal settings local {#mise-toml-configuration}

`mise.toml` is shared with everyone who clones the project. Put your own
overrides, such as a different version for testing or a local
`DATABASE_URL`, in `mise.local.toml` next to it, and add that file to
`.gitignore`:

```sh
mise use --env local node@26
echo 'mise.local.toml' >> .gitignore
```

mise merges the config files in the current directory and every parent
directory, plus your global config. Files closer to the current directory win:

1. `~/.config/mise/config.toml`: your global defaults
2. `~/work/mise.toml`: settings for every project under `~/work`
3. `~/work/app/mise.toml`: the shared project config
4. `~/work/app/mise.local.toml`: your personal overrides for the project

Run [`mise config ls`](/cli/config/ls.html) to see which files are active, and
see [Config files](/configuration.html#mise-toml) for every location mise
reads.

`mise.local.toml` is plain text: the name does not encrypt it or keep it out of
Git. Keep secrets out of committed config, and use a
[secret provider](/environments/secrets/) for values that must stay private.

## Share the setup {#share-the-setup}

Commit `mise.toml`. To record the exact versions and download checksums that the
requests resolved to, create a lockfile and commit it too:

```sh
mise lock
git add mise.toml mise.lock
```

See [Lockfile](/dev-tools/mise-lock.html) for updating and enforcing it. If the
config uses a feature from a recent mise release, require that release with
[`min_version`](/configuration.html#minimum-mise-version) instead of asking
everyone to install one exact version:

```toml [mise.toml]
min_version = "2026.10.0"
```

After cloning, a teammate reads the project's config, since tasks, hooks and
`[env]` can run code, then runs:

```sh
mise install    # installs the declared tools and trusts the project
mise tasks ls   # lists the project's tasks
```

See [Configuration trust](/security.html#configuration-trust) for what trust
allows.

For contributors who do not have mise, commit a wrapper script that downloads a
pinned mise release on first use:

```sh
mise generate install-script --localize --write
./bin/mise install
```

The [CI page](/continuous-integration.html#bootstrapping) covers the wrapper's
options, including a Windows launcher.

## Run project commands as tasks {#tasks}

Wrap the commands the project already runs, so they get mise's tools and
environment:

```toml [mise.toml]
[tasks]
build = "npm run build"
test = "npm test"
```

```sh
mise run test
```

`mise run test` installs missing tools, sets `[env]`, then runs `npm test`,
with or without shell activation.

A task can also be an executable script in `mise-tasks/`. Do not define the
same task name in both `mise.toml` and `mise-tasks/`:

```sh [mise-tasks/build]
#!/usr/bin/env bash
npm run build
```

On Unix, make the script executable with `chmod +x mise-tasks/build`. File
tasks can declare flags and arguments in `#USAGE` comments; mise parses them,
adds `--help`, and completes them in your shell. See
[Task arguments](/tasks/task-arguments.html).

To call executables from the project's npm packages directly, add
`node_modules/.bin` to `PATH` for the project:

```toml [mise.toml]
[env]
_.path = "./node_modules/.bin"
```

The path is relative to the directory that holds `mise.toml`, so it also works
from subdirectories. See [Environment variables](/environments/) for more.

## Keep tools current {#upgrading-dev-tools}

[`mise outdated`](/cli/outdated.html) lists tools with newer versions, and
[`mise upgrade`](/cli/upgrade.html) installs them within the requests in
`mise.toml`. See [Upgrade tools](/dev-tools/#upgrade-tools) for `--bump`,
lockfiles and automatic updates.

## Everyday commands {#common-commands}

| Goal                           | Command                                                                                |
| ------------------------------ | -------------------------------------------------------------------------------------- |
| Show active config files       | [`mise config ls`](/cli/config/ls.html)                                                |
| Inspect selected tool versions | [`mise ls --current`](/cli/ls.html)                                                    |
| Find available releases        | [`mise ls-remote TOOL`](/cli/ls-remote.html)                                           |
| See tools with updates         | [`mise outdated`](/cli/outdated.html)                                                  |
| List project tasks             | [`mise tasks ls`](/cli/tasks/ls.html)                                                  |
| Diagnose environment problems  | [`mise doctor`](/cli/doctor.html)                                                      |
| Update mise itself             | [`mise self-update`](/cli/self-update.html), or the package manager used to install it |

## Further reading {#final-thoughts}

- [Dev tools](/dev-tools/): versions, backends and tool options
- [Environments](/environments/): variables, `.env` files and secrets
- [Tasks](/tasks/)
- [Configuration](/configuration.html) and [Settings](/configuration/settings.html)
- [Registry](/registry.html): short names such as `node` and the backend each maps to
- [CLI reference](/cli/)
