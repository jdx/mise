# Configuration

## `.rtx.toml`

`.rtx.toml` is a new config file that replaces asdf-style `.tool-versions` files with a file
that has lot more flexibility. It supports functionality that is not possible with `.tool-versions`, such as:

- setting arbitrary env vars while inside the directory
- passing options to plugins like `virtualenv='.venv'` for [python](https://github.com/jdx/rtx/blob/main/docs/python.md#experimental-automatic-virtualenv-creationactivation).
- specifying custom plugin URLs

They can use any of the following project locations (in order of precedence, top is highest):

- `.rtx.toml`
- `.rtx/config.toml`
- `.config/rtx.toml`
- `.config/rtx/rtx.toml`

They can also be named `.rtx.local.toml` and environment-specific files like `.rtx.production.toml`.
Can also be opted-in. See [Config Environments](/profiles) for more details.
Run `rtx config` to see the order of precedence on your system.

Here is what an `.rtx.toml` looks like:

```toml
[env]
# supports arbitrary env vars so rtx can be used like direnv/dotenv
NODE_ENV = 'production'

[tools]
# specify single or multiple versions
terraform = '1.0.0'
erlang = ['23.3', '24.0']

# supports everything you can do with .tool-versions currently
node = ['16', 'prefix:20', 'ref:master', 'path:~/.nodes/14']

# send arbitrary options to the plugin, passed as:
# RTX_TOOL_OPTS__VENV=.venv
python = {version='3.10', virtualenv='.venv'}

[plugins]
# specify a custom repo url
# note this will only be used if the plugin does not already exist
python = 'https://github.com/asdf-community/asdf-python'

[alias.node] # project-local aliases
my_custom_node = '20'
```

`.rtx.toml` files are hierarchical. The configuration in a file in the current directory will
override conflicting configuration in parent directories. For example, if `~/src/myproj/.rtx.toml`
defines the following:

```toml
[tools]
node = '20'
python = '3.10'
```

And `~/src/myproj/backend/.rtx.toml` defines:

```toml
[tools]
node = '18'
ruby = '3.1'
```

Then when inside of `~/src/myproj/backend`, `node` will be `18`, `python` will be `3.10`, and `ruby`
will be `3.1`. You can check the active versions with `rtx ls --current`.

You can also have environment specific config files like `.rtx.production.toml`, see
[Profiles](/profiles) for more details.

### `[env]` - Arbitrary Environment Variables

The `[env]` section of .rtx.toml allows setting arbitrary environment variables.
These can be simple key-value entries like this:

```toml
[env]
NODE_ENV = 'production'
```

`PATH` is treated specially, it needs to be defined as an array in `env_path`:

```toml
env_path = [
    # adds an absolute path
    "~/.local/share/bin",
    # adds a path relative to the .rtx.toml, not PWD
    "./node_modules/.bin",
]
```

_Note: `env_path` is a top-level key, it does not go inside of `[env]`._

Environment variable values can be templates, see [Templates](/templates) for details.

```toml
[env]
LD_LIBRARY_PATH = "/some/path:{{env.LD_LIBRARY_PATH}}"
```

`env_file` can be used to specify a [dotenv](https://dotenv.org) file to load:

```toml
env_file = '.env'
```

_Note: `env_file` goes at the top of the file, above `[env]`._

```toml
[env]
NODE_ENV = false # unset a previously set NODE_ENV
```

### `[plugins]` - Specify Custom Plugin Repository URLs

Use `[plugins]` to add/modify plugin shortnames. Note that this will only modify
_new_ plugin installations. Existing plugins can use any URL.

```toml
[plugins]
elixir = "https://github.com/my-org/rtx-elixir.git"
node = "https://github.com/my-org/rtx-node.git#DEADBEEF" # supports specific gitref
```

If you simply want to install a plugin from a specific URL once, it's better to use
`rtx plugin install plugin <GIT_URL>`. Add this section to `.rtx.toml` if you want
to share the plugin location/revision with other developers in your project.

This is similar to [`RTX_SHORTHANDS`](https://github.com/jdx/rtx#rtx_shorthands_fileconfigrtxshorthandstoml)
but doesn't require a separate file.

## Legacy version files

rtx supports "legacy version files" just like asdf. They're language-specific files like `.node-version`
and `.python-version`. These are ideal for setting the runtime version of a project without forcing
other developers to use a specific tool like rtx/asdf.

They support aliases, which means you can have an `.nvmrc` file with `lts/hydrogen` and it will work
in rtx and nvm. Here are some of the supported legacy version files:

| Plugin    | "Legacy" (Idiomatic) Files                         |
|-----------|----------------------------------------------------|
| crystal   | `.crystal-version`                                 |
| elixir    | `.exenv-version`                                   |
| go        | `.go-version`, `go.mod`                            |
| java      | `.java-version`, `.sdkmanrc`                       |
| node      | `.nvmrc`, `.node-version`                          |
| python    | `.python-version`                                  |
| ruby      | `.ruby-version`, `Gemfile`                         |
| terraform | `.terraform-version`, `.packer-version`, `main.tf` |
| yarn      | `.yarnrc`                                          |

In rtx these are enabled by default. You can disable them with `rtx settings set legacy_version_file false`.
There is a performance cost to having these when they're parsed as it's performed by the plugin in
`bin/parse-version-file`. However these are [cached](/cache-behavior) so it's not a huge deal.
You may not even notice.

> [!NOTE]
>
> asdf calls these "legacy version files" so we do too. I think this is a bad name since it implies
> that they shouldn't be used—which is definitely not the case IMO. I prefer the term "idiomatic"
> version files since they're version files not specific to asdf/rtx and can be used by other tools.
> (`.nvmrc` being a notable exception, which is tied to a specific tool.)

## `.tool-versions`

The `.tool-versions` file is asdf's config file and it can be used in rtx just like `.rtx.toml`.
It isn't as flexible so it's recommended to use `.rtx.toml` instead. It can be useful if you
already have a lot of `.tool-versions` files or work on a team that uses asdf.

Here is an example with all the supported syntax:

```
node        20.0.0       # comments are allowed
ruby        3            # can be fuzzy version
shellcheck  latest       # also supports "latest"
jq          1.6
erlang      ref:master   # compile from vcs ref
go          prefix:1.19  # uses the latest 1.19.x version—needed in case "1.19" is an exact match
shfmt       path:./shfmt # use a custom runtime
node        lts          # use lts version of node (not supported by all plugins)

node        sub-2:lts      # install 2 versions behind the latest lts (e.g.: 18 if lts is 20)
python      sub-0.1:latest # install python-3.10 if the latest is 3.11
```

See [the asdf docs](https://asdf-vm.com/manage/configuration.html#tool-versions) for more info on this file format.

## Scopes

Both `.rtx.toml` and `.tool-versions` support "scopes" which modify the behavior of the version:

- `ref:<SHA>` - compile from a vcs (usually git) ref
- `prefix:<PREFIX>` - use the latest version that matches the prefix. Useful for Go since `1.20`
  would only match `1.20` exactly but `prefix:1.20` will match `1.20.1` and `1.20.2` etc.
- `path:<PATH>` - use a custom compiled version at the given path. One use-case is to re-use
  Homebrew tools (e.g.: `path:/opt/homebrew/opt/node@20`).
- `sub-<PARTIAL_VERSION>:<ORIG_VERSION>` - subtracts PARTIAL_VERSION from ORIG_VERSION. This can
  be used to express something like "2 versions behind lts" such as `sub-2:lts`. Or 1 minor
  version behind the latest version: `sub-0.1:latest`.

## Global config: `~/.config/rtx/config.toml`

rtx can be configured in `~/.config/rtx/config.toml`. It's like local `.rtx.toml` files except that
it is used for all directories.

```toml
[tools]
# global tool versions go here
# you can set these with `rtx use -g`
node = 'lts'
python = ['3.10', '3.11']

[settings]
# plugins can read the versions files used by other version managers (if enabled by the plugin)
# for example, .nvmrc in the case of node's nvm
legacy_version_file = true                     # enabled by default (unlike asdf)
legacy_version_file_disable_tools = ['python'] # disable for specific tools

# configure `rtx install` to always keep the downloaded archive
always_keep_download = false        # deleted after install by default
always_keep_install = false         # deleted on failure by default

# configure how frequently (in minutes) to fetch updated plugin repository changes
# this is updated whenever a new runtime is installed
# (note: this isn't currently implemented but there are plans to add it: https://github.com/jdx/rtx/issues/128)
plugin_autoupdate_last_check_duration = '1 week' # set to 0 to disable updates

# config files with these prefixes will be trusted by default
trusted_config_paths = [
    '~/work/my-trusted-projects',
]

verbose = false     # set to true to see full installation output, see `RTX_VERBOSE`
asdf_compat = false # set to true to ensure .tool-versions will be compatible with asdf, see `RTX_ASDF_COMPAT`
jobs = 4            # number of plugins or runtimes to install in parallel. The default is `4`.
raw = false         # set to true to directly pipe plugins to stdin/stdout/stderr
yes = false         # set to true to automatically answer yes to all prompts

not_found_auto_install = true
task_output = "prefix" # see Task Runner for more information

shorthands_file = '~/.config/rtx/shorthands.toml' # path to the shorthands file, see `RTX_SHORTHANDS_FILE`
disable_default_shorthands = false # disable the default shorthands, see `RTX_DISABLE_DEFAULT_SHORTHANDS`
disable_tools = ['node']           # disable specific tools, generally used to turn off core tools

experimental = false # enable experimental features

[alias.node]
my_custom_node = '20'  # makes `rtx install node@my_custom_node` install node-20.x
                       # this can also be specified in a plugin (see below in "Aliases")
```

> [!TIP]
>
> These settings can also be managed with `rtx settings ls|get|set|unset`.

## System config: `/etc/rtx/config.toml`

Similar to `~/.config/rtx/config.toml` but for all users on the system. This is useful for
setting defaults for all users.

## Environment variables

rtx can also be configured via environment variables. The following options are available:

### `RTX_DATA_DIR`

Default: `~/.local/share/rtx` or `$XDG_DATA_HOME/rtx`

This is the directory where rtx stores plugins and tool installs. These are not supposed to be shared
across machines.

### `RTX_CACHE_DIR`

Default (Linux): `~/.cache/rtx` or `$XDG_CACHE_HOME/rtx`
Default (macOS): `~/Library/Caches/rtx` or `$XDG_CACHE_HOME/rtx`

This is the directory where rtx stores internal cache. This is not supposed to be shared
across machines. It may be deleted at any time rtx is not running.

### `RTX_TMP_DIR`

Default: [`std::env::temp_dir()`](https://doc.rust-lang.org/std/env/fn.temp_dir.html) implementation in rust

This is used for temporary storage such as when installing tools.

### `RTX_SYSTEM_DIR`

Default: `/etc/rtx`

This is the directory where rtx stores system-wide configuration.

### `RTX_CONFIG_FILE`

Default: `$RTX_CONFIG_DIR/config.toml` (Usually ~/.config/rtx/config.toml)

This is the path to the config file.

### `RTX_DEFAULT_TOOL_VERSIONS_FILENAME`

Set to something other than ".tool-versions" to have rtx look for `.tool-versions` files but with
a different name.

### `RTX_DEFAULT_CONFIG_FILENAME`

Set to something other than `.rtx.toml` to have rtx look for `.rtx.toml` config files with a different name.

### `RTX_ENV`

Enables environment-specific config files such as `.rtx.development.toml`.
Use this for different env vars or different tool versions in
development/staging/production environments. See
[Profiles](/profiles) for more on how
to use this feature.

### `RTX_USE_VERSIONS_HOST`

Default: `true`

Set to "false" to disable using [rtx-versions](https://rtx-versions.jdx.dev) as
a quick way for rtx to query for new versions. This host regularly grabs all the
latest versions of core and community plugins. It's faster than running a plugin's
`list-all` command and gets around GitHub rate limiting problems when using it.

See [FAQ](/faq#new-version-of-a-tool-is-not-available) for more information.

### `RTX_${PLUGIN}_VERSION`

Set the version for a runtime. For example, `RTX_NODE_VERSION=20` will use <node@20.x> regardless
of what is set in `.tool-versions`/`.rtx.toml`.

### `RTX_LEGACY_VERSION_FILE=1`

Plugins can read the versions files used by other version managers (if enabled by the plugin)
for example, `.nvmrc` in the case of node's nvm. See [legacy version files](#legacy-version-files) for more
information.

Set to "0" to disable legacy version file parsing.

### `RTX_LEGACY_VERSION_FILE_DISABLE_TOOLS=node,python`

Disable legacy version file parsing for specific tools. Separate with `,`.

### `RTX_USE_TOML=0`

Set to `1` to default to using `.rtx.toml` in `rtx local` instead of `.tool-versions` for
configuration.

For now this is not used by `rtx use` which will only use `.rtx.toml` unless `--path` is specified.

### `RTX_TRUSTED_CONFIG_PATHS`

This is a list of paths that rtx will automatically mark as
trusted. They can be separated with `:`.

### `RTX_LOG_LEVEL=trace|debug|info|warn|error`

These change the verbosity of rtx.

You can also use `RTX_DEBUG=1`, `RTX_TRACE=1`, and `RTX_QUIET=1` as well as
`--log-level=trace|debug|info|warn|error`.

### `RTX_LOG_FILE=~/rtx.log`

Output logs to a file.

### `RTX_LOG_FILE_LEVEL=trace|debug|info|warn|error`

Same as `RTX_LOG_LEVEL` but for the log _file_ output level. This is useful if you want
to store the logs but not have them litter your display.

### `RTX_ALWAYS_KEEP_DOWNLOAD=1`

Set to "1" to always keep the downloaded archive. By default it is deleted after install.

### `RTX_ALWAYS_KEEP_INSTALL=1`

Set to "1" to always keep the install directory. By default it is deleted on failure.

### `RTX_VERBOSE=1`

This shows the installation output during `rtx install` and `rtx plugin install`.
This should likely be merged so it behaves the same as `RTX_DEBUG=1` and we don't have
2 configuration for the same thing, but for now it is its own config.

Equivalent to `RTX_LOG_LEVEL=debug`.

### `RTX_QUIET=1`

Equivalent to `RTX_LOG_LEVEL=warn`.

### `RTX_ASDF_COMPAT=1`

Only output `.tool-versions` files in `rtx local|global` which will be usable by asdf.
This disables rtx functionality that would otherwise make these files incompatible with asdf.

### `RTX_JOBS=1`

Set the number plugins or runtimes to install in parallel. The default is `4`.

### `RTX_RAW=1`

Set to "1" to directly pipe plugin scripts to stdin/stdout/stderr. By default stdin is disabled
because when installing a bunch of plugins in parallel you won't see the prompt. Use this if a
plugin accepts input or otherwise does not seem to be installing correctly.

Sets `RTX_JOBS=1` because only 1 plugin script can be executed at a time.

### `RTX_SHORTHANDS_FILE=~/.config/rtx/shorthands.toml`

Use a custom file for the shorthand aliases. This is useful if you want to share plugins within
an organization.

Shorthands make it so when a user runs something like `rtx install elixir` rtx will
automatically install the [asdf-elixir](https://github.com/asdf-vm/asdf-elixir) plugin. By
default, it uses the shorthands in
[`src/default_shorthands.rs`](https://github.com/jdx/rtx/blob/main/src/default_shorthands.rs).

The file should be in this toml format:

```toml
elixir = "https://github.com/my-org/rtx-elixir.git"
node = "https://github.com/my-org/rtx-node.git"
```

### `RTX_DISABLE_DEFAULT_SHORTHANDS=1`

Disables the shorthand aliases for installing plugins. You will have to specify full URLs when
installing plugins, e.g.: `rtx plugin install node https://github.com/asdf-vm/asdf-node.git`

### `RTX_DISABLE_TOOLS=python,node`

Disables the specified tools. Separate with `,`. Generally used for core plugins but works with
all.

### `RTX_YES=1`

This will automatically answer yes or no to prompts. This is useful for scripting.

### `RTX_NOT_FOUND_AUTO_INSTALL=true`

Set to false to disable the "command not found" handler to autoinstall missing tool versions.

### `RTX_TASK_OUTPUT=prefix`

This controls the output of `rtx run`. It can be one of:

- `prefix` - (default if jobs > 1) print by line with the prefix of the task name
- `interleave` - (default if jobs == 1) display stdout/stderr as it comes in

### `RTX_EXPERIMENTAL=true`

Enables experimental features.

### `RTX_ALL_COMPILE=1`

Default: false unless running NixOS or Alpine (let me know if others should be added)

Do not use precompiled binaries for all languages. Useful if running on a Linux distribution
like Alpine that does not use glibc and therefore likely won't be able to run precompiled binaries.

Note that this needs to be setup for each language. File a ticket if you notice a language that is not
working with this config.

### `RTX_FISH_AUTO_ACTIVATE=1`

Configures the vendor_conf.d script for fish shell to automatically activate.
This file is automatically used in homebrew and potentially other installs to
automatically activate rtx without configuring.

Defaults to enabled, set to "0" to disable.
