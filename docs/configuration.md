# Configuration

## `mise.toml`

`mise.toml` is the config file for mise. It can use any of the following file paths (in order of precedence, top is highest):

- `.mise.local.toml` - used for local config, this should not be committed to source control
- `mise.local.toml`
- `.mise.$MISE_ENV.toml`
- `mise.$MISE_ENV.toml` - used for environment-specific config such as `mise.staging.toml`
- `.mise.toml`
- `.mise/config.toml`
- `mise.toml`
- `mise/config.toml`
- `.config/mise.toml` - use this in order to group config files into a common directory
- `.config/mise/config.toml`

See [Profiles](/profiles) for more information about `.mise.$MISE_ENV.toml` files.
These files recurse upwards, so if you have a `~/src/work/myproj/.mise.toml` file, what is defined
there will override anything set in
`~/src/work/.mise.toml` or `~/.config/mise.toml`. The config contents are merged together.

:::tip
Run `mise config` to see what files mise has loaded along with their precedence.
:::

Here is what an `.mise.toml` looks like:

```toml
[env]
# supports arbitrary env vars so mise can be used like direnv/dotenv
NODE_ENV = 'production'

[tools]
# specify single or multiple versions
terraform = '1.0.0'
erlang = ['23.3', '24.0']

# supports everything you can do with .tool-versions currently
node = ['16', 'prefix:20', 'ref:master', 'path:~/.nodes/14']

[plugins]
# specify a custom repo url
# note this will only be used if the plugin does not already exist
python = 'https://github.com/asdf-community/asdf-python'

[alias.node] # project-local aliases
my_custom_node = '20'
```

`.mise.toml` files are hierarchical. The configuration in a file in the current directory will
override conflicting configuration in parent directories. For example, if `~/src/myproj/.mise.toml`
defines the following:

```toml
[tools]
node = '20'
python = '3.10'
```

And `~/src/myproj/backend/.mise.toml` defines:

```toml
[tools]
node = '18'
ruby = '3.1'
```

Then when inside of `~/src/myproj/backend`, `node` will be `18`, `python` will be `3.10`, and `ruby`
will be `3.1`. You can check the active versions with `mise ls --current`.

You can also have environment specific config files like `.mise.production.toml`, see
[Profiles](/profiles) for more details.

### `[env]` - Arbitrary Environment Variables

See [environments](/environments).

### `[plugins]` - Specify Custom Plugin Repository URLs

Use `[plugins]` to add/modify plugin shortnames. Note that this will only modify
_new_ plugin installations. Existing plugins can use any URL.

```toml
[plugins]
elixir = "https://github.com/my-org/mise-elixir.git"
node = "https://github.com/my-org/mise-node.git#DEADBEEF" # supports specific gitref
```

If you simply want to install a plugin from a specific URL once, it's better to use
`mise plugin install plugin <GIT_URL>`. Add this section to `.mise.toml` if you want
to share the plugin location/revision with other developers in your project.

This is similar
to [`MISE_SHORTHANDS`](https://github.com/jdx/mise#mise_shorthands_fileconfigmiseshorthandstoml)
but doesn't require a separate file.

### `[aliases]` - Tool version aliases

The following makes `mise install node@my_custom_node` install node-20.x
this can also be specified in a [plugin](/dev-tools/aliases.md).
note adding an alias will also add a symlink, in this case:

```sh
~/.local/share/mise/installs/node/20 -> ./20.x.x
```

```toml
my_custom_node = '20'
```

## Global config: `~/.config/mise/config.toml`

mise can be configured in `~/.config/mise/config.toml`. It's like local `.mise.toml` files except
that
it is used for all directories.

```toml
[tools]
# global tool versions go here
# you can set these with `mise use -g`
node = 'lts'
python = ['3.10', '3.11']

[settings]
# plugins can read the versions files used by other version managers (if enabled by the plugin)
# for example, .nvmrc in the case of node's nvm
legacy_version_file = true                     # enabled by default (unlike asdf)
legacy_version_file_disable_tools = ['python'] # disable for specific tools

# configure `mise install` to always keep the downloaded archive
always_keep_download = false        # deleted after install by default
always_keep_install = false         # deleted on failure by default

# configure how frequently (in minutes) to fetch updated plugin repository changes
# this is updated whenever a new runtime is installed
# (note: this isn't currently implemented but there are plans to add it: https://github.com/jdx/mise/issues/128)
plugin_autoupdate_last_check_duration = '1 week' # set to 0 to disable updates

# config files with these prefixes will be trusted by default
trusted_config_paths = [
    '~/work/my-trusted-projects',
]

verbose = false     # set to true to see full installation output, see `MISE_VERBOSE`
asdf_compat = false # set to true to ensure .tool-versions will be compatible with asdf, see `MISE_ASDF_COMPAT`
http_timeout = 30   # set the timeout for http requests in seconds, see `MISE_HTTP_TIMEOUT`
jobs = 4            # number of plugins or runtimes to install in parallel. The default is `4`.
raw = false         # set to true to directly pipe plugins to stdin/stdout/stderr
yes = false         # set to true to automatically answer yes to all prompts

not_found_auto_install = true # see MISE_NOT_FOUND_AUTO_INSTALL
task_output = "prefix" # see Tasks Runner for more information
paranoid = false       # see MISE_PARANOID

shorthands_file = '~/.config/mise/shorthands.toml' # path to the shorthands file, see `MISE_SHORTHANDS_FILE`
disable_default_shorthands = false # disable the default shorthands, see `MISE_DISABLE_DEFAULT_SHORTHANDS`
disable_tools = ['node']           # disable specific tools, generally used to turn off core tools

env_file = '.env' # load env vars from a dotenv file, see `MISE_ENV_FILE`

experimental = true # enable experimental features

# configure messages displayed when entering directories with config files
status = { missing_tools = "if_other_versions_installed", show_env = false, show_tools = false }
```

## System config: `/etc/mise/config.toml`

Similar to `~/.config/mise/config.toml` but for all users on the system. This is useful for
setting defaults for all users.

## `.tool-versions`

The `.tool-versions` file is asdf's config file and it can be used in mise just like `.mise.toml`.
It isn't as flexible so it's recommended to use `.mise.toml` instead. It can be useful if you
already have a lot of `.tool-versions` files or work on a team that uses asdf.

Here is an example with all the supported syntax:

```text
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

See [the asdf docs](https://asdf-vm.com/manage/configuration.html#tool-versions) for more info on
this file format.

## Scopes

Both `.mise.toml` and `.tool-versions` support "scopes" which modify the behavior of the version:

- `ref:<SHA>` - compile from a vcs (usually git) ref
- `prefix:<PREFIX>` - use the latest version that matches the prefix. Useful for Go since `1.20`
  would only match `1.20` exactly but `prefix:1.20` will match `1.20.1` and `1.20.2` etc.
- `path:<PATH>` - use a custom compiled version at the given path. One use-case is to re-use
  Homebrew tools (e.g.: `path:/opt/homebrew/opt/node@20`).
- `sub-<PARTIAL_VERSION>:<ORIG_VERSION>` - subtracts PARTIAL_VERSION from ORIG_VERSION. This can
  be used to express something like "2 versions behind lts" such as `sub-2:lts`. Or 1 minor
  version behind the latest version: `sub-0.1:latest`.

## Legacy version files

mise supports "legacy version files" just like asdf. They're language-specific files
like `.node-version`
and `.python-version`. These are ideal for setting the runtime version of a project without forcing
other developers to use a specific tool like mise/asdf.

They support aliases, which means you can have an `.nvmrc` file with `lts/hydrogen` and it will work
in mise and nvm. Here are some of the supported legacy version files:

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

In mise these are enabled by default. You can disable them
with `mise settings set legacy_version_file false`.
There is a performance cost to having these when they're parsed as it's performed by the plugin in
`bin/parse-version-file`. However these are [cached](/cache-behavior) so it's not a huge deal.
You may not even notice.

::: info
asdf calls these "legacy version files" so we do too. I think this is a bad name since it implies
that they shouldn't be used—which is definitely not the case IMO. I prefer the term "idiomatic"
version files since they're version files not specific to asdf/mise and can be used by other tools.
(`.nvmrc` being a notable exception, which is tied to a specific tool.)
:::

## Settings

See [Settings](/settings) for the full list of settings.

## Tasks

See [Tasks](/tasks/) for the full list of configuration options.

## Environment variables

mise can also be configured via environment variables. The following options are available:

### `MISE_DATA_DIR`

Default: `~/.local/share/mise` or `$XDG_DATA_HOME/mise`

This is the directory where mise stores plugins and tool installs. These are not supposed to be
shared
across machines.

### `MISE_CACHE_DIR`

Default (Linux): `~/.cache/mise` or `$XDG_CACHE_HOME/mise`
Default (macOS): `~/Library/Caches/mise` or `$XDG_CACHE_HOME/mise`

This is the directory where mise stores internal cache. This is not supposed to be shared
across machines. It may be deleted at any time mise is not running.

### `MISE_TMP_DIR`

Default: [`std::env::temp_dir()`](https://doc.rust-lang.org/std/env/fn.temp_dir.html) implementation
in rust

This is used for temporary storage such as when installing tools.

### `MISE_SYSTEM_DIR`

Default: `/etc/mise`

This is the directory where mise stores system-wide configuration.

### `MISE_GLOBAL_CONFIG_FILE`

Default: `$MISE_CONFIG_DIR/config.toml` (Usually ~/.config/mise/config.toml)

This is the path to the config file.

### `MISE_DEFAULT_TOOL_VERSIONS_FILENAME`

Set to something other than ".tool-versions" to have mise look for `.tool-versions` files but with
a different name.

### `MISE_DEFAULT_CONFIG_FILENAME`

Set to something other than `.mise.toml` to have mise look for `.mise.toml` config files with a
different name.

### `MISE_ENV`

Enables environment-specific config files such as `.mise.development.toml`.
Use this for different env vars or different tool versions in
development/staging/production environments. See
[Profiles](/profiles) for more on how
to use this feature.

### `MISE_ENV_FILE`

Set to a filename to read from env from a dotenv file. e.g.: `MISE_ENV_FILE=.env`.
Uses [dotenvy](https://crates.io/crates/dotenvy) under the hood.

### `MISE_${PLUGIN}_VERSION`

Set the version for a runtime. For example, `MISE_NODE_VERSION=20` will use <node@20.x> regardless
of what is set in `.tool-versions`/`.mise.toml`.

### `MISE_USE_TOML=0`

Set to `1` to default to using `.mise.toml` in `mise local` instead of `.tool-versions` for
configuration.

For now this is not used by `mise use` which will only use `.mise.toml` unless `--path` is
specified.

### `MISE_TRUSTED_CONFIG_PATHS`

This is a list of paths that mise will automatically mark as
trusted. They can be separated with `:`.

### `MISE_LOG_LEVEL=trace|debug|info|warn|error`

These change the verbosity of mise.

You can also use `MISE_DEBUG=1`, `MISE_TRACE=1`, and `MISE_QUIET=1` as well as
`--log-level=trace|debug|info|warn|error`.

### `MISE_LOG_FILE=~/mise.log`

Output logs to a file.

### `MISE_LOG_FILE_LEVEL=trace|debug|info|warn|error`

Same as `MISE_LOG_LEVEL` but for the log _file_ output level. This is useful if you want
to store the logs but not have them litter your display.

### `MISE_QUIET=1`

Equivalent to `MISE_LOG_LEVEL=warn`.

### `MISE_HTTP_TIMEOUT`

Set the timeout for http requests in seconds. The default is `30`.

### `MISE_RAW=1`

Set to "1" to directly pipe plugin scripts to stdin/stdout/stderr. By default stdin is disabled
because when installing a bunch of plugins in parallel you won't see the prompt. Use this if a
plugin accepts input or otherwise does not seem to be installing correctly.

Sets `MISE_JOBS=1` because only 1 plugin script can be executed at a time.

### `MISE_FISH_AUTO_ACTIVATE=1`

Configures the vendor_conf.d script for fish shell to automatically activate.
This file is automatically used in homebrew and potentially other installs to
automatically activate mise without configuring.

Defaults to enabled, set to "0" to disable.
