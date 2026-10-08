---
description: "Configure tools, environment variables, and tasks in mise.toml, and control where mise finds and writes config files."
socialDescription: "Configure tools, environment variables, and tasks in mise.toml, and where mise finds config files."
---

# mise.toml {#mise-toml-reference}

A project's `mise.toml` declares the tools, environment variables, and tasks the
project needs. mise combines it with your global config and with config files in
parent directories.

```mise-toml [mise.toml]
[tools]
node = "24"

[env]
NODE_ENV = "development"

[tasks.hello]
run = "node --eval 'console.log(process.env.NODE_ENV)'"
```

Run `mise run hello` to install Node.js if it is missing and print `development`.
Run [`mise config`](/cli/config.html) to list the config files mise loaded, and
`mise ls --current` to see the selected tool versions.

| Configure                                       | Reference                                                      |
| ----------------------------------------------- | -------------------------------------------------------------- |
| Tool versions and installation options          | [Dev tools](/dev-tools/)                                       |
| Environment variables passed to commands        | [Environment variables](/environments/)                        |
| Values reused inside templates                  | [Config variables](/configuration/vars.html)                   |
| Development, test, and production overlays      | [Config environments](/configuration/environments.html)        |
| Commands and their dependencies                 | [Tasks](/tasks/)                                               |
| Commands run on `cd`, on install, or on changes | [Hooks](/hooks.html)                                           |
| Checks that a machine can build the project     | [Project diagnostics](/configuration/project-diagnostics.html) |
| Long-running services                           | [Daemons](/daemons.html)                                       |
| mise's own behavior                             | [Settings](/configuration/settings.html)                       |

[Top-level keys](#top-level-keys) lists every section a `mise.toml` can contain.

## Config file locations {#mise-toml}

In each directory, mise reads these files. A file higher in the table overrides
one lower down.

| File                         | Use                                                                                    |
| ---------------------------- | -------------------------------------------------------------------------------------- |
| `mise.local.toml`            | Personal overrides. Keep it out of version control.                                    |
| `mise.toml`                  | Shared project config                                                                  |
| `mise/config.toml`           | Project config in a `mise/` directory                                                  |
| `mise/conf.d/*.toml`         | Config fragments, loaded in alphabetical order; see [conf.d](#conf-d)                  |
| `.mise/config.toml`          | Project config in a hidden `.mise/` directory                                          |
| `.mise/conf.d/*.toml`        | Fragments in `.mise/`                                                                  |
| `.config/mise.toml`          | Project config under `.config/`                                                        |
| `.config/mise/config.toml`   | Project config in `.config/mise/`                                                      |
| `.config/mise/conf.d/*.toml` | Fragments in `.config/mise/`                                                           |
| `.tool-versions`             | Tool versions in the [`.tool-versions` format](/dev-tools/versions.html#tool-versions) |

- `mise.toml` and `mise.local.toml` also work as dotfiles. `.mise.toml`
  overrides `mise.toml`, and `.mise.local.toml` overrides `mise.local.toml`.
- Each `config.toml` can have a `config.local.toml` beside it, such as
  `.config/mise/config.local.toml`, and `.config/mise.toml` can have
  `.config/mise.local.toml`. Every local file overrides every shared file in
  the same directory.
- Environment files such as `mise.production.toml` override the shared files
  when their environment is selected, and the local files override them in
  turn. Environment local files such as `mise.production.local.toml` override
  all of these. With several environments selected, a later environment's file
  overrides an earlier one's of the same kind. See
  [Config environments](/configuration/environments.html#file-names-and-precedence).
- The [`override_config_filenames`](/configuration/settings.html#override_config_filenames)
  setting replaces the TOML names in this list with your own (environment files
  still load), and
  [`override_tool_versions_filenames`](/configuration/settings.html#override_tool_versions_filenames)
  replaces `.tool-versions`.
  [`default_config_filename`](/configuration/settings.html#default_config_filename)
  changes the name mise creates.

`mise config` lists the files mise loaded, lowest precedence first: later files
override earlier ones.

## How config files combine {#configuration-hierarchy}

mise loads config in this order, and a later file overrides an earlier one:

1. System config in `/etc/mise`.
2. Global config in `~/.config/mise`.
3. Config files in every directory from the filesystem root down to the current
   directory. The search starts below the nearest
   [`ceiling_paths`](/configuration/settings.html#ceiling_paths) directory when
   one is set; files in the ceiling directory itself are not loaded.

So a file in a deeper directory overrides one in a parent directory. Within one
directory, the order in [Config file locations](#mise-toml) applies. Selecting a
[config environment](/configuration/environments.html) adds environment files
at each level of this search. An environment file in a parent directory does not
override an ordinary file in a child directory.

```text
/
├── etc/mise/                         # system config (lowest precedence)
│   ├── conf.d/*.toml
│   ├── config.toml
│   └── config.<env>.toml
└── home/user/
    ├── .config/mise/                 # global config
    │   ├── conf.d/*.toml
    │   ├── config.toml
    │   ├── config.<env>.toml
    │   ├── config.local.toml
    │   └── config.<env>.local.toml
    └── work/
        ├── mise.toml                 # shared by every project in work/
        └── myproject/
            ├── mise.toml             # project config
            ├── mise.<env>.toml       # environment config
            ├── mise.local.toml       # personal overrides, not committed
            ├── mise.<env>.local.toml
            └── backend/
                └── mise.toml         # nearest file (highest precedence)
```

Values merge rather than replace whole files. With these three files, mise uses
the `node` request from `mise.local.toml` and the `python` request from the
global config when you work in `~/src/app`:

::: code-group

```toml [~/.config/mise/config.toml]
[tools]
node = "22"
python = "3.13"
```

```toml [~/src/app/mise.toml]
[tools]
node = "24"
```

```toml [~/src/app/mise.local.toml]
[tools]
node = "25"
```

:::

These are version requests, which mise then resolves to concrete versions.

### How each section merges {#merge-behavior-by-section}

| Section              | How files combine                                                                                                                                                                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `[tools]`            | Per tool. The nearest file's entry replaces that tool's whole entry, including its options. Other tools are inherited.                                                                                                                                |
| `[env]`              | Per variable; the nearest file wins. See [Environment variables](/environments/).                                                                                                                                                                     |
| `[vars]`             | Per variable; the nearest file wins.                                                                                                                                                                                                                  |
| `[settings]`         | Per setting.                                                                                                                                                                                                                                          |
| `[tasks]`            | Per task name. A task from a nearer directory replaces the parent's task. Within one directory, a block without `run`, `run_windows`, or `file` adds metadata instead. See [which definition wins](/tasks/task-discovery.html#which-definition-wins). |
| `[tool_config]`      | Not merged. It applies only to tools declared in the same [config root](#config-root).                                                                                                                                                                |
| `[secrets.*]`        | Per field; the nearest project file wins. Ignored in global config.                                                                                                                                                                                   |
| `[daemons]`          | Per daemon. The nearest declaration replaces the whole daemon.                                                                                                                                                                                        |
| `[daemons_settings]` | Per key, and inherited by child projects. Ignored in global and system config.                                                                                                                                                                        |

## Split config with conf.d {#conf-d}

Put `*.toml` files in a `conf.d` directory to split config by topic. mise loads
them in alphabetical order, before the directory's own `config.toml`, so
`config.toml` overrides its fragments. This works in `mise/conf.d`,
`.mise/conf.d`, and `.config/mise/conf.d` in a project, and in the global
(`~/.config/mise/conf.d`) and system (`/etc/mise/conf.d`) directories. Files
whose names start with `.` are skipped.

Dots in fragment names, such as `node.tools.toml`, are deprecated; rename the
file to `node-tools.toml`. See
[conf.d environments](/configuration/environments.html#conf-d-environments).

### conf.d folders {#conf-d-folders}

A folder inside a `conf.d` directory is a fragment that keeps its config next to
the files it uses:

```text
~/.config/mise/conf.d/
├── git.toml                  # single-file fragment
└── git-tools/                # folder fragment
    ├── mise.toml             # always loaded
    ├── mise.local.toml       # always loaded, usually gitignored
    ├── mise.linux.toml       # loaded when the linux environment is active
    ├── mise.linux.local.toml
    └── gitconfig
```

The folder is the [config root](#config-root) for its files. Relative paths,
such as a [dotfile](/dotfiles.html) source of `"gitconfig"`, resolve inside the
folder, <code v-pre>{{ config_root }}</code> is the folder's path, and tasks
defined in the folder run there by default. The folder can be a symlink, so a
dotfiles checkout can keep its own layout:

```sh
ln -s ~/src/dotfiles/git ~/.config/mise/conf.d/git-tools
```

mise reads only `mise.toml`, `mise.local.toml`, `mise.<env>.toml`, and
`mise.<env>.local.toml` from a folder, and does not search folders recursively.
Folders whose names start with `.` are ignored. `mise.<env>.toml` files load for
explicit [config environments](/configuration/environments.html) and
[platform environments](/configuration/environments.html#platform-environments),
and are not affected by the `env_conf_d` migration.

Folder fragments load after single-file fragments in the same `conf.d`
directory, in alphabetical order by folder name, and before the directory's own
config such as `config.toml`. Their environment files take the same place as
`conf.d/<name>.<env>.toml` and `conf.d/<name>.<env>.local.toml` would. A
folder's `mise.local.toml` loads just before the directory's own
`config.local.toml`: it overrides the `.<env>.toml` files in the folder and the
directory, but not their `.<env>.local.toml` files, following the
[environment file order](/configuration/environments.html#file-names-and-precedence).
Tools declared in a project folder fragment share the project's lockfile.

For tasks, a folder is its own root. Its `[task_config]` applies only to the
tasks it defines, and `task_config.includes = ["tasks"]` loads file tasks from
the folder's `tasks/` directory without replacing the default task directories
of the surrounding config, such as `~/.config/mise/tasks`. `[task_config]`
values that the surrounding config sets with `cascade = true` still apply,
except `includes`. When a folder and other config define a task with the same
name, the task from the higher-precedence file wins, and a task found only in a
default task directory, such as `.mise/tasks`, loses to one a config file
defines. See [Task discovery and precedence](/tasks/task-discovery.html).

## Global and system config {#global-config}

Global config applies in every directory. It lives in `~/.config/mise`
([`MISE_CONFIG_DIR`](/directories.html#config-mise)), normally in `config.toml`.
The directory can also hold `mise.toml`, `conf.d/` fragments, `config.local.toml`
for machine-specific values, and environment files such as `config.work.toml`.
`mise use --global` and `mise settings set` write to it.

```toml [~/.config/mise/config.toml]
[tools]
node = "lts"
python = ["3.14", "3.13"]

[settings]
idiomatic_version_file_enable_tools = ["node"]
trusted_config_paths = ["~/work"]

[settings.status]
show_env = false
show_tools = false
```

Global config differs from project config in a few ways:

- Its [config root](#config-root) is your home directory, or
  [`global_config_root`](/configuration/settings.html#global_config_root).
- `[secrets.*]` and `[daemons_settings]` are ignored there, and
  `[daemon_providers]` is allowed only there.
- [`global_config_file`](/configuration/settings.html#global_config_file)
  (`MISE_GLOBAL_CONFIG_FILE`) replaces all of these files with one file.

System config applies to every user on the machine and has the lowest
precedence. It lives in `/etc/mise`
([`MISE_SYSTEM_CONFIG_DIR`](/directories.html#system-config)) and accepts the
same file names as the global directory, such as `/etc/mise/config.toml`.
[`system_config_file`](/configuration/settings.html#system_config_file)
(`MISE_SYSTEM_CONFIG_FILE`) replaces them with a single file.

## Which file mise writes to {#target-file-for-write-operations}

When [`mise use`](/cli/use.html), [`mise set`](/cli/set.html),
[`mise unset`](/cli/unset.html), `mise settings set --local`, and
[`mise tasks add`](/cli/tasks/add.html) change project config, they write to the
lowest-precedence file in the nearest directory that has config. Shared config
is updated by default, and personal and environment files change only when you
ask for them:

```sh
# In a directory with mise.toml and mise.local.toml:
mise use node@24                # writes mise.toml
mise set NODE_ENV=production    # writes mise.toml
mise use --env local node@25    # writes mise.local.toml
mise use --env staging node@24  # writes mise.staging.toml
```

When the directory has only `mise.local.toml`, writes go to `mise.local.toml`.
From a subdirectory without config, writes go to the parent directory's file.
When no directory has a config file, mise creates `mise.toml` in the current
directory; in your home directory, it writes the global config instead. Use
`--global` to write the global config, or `--path` to choose a file.

Other commands choose differently:

- [`mise config get`](/cli/config/get.html) and
  [`mise config set`](/cli/config/set.html) use the highest-precedence loaded
  TOML file, which can be `mise.local.toml`. Use `--file` to choose another
  project file.
- [`mise unuse`](/cli/unuse.html) uses the first loaded config that declares a
  requested tool. A version-qualified argument matches the configured request
  literally: `node@20` matches `node = "20"`, not `node = "20.0.0"`. Use
  `--path` to choose the file.

### Send global writes to separate files {#global-section-write-targets}

To keep global tools, bootstrap packages, and dotfiles in separate files, set a
target for each in [`write_targets`](/configuration/settings.html#write_targets):

```toml [~/.config/mise/config.toml]
[settings.write_targets]
tools = "~/.config/mise/conf.d/10-tools.toml"
packages = "~/.config/mise/conf.d/20-packages.toml"
dotfiles = "~/.config/mise/conf.d/30-dotfiles.toml"
```

New declarations from `mise use --global`,
`mise bootstrap packages use --global`,
`mise bootstrap packages import --global`, and `mise dotfiles add` (which
writes global config by default) then go to those files. A target must be an
absolute path (or start with `~/`) to a file mise loads from the global config
directory: `config.toml`, `mise.toml`, or a `conf.d` fragment. A path such as
`~/.config/mise/tools.toml` is not loaded, so mise rejects it.

Only new entries move. An existing tool, package, or dotfile stays in the file
that declares it, and when one command updates entries found in more than one
global file, mise stops and asks for `--path`. `--path` always wins, and project
writes such as `mise use --env staging` are not affected. Unset targets use the
normal global config file.

## .miserc.toml {#miserc}

`.miserc.toml` holds settings mise needs before it looks for config files, such
as which [config environments](/configuration/environments.html) to load. mise
reads it before any `mise.toml`, so setting these values under `[settings]` in
`mise.toml` has no effect on config discovery.

```toml [.miserc.toml]
env = ["development"]
```

It accepts only these keys:
[`env`](/configuration/settings.html#env),
[`auto_env`](/configuration/settings.html#auto_env),
[`env_conf_d`](/configuration/settings.html#env_conf_d),
[`ceiling_paths`](/configuration/settings.html#ceiling_paths),
[`ignored_config_paths`](/configuration/settings.html#ignored_config_paths),
[`override_config_filenames`](/configuration/settings.html#override_config_filenames),
and
[`override_tool_versions_filenames`](/configuration/settings.html#override_tool_versions_filenames).
Its JSON schema is
[schema/miserc.json](https://github.com/jdx/mise/blob/main/schema/miserc.json).
Relative paths in `ignored_config_paths` resolve from the directory that holds
the `.miserc.toml` file.

### Where mise looks for .miserc.toml {#miserc-locations}

From highest to lowest precedence:

1. The current directory, then each parent: `.miserc.local.toml`,
   `.miserc.toml`, then `.config/miserc.toml`. The search ends after your home
   directory, or at the filesystem root when you are outside it, and stops
   before any directory listed in the `MISE_CEILING_PATHS` environment
   variable, which is not searched. In the home directory and at the root, only
   `.miserc.local.toml` and `.miserc.toml` are read.
2. `~/.config/mise/miserc.local.toml`, then `~/.config/mise/miserc.toml`, read
   from any directory.
3. `/etc/mise/miserc.toml`.

A file overrides only the keys it sets; the others keep their inherited values.
`env` replaces the inherited list, and `env = []` clears it. The `-E` flag and
environment variables such as `MISE_ENV` override every `.miserc.toml` file.

### Personal and machine-wide choices

Use `.miserc.local.toml` for a choice that belongs to your checkout rather than
the whole project:

```toml [.miserc.local.toml]
env = ["native"]
```

Commands such as `mise install` and `mise run dev` then load `mise.native.toml`.
Add `.miserc.local.toml` to your global Git ignore file (`core.excludesFile`) so
it stays untracked in every repository, and create it separately in each
worktree.

To choose for the whole machine without editing a shared global
`miserc.toml`, use `miserc.local.toml` in `~/.config/mise`:

```toml [~/.config/mise/miserc.local.toml]
env = ["work"]
```

Project `.miserc.toml` and `.miserc.local.toml` files can still override it.

### Templates in .miserc.toml

Values in `.miserc.toml` can use [Tera templates](/templates.html) with a
limited context, such as <code v-pre>ceiling_paths = ["{{ env.HOME }}"]</code>.
See [templates in .miserc.toml](/templates.html#miserc-template-support) for
what is available. If a template fails to render, mise uses the file's raw
content without printing a warning.

## config_root {#config-root}

`config_root` is the directory a config file's relative paths resolve against,
and the value of <code v-pre>{{ config_root }}</code> in its templates. For
project config it is the project directory, even when the file is
`.config/mise.toml`, `.config/mise/config.toml`, `.mise/config.toml`,
`mise/config.toml`, or a `conf.d/*.toml` fragment, and even when you run mise
from a subdirectory. Tasks defined in the file run there by default. The
exceptions are `.config/mise/mise.toml` and `.config/mise/mise.local.toml`,
whose root is `.config/mise/` itself.

| Config file                                  | `config_root`                |
| -------------------------------------------- | ---------------------------- |
| `~/src/app/mise.toml`                        | `~/src/app`                  |
| `~/src/app/.config/mise.toml`                | `~/src/app`                  |
| `~/src/app/.config/mise/config.toml`         | `~/src/app`                  |
| `~/src/app/.config/mise/conf.d/tools.toml`   | `~/src/app`                  |
| `~/src/app/.mise/config.toml`                | `~/src/app`                  |
| `~/src/app/mise/conf.d/node/mise.toml`       | `~/src/app/mise/conf.d/node` |
| `~/.config/mise/config.toml` (global config) | `~`                          |

A [conf.d folder](#conf-d-folders) is its own root. For global config, set
[`global_config_root`](/configuration/settings.html#global_config_root) to use
another directory.

```toml
[env]
# Both entries resolve against the project root
_.path = ["tools/bin", "{{ config_root }}/tools/bin"]

# Same as "{{ config_root }}/scripts/env.sh"
_.source = "scripts/env.sh"
```

## Top-level keys {#top-level-keys}

| Key                                 | Purpose                                                                 | Documentation                                                      |
| ----------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `[tools]`                           | Tool versions and per-tool options                                      | [Dev tools](/dev-tools/), [tool options](/dev-tools/#tool-options) |
| `[tool_config]`                     | Policy for the tools this config root declares                          | [`[tool_config]`](#tool-config)                                    |
| `[tool_alias]`                      | Custom version names for a tool                                         | [Tool aliases](/dev-tools/aliases.html)                            |
| `[plugins]`                         | Plugin repository URLs and local plugin paths                           | [`[plugins]`](#plugins)                                            |
| `[env]`                             | Environment variables for commands and tasks                            | [Environment variables](/environments/)                            |
| `[vars]`                            | Values for templates, not exported                                      | [Config variables](/configuration/vars.html)                       |
| `redactions`                        | `env` or `vars` keys to hide in output                                  | [Redaction](/environments/secrets/#redaction)                      |
| `[secrets.*]` (experimental)        | Sources for mise secrets; project config only                           | [fnox](/environments/secrets/fnox.html)                            |
| `[tasks]`                           | Tasks                                                                   | [Tasks](/tasks/)                                                   |
| `[task_config]`                     | Task includes, excludes, and defaults                                   | [Task configuration reference](/tasks/task-configuration.html)     |
| `[task_templates]`                  | Shared task definitions for `extends`                                   | [Task templates](/tasks/templates.html)                            |
| `[settings]`                        | mise's own behavior                                                     | [Settings](/configuration/settings.html)                           |
| `[hooks]`                           | Commands run on `cd`, enter, leave, and install                         | [Hooks](/hooks.html)                                               |
| `[[watch_files]]`                   | Commands run when files change                                          | [Hooks](/hooks.html)                                               |
| `[shell_alias]`                     | Shell aliases defined while you are in the directory; needs activation  | [Shell aliases](/shell-aliases.html)                               |
| `[wrappers]`                        | Commands that intercept a binary name                                   | [Command wrappers](/dev-tools/shims.html#command-wrappers)         |
| `include`                           | Remote config files merged into this one                                | [`include`](#include)                                              |
| `min_version`                       | Oldest mise release that can use this file                              | [`min_version`](#minimum-mise-version)                             |
| `monorepo_root`, `[monorepo]`       | `//path:task` addressing; trusting the root trusts the configs below it | [Monorepo tasks](/tasks/monorepo.html)                             |
| `[doctor]`                          | Project checks for `mise doctor project`                                | [Project diagnostics](/configuration/project-diagnostics.html)     |
| `[deps]` (experimental)             | Project dependency providers                                            | [Project dependencies](/dev-tools/deps.html)                       |
| `[oci]` (experimental)              | Settings for `mise oci build`                                           | [OCI images](/dev-tools/mise-oci.html)                             |
| `[daemons]` (experimental)          | Background processes and service presets                                | [Daemons](/daemons.html)                                           |
| `[daemon_groups]` (experimental)    | Named sets of daemons                                                   | [Groups](/daemons.html#groups)                                     |
| `[daemons_settings]` (experimental) | Namespace for daemon IDs and hostnames                                  | [Ports, URLs, and worktrees](/daemons/worktrees.html#namespaces)   |
| `[daemon_providers]` (experimental) | Shared servers; global config only                                      | [Share daemons across projects](/daemons/sharing.html)             |
| `[bootstrap]`                       | Machine setup: packages, repositories, services, and more               | [Bootstrap](/bootstrap.html)                                       |
| `[dotfiles]`                        | Files mise manages in your home directory                               | [Dotfiles](/dotfiles.html)                                         |
| `[dotfile_groups]`                  | Named directory trees of dotfiles                                       | [Groups](/dotfiles/groups.html)                                    |
| `[history]`                         | What dotfiles history captures                                          | [History](/dotfiles/history.html)                                  |
| `[_]`                               | Free-form data that mise ignores                                        | [`[_]`](#free-form-data)                                           |

Version requests, prefixes such as `prefix:1.25`, and `.tool-versions` are
described in [Version requests and version files](/dev-tools/versions.html).
A global tool can update itself with
[`auto_update`](/dev-tools/#automatic-tool-updates).

These keys are deprecated:

| Deprecated key               | Use instead     | Removed in  |
| ---------------------------- | --------------- | ----------- |
| `[alias]`                    | `[tool_alias]`  | No date set |
| `env_file`, `dotenv`         | `env._.file`    | 2027.4.0    |
| `env_path`                   | `env._.path`    | 2027.4.0    |
| `experimental_monorepo_root` | `monorepo_root` | 2027.12.0   |

### `[tool_config]` {#tool-config}

`[tool_config]` applies policy to the tools declared by config files that share
its [config root](#config-root). Policy in `mise.local.toml` also applies to the
tools in `mise.toml` beside it. It does not affect tools inherited from global,
system, or parent config roots.

```toml
[tool_config]
locked = true

[tools]
node = "24"
```

`locked` is the only policy. It requires this config root's tools to resolve and
install from their lockfiles; see
[strict lockfile mode](/dev-tools/mise-lock.html#strict-lockfile-mode).

### `[plugins]` {#plugins}

`[plugins]` sets the repository for a plugin name, so everyone on the project
installs the same plugin:

```toml
[plugins]
elixir = "https://github.com/my-org/mise-elixir.git"
node = "https://github.com/my-org/mise-node.git#v1.2.0" # a branch, tag, or full commit SHA
"vfox-backend:myplugin" = "https://github.com/jdx/vfox-npm"
```

A ref pinned to a commit must be the full SHA; mise rejects abbreviated SHAs
because they cannot be fetched from the remote. The plugin type prefix, such as
`asdf:`, `vfox:`, or `vfox-backend:`, is optional. Without it, mise clones the
plugin and detects its type from the plugin's files.

An entry applies only to new installs. When an installed plugin no longer
matches its entry (a different URL, or a ref that is not checked out),
`mise install`, `mise plugins install`, and `mise doctor` warn about it. Run
`mise plugins install --force <name>` to reinstall it from `[plugins]`. To
install a plugin from a URL once without sharing it, run
`mise plugins install <name> <git-url>` instead.

An entry can also point at a local directory. Absolute paths and paths starting
with `~/` are used as they are. Relative paths starting with `./` or `../`
resolve from the [config root](#config-root) of the file that declares them:

```toml
[plugins]
example = "./plugins/mise-example"
```

mise symlinks a local plugin into its plugin directory, as
[`mise plugins link`](/cli/plugins/link.html) does, so changes to the source
are available immediately. `file://` URLs are Git repositories and are cloned.

`[plugins]` replaces the deprecated `shorthands_file` setting
(`MISE_SHORTHANDS_FILE`): move its `shortname = "backend-or-url"` rows here.

### `include` {#include}

`include` merges remote config files into this one, so an organization can
publish a baseline that every repository uses:

```toml
include = [
  "git::https://github.com/myorg/platform.git//mise.toml?ref=main",
  "oci::ghcr.io/myorg/platform-config@sha256:0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0",
]

[tools]
node = "24" # this file's own entries override the included ones
```

A `git::` include points at a `.toml` file. An `oci::` include points at an
artifact with a `mise.toml` at its root.

An included file ranks directly below the file that includes it, and a later entry
in `include` overrides an earlier one. Its relative paths, such as `_.file`,
and <code v-pre>{{ config_root }}</code> resolve against the including file. Its
tools are locked in the including file's lockfile, and its `min_version` is
enforced.

An included file can contain `[tools]`, `[tool_alias]`, `[env]`, `[vars]`,
`[hooks]`, `[shell_alias]`, `[plugins]`, `[wrappers]`, `[bootstrap]`,
`min_version`, and `[_]`. The deprecated `[alias]` and `env_path` keys are also
accepted. Its hooks
run before the including file's hooks for the same event. `[bootstrap]` entries
merge key by key, and the including file's own entry wins. Any other key is an
error: `include` (includes do not nest), `[settings]` and the monorepo keys
(mise reads them before it resolves includes), `[tasks]`, `task_config`, and
`task_templates` (share tasks with
[`task_config.includes`](/tasks/task-discovery.html)), and machine sections such
as `[dotfiles]` and `[daemons]`.

An include uses the including file's trust. An untrusted project cannot make
mise fetch a URL when you `cd` into it, and
[safe mode](/security.html#safe-mode) never fetches includes for project config.
Once you trust the file, the included content runs with that trust, as a
`mise.toml` that changes on `git pull` does. In
[paranoid mode](/paranoid.html#remote-includes), an include must be pinned to a
commit SHA or an OCI digest.

mise caches each included file under `MISE_CACHE_DIR` and never refetches a
commit SHA or an OCI digest. Commands such as `mise install`, `mise use`, and
`mise upgrade` refresh a branch, tag, or OCI tag once the cached copy is older
than
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache).
Shell activation, `mise exec`, `mise env`, `mise ls`, shims, and
[offline](/configuration/settings.html#offline) mode always use the cached copy.
If a refresh fails or returns a file this mise cannot load, mise warns and keeps
the cached copy; with nothing cached, it is an error. `mise cache clear` forces
a refetch.

### `min_version` {#minimum-mise-version}

A plain string is a hard minimum. Older mise releases stop with an error and
print upgrade instructions:

```toml
min_version = "2026.1.0"
```

Add a soft minimum to warn without failing:

```toml
min_version = { hard = "2025.6.0", soft = "2026.1.0" }
```

Set `hard` to the oldest release that understands this file and `soft` to the
release you want teammates on. `{ soft = "2026.1.0" }` on its own is also valid.

### `[_]` {#free-form-data}

mise never reads the `[_]` table, so you can keep your own data in a config
file without mise rejecting the key:

```toml
[_]
owner = "platform-team"
```

### Editor schema {#mise-toml-schema}

The JSON schema for `mise.toml` is at <https://mise.jdx.dev/schema/mise.json>
and in the [JSON Schema Store](https://www.schemastore.org/), so editors such as
[VS Code](https://code.visualstudio.com/docs/languages/json#_json-schemas-and-settings),
[IntelliJ](https://www.jetbrains.com/help/idea/json.html#ws_json_using_schemas),
and [Neovim](https://github.com/b0o/SchemaStore.nvim) can complete and validate
it. Task files loaded through `task_config.includes` use a separate schema:
<https://mise.jdx.dev/schema/mise-task.json>.

## Idiomatic version files {#idiomatic-version-files}

mise can read the version files other tools use, such as `.nvmrc` and
`.python-version`, once you enable them for a tool. See
[Idiomatic version files](/dev-tools/versions.html#idiomatic-version-files) for
the supported files and how to enable them.
