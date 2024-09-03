## `mise global [OPTIONS] [TOOL@VERSION]...`

```text
Sets/gets the global tool version(s)

Displays the contents of global config after writing.
The file is `$HOME/.config/mise/config.toml` by default. It can be changed with `$MISE_GLOBAL_CONFIG_FILE`.
If `$MISE_GLOBAL_CONFIG_FILE` is set to anything that ends in `.toml`, it will be parsed as `.mise.toml`.
Otherwise, it will be parsed as a `.tool-versions` file.

Use MISE_ASDF_COMPAT=1 to default the global config to ~/.tool-versions

Use `mise local` to set a tool version locally in the current directory.

Usage: global [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to add to .tool-versions
          e.g.: node@20
          If this is a single tool with no version, the current value of the global
          .tool-versions will be displayed

Options:
      --pin
          Save exact version to `~/.tool-versions`
          e.g.: `mise global --pin node@20` will save `node 20.0.0` to ~/.tool-versions

      --fuzzy
          Save fuzzy version to `~/.tool-versions`
          e.g.: `mise global --fuzzy node@20` will save `node 20` to ~/.tool-versions
          this is the default behavior unless MISE_ASDF_COMPAT=1

      --remove <PLUGIN>
          Remove the plugin(s) from ~/.tool-versions

      --path
          Get the path of the global config file

Examples:
    # set the current version of node to 20.x
    # will use a fuzzy version (e.g.: 20) in .tool-versions file
    $ mise global --fuzzy node@20

    # set the current version of node to 20.x
    # will use a precise version (e.g.: 20.0.0) in .tool-versions file
    $ mise global --pin node@20

    # show the current version of node in ~/.tool-versions
    $ mise global node
    20.0.0
```
