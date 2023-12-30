# Config Environments

It's possible to have separate `.rtx.toml` files in the same directory for different
environments like `development` and `production`. To enable, set `RTX_ENV` to an environment like
`development` or `production`. rtx will then look for a `.rtx.{RTX_ENV}.toml` file in the current directory.

rtx will also look for "local" files like `.rtx.local.toml` and `.rtx.{RTX_ENV}.local.toml` in
the current directory. These are intended to not be committed to version control.
(Add `.rtx.local.toml` and `.rtx.*.local.toml` to your `.gitignore` file.)

The priority of these files goes in this order (bottom overrides top):

- `.config/rtx/config.toml`
- `.rtx/config.toml`
- `.rtx.toml`
- `.config/rtx/config.local.toml`
- `.rtx/config.local.toml`
- `.rtx.local.toml`
- `.config/rtx/config.{RTX_ENV}.toml`
- `.rtx/config.{RTX_ENV}.toml`
- `.rtx.{RTX_ENV}.toml`
- `.config/rtx/config.{RTX_ENV}.local.toml`
- `.rtx/config.{RTX_ENV}.local.toml`
- `.rtx.{RTX_ENV}.local.toml`

Use `rtx doctor` to see which files are being used.

> [!IMPORTANT]
>
> Note that currently modifying `RTX_DEFAULT_CONFIG_FILENAME` to something other than `.rtx.toml`
> will not work with this feature. For now, it will disable it entirely. This may change in the
> future.
