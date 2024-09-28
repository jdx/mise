## `mise config get [OPTIONS] [KEY]`

```text
Display the value of a setting in a mise.toml file

Usage: config get [OPTIONS] [KEY]

Arguments:
  [KEY]
          The path of the config to display

Options:
  -f, --file <FILE>
          The path to the mise.toml file to edit
          
          If not provided, the nearest mise.toml file will be used

Examples:

    $ mise toml get tools.python
    3.12
```
