## `mise config set [OPTIONS] <KEY> <VALUE>`

```text
Display the value of a setting in a mise.toml file

Usage: config set [OPTIONS] <KEY> <VALUE>

Arguments:
  <KEY>
          The path of the config to display

  <VALUE>
          The value to set the key to

Options:
  -f, --file <FILE>
          The path to the mise.toml file to edit
          
          If not provided, the nearest mise.toml file will be used

  -t, --type <TYPE>
          [default: string]
          [possible values: string, integer, float, bool]

Examples:

    $ mise config set tools.python 3.12
    $ mise config set settings.always_keep_download true
    $ mise config set env.TEST_ENV_VAR ABC
```
