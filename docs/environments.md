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

## `mise.file`

In `.mise.toml`: `mise.file` can be used to specify a [dotenv](https://dotenv.org) file to load. It can be a string or array and uses relative or absolute paths:

```toml
[env]
mise.file = '.env'
```

_This uses [dotenvy](https://crates.io/crates/dotenvy) under the hood._

Or set [`MISE_ENV_FILE=.env`](/configuration#mise-env-file) to automatically load dotenv files in any
directory.

## `mise.path`

`PATH` is treated specially, it needs to be defined as a string/array in `mise.path`:

```toml
[env]
mise.path = [
    # adds an absolute path
    "~/.local/share/bin",
    # adds a path relative to the .mise.toml, not PWD
    "./node_modules/.bin",
]
```

## Templates

Environment variable values can be templates, see [Templates](/templates) for details.

```toml
[env]
LD_LIBRARY_PATH = "/some/path:{{env.LD_LIBRARY_PATH}}"
```
