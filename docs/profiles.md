# Profiles

It's possible to have separate `mise.toml` files in the same directory for different
environments like `development` and `production`. To enable, either set the `-P,--profile` option or `MISE_PROFILE` environment
variable to an environment like `development` or `production`. mise will then look for a `mise.{MISE_PROFILE}.toml` file
in the current directory, parent directories and the `MISE_CONFIG_DIR` directory.

mise will also look for "local" files like `mise.local.toml` and `mise.{MISE_PROFILE}.local.toml`
in the current directory and parent directories.
These are intended to not be committed to version control.
(Add `mise.local.toml` and `mise.*.local.toml` to your `.gitignore` file.)

The priority of these files goes in this order (top overrides bottom):

- `mise.{MISE_PROFILE}.local.toml`
- `mise.local.toml`
- `mise.{MISE_PROFILE}.toml`
- `mise.toml`

You can also use paths like `mise/config.{MISE_PROFILE}.toml` or `.config/mise.{MISE_PROFILE}.toml` Those rules
follow the order in [Configuration](./configuration.md).

Use `mise config` to see which files are being used.

::: warning
Note that currently modifying `MISE_DEFAULT_CONFIG_FILENAME` to something other than `mise.toml`
will not work with this feature. For now, it will disable it entirely. This may change in the
future.
:::
