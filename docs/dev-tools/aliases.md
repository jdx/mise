# Tool Aliases

::: tip
`[alias]` has been renamed to `[tool_alias]` to distinguish it from `[shell_alias]`.
The old `[alias]` key still works but is deprecated.

For shell command aliases (like `alias ll='ls -la'`), see [Shell Aliases](/shell-aliases).
:::

## Aliased Backends

Tools can be aliased so that something like `node` which normally maps to `core:node` can be changed
to a different backend instead.

```toml [~/.config/mise/config.toml]
[tool_alias]
node = 'github:company/our-custom-node'   # shorthand for https://github.com/company/our-custom-node
erlang = 'aqua:company/our-custom-erlang' # use an aqua registry entry
```

## Aliased Versions

mise supports aliasing the versions of runtimes. One use-case for this is to define a stable name
that points to a specific version, so you can reference it symbolically in
`mise.toml`/`.tool-versions`. For example, you may want `lts-iron` to map to Node.js 20 so you can
set it with `node = "lts-iron"`.

User aliases can be created by adding a `tool_alias.<PLUGIN>.versions` section to
`~/.config/mise/config.toml`:

```toml
[tool_alias.node.versions]
lts-iron = '20'
```

Then reference the alias when pinning the tool:

```toml
[tools]
node = "lts-iron"
```

Plugins can also provide aliases via a `bin/list-aliases` script. Here is an example showing node.js
versions:

```bash
#!/usr/bin/env bash

echo "lts-krypton 24"
echo "lts-jod 22"
echo "lts-iron 20"
```

(mise's built-in node plugin already ships these LTS aliases; the example above shows the format
that other plugins can use.)

## Templates

Alias values can be templates, see [Templates](/templates) for details.

```toml
[tool_alias.node.versions]
current = "{{exec(command='node --version')}}"
```
