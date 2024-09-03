## `mise local [OPTIONS] [TOOL@VERSION]...`

```text
Sets/gets tool version in local .tool-versions or .mise.toml

Use this to set a tool's version when within a directory
Use `mise global` to set a tool version globally
This uses `.tool-version` by default unless there is a `.mise.toml` file or if `MISE_USE_TOML`
is set. A future v2 release of mise will default to using `.mise.toml`.

Usage: local [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to add to .tool-versions/.mise.toml
          e.g.: node@20
          if this is a single tool with no version,
          the current value of .tool-versions/.mise.toml will be displayed

Options:
  -p, --parent
          Recurse up to find a .tool-versions file rather than using the current directory only
          by default this command will only set the tool in the current directory
          ("$PWD/.tool-versions")

      --pin
          Save exact version to `.tool-versions`
          e.g.: `mise local --pin node@20` will save `node 20.0.0` to .tool-versions

      --fuzzy
          Save fuzzy version to `.tool-versions` e.g.: `mise local --fuzzy node@20` will save `node
          20` to .tool-versions This is the default behavior unless MISE_ASDF_COMPAT=1

      --remove <PLUGIN>
          Remove the plugin(s) from .tool-versions

      --path
          Get the path of the config file

Examples:
    # set the current version of node to 20.x for the current directory
    # will use a precise version (e.g.: 20.0.0) in .tool-versions file
    $ mise local node@20

    # set node to 20.x for the current project (recurses up to find .tool-versions)
    $ mise local -p node@20

    # set the current version of node to 20.x for the current directory
    # will use a fuzzy version (e.g.: 20) in .tool-versions file
    $ mise local --fuzzy node@20

    # removes node from .tool-versions
    $ mise local --remove=node

    # show the current version of node in .tool-versions
    $ mise local node
    20.0.0
```
