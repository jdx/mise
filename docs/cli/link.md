## `mise link [OPTIONS] <TOOL@VERSION> <PATH>`

**Aliases:** `ln`

```text
Symlinks a tool version into mise

Use this for adding installs either custom compiled outside
mise or built with a different tool.

Usage: link [OPTIONS] <TOOL@VERSION> <PATH>

Arguments:
  <TOOL@VERSION>
          Tool name and version to create a symlink for

  <PATH>
          The local path to the tool version
          e.g.: ~/.nvm/versions/node/v20.0.0

Options:
  -f, --force
          Overwrite an existing tool version if it exists

Examples:
    # build node-20.0.0 with node-build and link it into mise
    $ node-build 20.0.0 ~/.nodes/20.0.0
    $ mise link node@20.0.0 ~/.nodes/20.0.0

    # have mise use the python version provided by Homebrew
    $ brew install node
    $ mise link node@brew $(brew --prefix node)
    $ mise use node@brew
```
