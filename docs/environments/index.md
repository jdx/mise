# Environments

> Like [direnv](https://github.com/direnv/direnv) it
> manages _environment variables_ for
> different project directories.

Use mise to specify environment variables used for different projects.

To get started, create a `mise.toml` file in the root of your project directory:

```toml [mise.toml]
[env]
NODE_ENV = 'production'
```

To clear an env var, set it to `false`:

```toml [mise.toml]
[env]
NODE_ENV = false # unset a previously set NODE_ENV
```

You can also use the CLI to get/set env vars:

```sh
mise set NODE_ENV=development
# mise set NODE_ENV
# development

mise set
# key       value        source
# NODE_ENV  development  mise.toml

cat mise.toml
# [env]
# NODE_ENV = 'development'

mise unset NODE_ENV
```

Additionally, the [mise env [--json] [--dotenv]](/cli/env.html) command can be used to export the environment variables in various formats (including `PATH` and environment variables set by tools or plugins).

## Using environment variables

Environment variables are available when using [`mise x|exec`](/cli/exec.html), or with [`mise r|run`](/cli/run.html) (i.e. with [tasks](/tasks/)):

```shell
mise set MY_VAR=123
mise exec -- echo $MY_VAR
# 123
```

You can of course combine them with [tools](/dev-tools/):

```sh
mise use node@22
mise set MY_VAR=123
cat mise.toml
# [tools]
# node = '22'
# [env]
# MY_VAR = '123'
mise exec -- node --eval 'console.log(process.env.MY_VAR)'
# 123
```

If [mise is activated](/getting-started.html#activate-mise), it will automatically set environment variables in the current shell session when you `cd` into a directory.

```shell
cd /path/to/project
mise set NODE_ENV=production
cat mise.toml
# [env]
# NODE_ENV = 'production'

echo $NODE_ENV
# production
```

If you are using [`shims`](/dev-tools/shims.html), the environment variables will be available when using the shim:

```shell
mise set NODE_ENV=production
mise use node@22
# using the absolute path for the example
~/.local/share/mise/shims/node --eval 'console.log(process.env.NODE_ENV)'
```

Finally, you can also use [`mise en`](/cli/en.html) to start a new shell session with the environment variables set.

```shell
mise set FOO=bar
mise en
> echo $FOO
# bar
```

## Environment in tasks

It is also possible to define environment inside a task

```toml [mise.toml]
[tasks.print]
run = "echo $MY_VAR"
env = { _.file = '/path/to/file.env', "MY_VAR" = "my variable" }
```

## Lazy eval

Environment variables typically are resolved before tools—that way you can configure tool installation
with environment variables. However, sometimes you want to access environment variables produced by
tools. To do that, turn the value into a map with `tools = true`:

```toml
[env]
MY_VAR = { value = "tools path: {{env.PATH}}", tools = true }
_.path = { path = ["{{env.GEM_HOME}}/bin"], tools = true } # directives may also set tools = true
```

## Redactions

Variables can be redacted from the output by setting `redact = true`:

```toml
[env]
SECRET = { value = "my_secret", redact = true }
_.file = { path = ".env.json", redact = true }
```

You can also use the `redactions` array to mark multiple environment variables as sensitive:

```toml
redactions = ["SECRET_*", "*_TOKEN", "PASSWORD"]
[env]
SECRET_KEY = "sensitive_value"
API_TOKEN = "token_123"
PASSWORD = "my_password"
```

### Viewing Redacted Environment Variables

The `mise env` command provides flags to work with redacted variables:

```bash
# Show only redacted environment variables
mise env --redacted

# Show only values (useful for piping)
mise env --values

# Show only values of redacted variables
mise env --redacted --values
```

::: danger
Because mise may output sensitive values that could show up in CI logs you'll need to be configure your CI setup
to know which values are sensitive.

For example, when using GitHub Actions, you should use `::add-mask::` to prevent secrets from appearing in logs:

```bash
# In a GitHub Actions workflow
for value in $(mise env --redacted --values); do
  echo "::add-mask::$value"
done
```

Note: If you're using [mise-action](https://github.com/jdx/mise-action), it will automatically redact values marked with `redact = true` or matching patterns in the `redactions` array.
:::

## Required Variables

You can mark environment variables as required by setting `required = true`. This ensures that the variable is defined either before mise runs or in a later config file (like `mise.local.toml`):

```toml
[env]
DATABASE_URL = { required = true }
API_KEY = { required = true }
```

You can also provide help text to guide users on how to set the variable:

```toml
[env]
DATABASE_URL = { required = "Set DATABASE_URL to your PostgreSQL connection string (e.g., postgres://user:pass@localhost/dbname)" }
API_KEY = { required = "Get your API key from https://example.com/api-keys" }
AWS_REGION = { required = "Set to your AWS region (e.g., us-east-1, eu-west-1)" }
```

When a required variable is missing, mise will show the help text in the error message to assist users.

### Required Variable Behavior

When a variable is marked as `required = true`, mise validates that it is defined through one of these sources:

1. **Pre-existing environment** - Variable was set before running mise
2. **Later config file** - Variable is defined in a config file processed after the one declaring it as required

```toml
# In mise.toml
[env]
DATABASE_URL = { required = true }
```

```toml
# In mise.local.toml (processed later)
[env]
DATABASE_URL = "postgres://prod.example.com/db"  # This satisfies the requirement
```

### Validation Behavior

- **Regular commands** (like `mise env`): Fail with clear error messages when required variables are missing
- **Shell activation** (`hook-env`): Warns about missing required variables but continues execution to avoid breaking shell setup

```bash
# This will fail if DATABASE_URL is not pre-defined or in a later config
$ mise env
Error: Required environment variable 'DATABASE_URL' is not defined...

# This will warn but continue (used by shell activation)
$ mise hook-env --shell bash
mise WARN Required environment variable 'DATABASE_URL' is not defined...
# Shell activation continues successfully
```

### Use Cases

Required variables are useful for:

- **Database connections** - Ensure critical connection strings are explicitly set
- **API keys** - Require explicit configuration of sensitive credentials
- **Environment-specific settings** - Force explicit configuration per environment
- **Team collaboration** - Document which variables team members must configure

```toml
[env]
# API keys (must be set in environment or mise.local.toml)
STRIPE_API_KEY = { required = true }
SENTRY_DSN = { required = true }

# Database connection (must be set in environment or mise.local.toml)
DATABASE_URL = { required = true }

# Feature flags (must be explicitly configured)
ENABLE_BETA_FEATURES = { required = true }
```

## `config_root`

`config_root` is the canonical project root directory that mise uses when resolving relative paths inside configuration files. Generally, when you use relative paths in mise you're referring to this directory.

- When your config lives at nested paths like `.config/mise/config.toml` or `.mise/config.toml`, `config_root` points to the project directory that contains those files (for example, `/path/to/project`).
- When your config lives at the project root (for example, `mise.toml`), `config_root` is simply the current directory.
- Relative paths in environment directives are resolved against `config_root` so they behave consistently regardless of where the config file itself lives.

Here's some example config files and their `config_root`:

| Config File                                 | `config_root` |
| ------------------------------------------- | ------------- |
| `~/src/foo/.config/mise/conf.d/config.toml` | `~/src/foo`   |
| `~/src/foo/.config/mise/config.toml`        | `~/src/foo`   |
| `~/src/foo/.mise/config.toml`               | `~/src/foo`   |
| `~/src/foo/mise.toml`                       | `~/src/foo`   |

You can see the implementation in [config_root.rs](https://github.com/jdx/mise/blob/main/src/config/config_file/config_root.rs).

Examples:

```toml
[env]
# These are equivalent and both resolve against the project root
_.path = ["tools/bin", "{{config_root}}/tools/bin"]

# Likewise, a relative source path resolves against the project root
_.source = "scripts/env.sh"          # == "{{config_root}}/scripts/env.sh"
```

## `env._` directives

`env._.*` define special behavior for setting environment variables. (e.g.: reading env vars
from a file). Since nested environment variables do not make sense,
we make use of this fact by creating a key named "\_" which is a
TOML table for the configuration of these directives.

### `env._.file`

In `mise.toml`: `env._.file` can be used to specify a [dotenv](https://dotenv.org) file to load.

```toml
[env]
_.file = '.env'
```

::: info
This uses [dotenvy](https://crates.io/crates/dotenvy) under the hood. If you have problems with
the way `env._.file` works, you will likely need to post an issue there,
not to mise since there is not much mise can do about the way that crate works.
:::

The `env._.file` directive supports:

- A single file as a string or an object
- Multiple files as an array of strings and objects
- Using relative or absolute paths
- Using `dotenv`, `json`, or `yaml` file formats
- The `redact` and `tools` options

```toml
[env]
_.file = '.env.yaml'
```

```toml
[env]
# Load env from the dotenv file after tools have defined environment variables
_.file = { path = ".env", tools = true }
```

```toml
[env]
_.file = [
    # Load env from the json file relative to this config file
    '.env.json',
    # Load env from the dotenv file at an absolute path
    '/User/bob/.env',
    # Load env from the yaml file relative to this config file and redacts the values
    { path = ".secrets.yaml", redact = true }
]
```

You can set [`MISE_ENV_FILE=.env`](/configuration#mise-env-file) to automatically load dotenv files in any
directory.

See [secrets](/environments/secrets/) for ways to read encrypted files with `env._.file`.

### `env._.path`

`PATH` is treated specially. Use `env._.path` to add extra directories to the `PATH`, making any executables in those directories available in the shell without needing to type the full path:

```toml
[env]
_.path = './bin'
```

The `env._.path` directive supports:

- A single path as a string or an object
- Multiple paths as an array of strings and objects
- Using relative or absolute paths
- The `tools` option

```toml
[env]
_.path = 'scripts'
```

```toml
[env]
# Define this path directory after tools have defined environment variables
_.path = { path = ["{{env.GEM_HOME}}/bin"], tools = true }
```

```toml
[env]
_.path = [
    # adds an absolute path
    "~/.local/share/bin",
    # adds a path relative to the project root (config_root)
    "{{config_root}}/node_modules/.bin",
    # adds a relative path (equivalent to "{{config_root}}/tools/bin")
    "tools/bin",
]
```

Relative paths like `tools/bin` or `./tools/bin` are resolved against <span v-pre>`{{config_root}}`</span>. For example, with a config file at `/path/to/project/.config/mise/config.toml`, `tools/bin` resolves to `/path/to/project/tools/bin`.

### `env._.source`

Source an external bash script and pull exported environment variables out of it:

```toml
[env]
_.source = "./script.sh"
```

::: info
This **must** be a script that runs in bash as if it were executed like this:

```sh
source ./script.sh
```

The shebang will be **ignored**. See [#1448](https://github.com/jdx/mise/issues/1448)
for a potential alternative that would work with binaries or other script languages.
:::

The `env._.source` directive supports:

- A single source as a string or an object
- Multiple sources as an array of strings and objects
- Using relative or absolute paths
- The `redact` and `tools` options

```toml
[env]
_.source = 'source.sh'
```

```toml
[env]
# Source this file after tools have defined environment variables
_.source = { path = "my/env.sh", tools = true }
```

```toml
[env]
_.source = [
    # Sources the file relative to the config root
    './scripts/base.sh',
    # Sources a file at an absolute path
    '/User/bob/env.sh',
    # Sources the file relative to the config root and redacts the values
    { path = ".secrets.sh", redact = true }
]
```

## Plugin-provided `env._` Directives

Plugins can provide their own `env._` directives. See [mise-env-sample](https://github.com/jdx/mise-env-sample) for an example of one.

## Multiple `env._` Directives

It may be necessary to use multiple `env._` directives, however TOML fails with this syntax
because it has 2 identical keys in a table:

```toml
[env]
_.source = "./script_1.sh"
_.source = "./script_2.sh" # invalid // [!code error]
```

For this use-case, you can optionally make `[env]` an array-of-tables instead by using `[[env]]` instead:

```toml
[[env]]
_.source = "./script_1.sh"
[[env]]
_.source = "./script_2.sh"
```

It works identically but you can have multiple tables.

## Templates

Environment variable values can be templates, see [Templates](/templates) for details.

```toml
[env]
LD_LIBRARY_PATH = "/some/path:{{env.LD_LIBRARY_PATH}}"
```

## Using env vars in other env vars

You can use the value of an environment variable in later env vars:

```toml
[env]
MY_PROJ_LIB = "{{config_root}}/lib"
LD_LIBRARY_PATH = "/some/path:{{env.MY_PROJ_LIB}}"
```

Of course the ordering matters when doing this.
