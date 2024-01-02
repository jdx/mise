# Profiles

It's possible to have separate `.mise.toml` files in the same directory for different
environments like `development` and `production`. To enable, set `RTX_ENV` to an environment like
`development` or `production`. mise will then look for a `.mise.{RTX_ENV}.toml` file in the current directory.

mise will also look for "local" files like `.mise.local.toml` and `.mise.{RTX_ENV}.local.toml` in
the current directory. These are intended to not be committed to version control.
(Add `.mise.local.toml` and `.mise.*.local.toml` to your `.gitignore` file.)

The priority of these files goes in this order (bottom overrides top):

- `.config/mise/config.toml`
- `.mise/config.toml`
- `.mise.toml`
- `.config/mise/config.local.toml`
- `.mise/config.local.toml`
- `.mise.local.toml`
- `.config/mise/config.{RTX_ENV}.toml`
- `.mise/config.{RTX_ENV}.toml`
- `.mise.{RTX_ENV}.toml`
- `.config/mise/config.{RTX_ENV}.local.toml`
- `.mise/config.{RTX_ENV}.local.toml`
- `.mise.{RTX_ENV}.local.toml`

Use `mise doctor` to see which files are being used.

::: warning
Note that currently modifying `RTX_DEFAULT_CONFIG_FILENAME` to something other than `.mise.toml`
will not work with this feature. For now, it will disable it entirely. This may change in the
future.
:::
