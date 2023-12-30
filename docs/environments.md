# Environments

Use rtx to specify environment variables used for different projects. Create a `.rtx.toml` file
in the root of your project directory:

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

See [templates](/templates) for setting config dynamically.
