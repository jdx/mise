# Environments

Use mise to specify environment variables used for different projects. Create a `.mise.toml` file
in the root of your project directory:

```toml
[env]
NODE_ENV = 'production'
```

To clear an env var, set it to `false`:

```toml
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
NODE_ENV  development  .mise.toml
$ mise unset NODE_ENV
```

## `env._` directives

`env._.*` define special behavior for setting environment variables. (e.g.: reading env vars
from a file). Since nested environment variables do not make sense,
we make use of this fact by creating a key named "_" which is a
TOML table for the configuration of these directives.

### `env._.file`

In `.mise.toml`: `env._.file` can be used to specify a [dotenv](https://dotenv.org) file to load.
It can be a string or array and uses relative or absolute paths:

```toml
[env]
_.file = '.env'
```

_This uses [dotenvy](https://crates.io/crates/dotenvy) under the hood. If you have problems with
the way `env._.file` works, you will likely need to post an issue there,
not to mise since there is not much mise can do about the way that crate works._

Or set [`MISE_ENV_FILE=.env`](/configuration#mise-env-file) to automatically load dotenv files in any
directory.

### `env._.path`

`PATH` is treated specially, it needs to be defined as a string/array in `mise.path`:

```toml
[env]
_.path = [
    # adds an absolute path
    "~/.local/share/bin",
    # adds a path relative to the .mise.toml, not PWD
    "./node_modules/.bin",
]
```

### `env._.source` [<Badge type="warning" text="coming soon" />](https://github.com/jdx/mise/issues/1447)

_Follow [#1447](https://github.com/jdx/mise/issues/1447) to see when this is available._

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

The shebang will be ignored. It would be possible to use different types of scripts,
or binaries that are not scripts at all. See [#1448](https://github.com/jdx/mise/issues/1448)
for information.
:::

## Multiple `env._` Directives <Badge type="warning" text="coming soon" />

_Follow [#1449](https://github.com/jdx/mise/issues/1449) to see when this is available._

It may be necessary to use multiple `env._` directives but TOML syntax won't allow 2 keys
in a table like that:

```toml
[env]
_.source = "./script_1.sh"
_.source = "./script_2.sh" # invalid // [!code error]
```

To support that, you can optionally make `[env]` an array instead by using TOML's `[[env]]` syntax:

```toml
[[env]]
_.source = "./script_1.sh"
[[env]]
_.source = "./script_2.sh"
```

## Templates

Environment variable values can be templates, see [Templates](/templates) for details.

```toml
[env]
LD_LIBRARY_PATH = "/some/path:{{env.LD_LIBRARY_PATH}}"
```

## Using env vars in other env vars <Badge type="warning" text="coming soon" />

_Follow [#1262](https://github.com/jdx/mise/issues/1262) to see when this is available._

You can use the value of an environment variable in later env vars:

```toml
[env]
MY_PROJ_LIB = "{{config_root}}/lib"
LD_LIBRARY_PATH = "/some/path:{{env.MY_PROJ_LIB}}"
```

Of course the ordering matters when doing this.
