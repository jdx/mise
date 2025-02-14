# Environments

> Like [direnv](https://github.com/direnv/direnv) it
> manages _environment variables_ for
> different project directories.

Use mise to specify environment variables used for different projects. Create a `mise.toml` file
in the root of your project directory:

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
$ mise set NODE_ENV=development
$ mise set NODE_ENV
development
$ mise set
key       value        source
NODE_ENV  development  mise.toml
$ mise unset NODE_ENV
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
_.file = { path = [".env.json"], redact = true }
```

## `env._` directives

`env._.*` define special behavior for setting environment variables. (e.g.: reading env vars
from a file). Since nested environment variables do not make sense,
we make use of this fact by creating a key named "\_" which is a
TOML table for the configuration of these directives.

### `env._.file`

In `mise.toml`: `env._.file` can be used to specify a [dotenv](https://dotenv.org) file to load.
It can be a string or array and uses relative or absolute paths:

```toml
[env]
_.file = '.env'
```

::: info
This uses [dotenvy](https://crates.io/crates/dotenvy) under the hood. If you have problems with
the way `env._.file` works, you will likely need to post an issue there,
not to mise since there is not much mise can do about the way that crate works.
:::

Or set [`MISE_ENV_FILE=.env`](/configuration#mise-env-file) to automatically load dotenv files in any
directory.

You can also use json or yaml files:

```toml
[env]
_.file = '.env.json'
```

See [secrets](/environments/secrets) for ways to read encrypted files with `env._.file`.

### `env._.path`

`PATH` is treated specially. It needs to be defined as a string/array in `mise.path`:

```toml
[env]
_.path = [
    # adds an absolute path
    "~/.local/share/bin",
    # adds paths relative to directory in which this file was found (see below for details), not PWD
    "{{config_root}}/node_modules/.bin",
    # adds paths relative to the exact file that this is found in (not PWD)
    "tools/bin",
]
```

Adding a relative path like `tools/bin` or `./tools/bin` is similar to adding a path rooted at <span v-pre>`{{config_root}}`</span>, but behaves differently if your config file is nested in a subdirectory like `/path/to/project/.config/mise/config.toml`. Including `tools/bin` will add the path `/path/to/project/.config/mise/tools/bin`, whereas including <span v-pre>`{{config_root}}/tools/bin`</span> will add the path `/path/to/project/tools/bin`.

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
