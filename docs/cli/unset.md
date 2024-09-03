## `mise unset [OPTIONS] [KEYS]...`

```text
Remove environment variable(s) from the config file

By default this command modifies ".mise.toml" in the current directory.

Usage: unset [OPTIONS] [KEYS]...

Arguments:
  [KEYS]...
          Environment variable(s) to remove
          e.g.: NODE_ENV

Options:
  -f, --file <FILE>
          Specify a file to use instead of ".mise.toml"

  -g, --global
          Use the global config file
```
