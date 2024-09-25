## `mise toml set <KEY> <VALUE> [FILE]`

```text
Display the value of a setting in a mise.toml file

Usage: toml set <KEY> <VALUE> [FILE]

Arguments:
  <KEY>
          The path of the config to display

  <VALUE>
          The value to set the key to

  [FILE]
          The path to the mise.toml file to edit
          
          If not provided, the nearest mise.toml file will be used

Examples:

    $ mise toml set tools.python 3.12
    $ mise toml set settings.always_keep_download true
    $ mise toml set env.TEST_ENV_VAR ABC
```
