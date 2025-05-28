# `mise plugins install`

- **Usage**: `mise plugins install [FLAGS] [NEW_PLUGIN] [GIT_URL]`
- **Aliases**: `i`, `a`, `add`
- **Source code**: [`src/cli/plugins/install.rs`](https://github.com/jdx/mise/blob/main/src/cli/plugins/install.rs)

Install a plugin

note that mise automatically can install plugins when you install a tool
e.g.: `mise install node@20` will autoinstall the node plugin

This behavior can be modified in ~/.config/mise/config.toml

## Arguments

### `[NEW_PLUGIN]`

The name of the plugin to install
e.g.: node, ruby
Can specify multiple plugins: `mise plugins install node ruby python`

### `[GIT_URL]`

The git url of the plugin

## Flags

### `-f --force`

Reinstall even if plugin exists

### `-a --all`

Install all missing plugins
This will only install plugins that have matching shorthands.
i.e.: they don't need the full git repo url

### `-v --verbose…`

Show installation output

### `-j --jobs <JOBS>`

Number of jobs to run in parallel

Examples:

```
# install the poetry via shorthand
$ mise plugins install poetry

# install the poetry plugin using a specific git url
$ mise plugins install poetry https://github.com/mise-plugins/mise-poetry.git

# install the poetry plugin using the git url only
# (poetry is inferred from the url)
$ mise plugins install https://github.com/mise-plugins/mise-poetry.git

# install the poetry plugin using a specific ref
$ mise plugins install poetry https://github.com/mise-plugins/mise-poetry.git#11d0c1e
```
