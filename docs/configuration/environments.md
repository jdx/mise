# Config Environments

It's possible to have separate `mise.toml` files in the same directory for different
environments like `development` and `production`. To enable, either set the `-E,--env` option or `MISE_ENV` environment
variable to an environment like `development` or `production`. mise will then look for a `mise.{MISE_ENV}.toml` file
in the current directory, parent directories and the `MISE_CONFIG_DIR` directory.

mise will also look for "local" files like `mise.local.toml` and `mise.{MISE_ENV}.local.toml`
in the current directory and parent directories.
These are intended to not be committed to version control.
(Add `mise.local.toml` and `mise.*.local.toml` to your `.gitignore` file.)

The priority of these files goes in this order (top overrides bottom):

- `mise.{MISE_ENV}.local.toml`
- `mise.local.toml`
- `mise.{MISE_ENV}.toml`
- `mise.toml`

If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, that will be used instead of all of this.

You can also use paths like `mise/config.{MISE_ENV}.toml` or `.config/mise.{MISE_ENV}.toml` Those rules
follow the order in [Configuration](/configuration).

Use `mise config` to see which files are being used.

The rules around which file is written are different because we ultimately need to choose one. See
the docs for [`mise use`](/cli/use.html) for more information.

Multiple environments can be specified, e.g. `MISE_ENV=ci,test` with the last one taking precedence.
