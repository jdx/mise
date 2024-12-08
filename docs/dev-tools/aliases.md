# Aliases

## Aliased Backends

Tools can be aliased so that something like `node` which normally maps to `core:node` can be changed
to something like `asdf:company/our-custom-node` instead.

```toml
[alias]
node = 'asdf:company/our-custom-node' # shorthand for https://github.com/company/our-custom-node
erlang = 'asdf:https://github.com/company/our-custom-erlang'
```

## Aliased Versions

mise supports aliasing the versions of runtimes. One use-case for this is to define aliases for LTS
versions of runtimes. For example, you may want to specify `lts-hydrogen` as the version for <node@20.x>
so you can use set it with `node lts-hydrogen` in `mise.toml`/`.tool-versions`.

User aliases can be created by adding an `alias.<PLUGIN>` section to `~/.config/mise/config.toml`:

```toml
[alias.node.versions]
my_custom_20 = '20'
```

Plugins can also provide aliases via a `bin/list-aliases` script. Here is an example showing node.js
versions:

```bash
#!/usr/bin/env bash

echo "lts-hydrogen 18"
echo "lts-gallium 16"
echo "lts-fermium 14"
```

::: info
Because this is mise-specific functionality not currently used by asdf it isn't likely to be in any
plugin currently, but plugin authors can add this script without impacting asdf users.
:::

## Templates

Alias values can be templates, see [Templates](/templates) for details.

```toml
[alias.node.versions]
current = "{{exec(command='node --version')}}"
```
