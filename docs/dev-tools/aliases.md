---
description: "Use [tool_alias] to point a tool name at a different backend or give a version request a stable name."
---

# Tool aliases

Use `[tool_alias]` to point a tool name at a different backend or to give a
version request a name. Put shared aliases in the project's `mise.toml`, so
teammates get the alias along with the tool declaration, and personal ones in
`~/.config/mise/config.toml`.

Two other things in mise are called aliases. Registry aliases, such as `rg`
for `ripgrep`, are alternate names built into the [registry](/registry.html);
you do not define them. For command shortcuts such as `alias ll='ls -la'`, see
[Shell aliases](/shell-aliases.html).

::: warning Renamed configuration key
`[alias]` was renamed to `[tool_alias]`. The old key still works but is
deprecated and prints a warning.
:::

## Backend aliases {#aliased-backends}

A backend alias changes where mise gets a tool. For example, keep mise's
built-in Node.js even if someone has installed a `node` plugin, which would
otherwise take precedence:

```toml [mise.toml]
[tool_alias]
node = "core:node"

[tools]
node = "24"
```

Check the result with `mise tool node`, then run `mise install`. See
[how backend selection works](/dev-tools/backends/#how-backend-selection-works)
when an installed plugin or an environment variable picks a different source
than you expect.

Aliases can also install separate release assets from one GitHub repository
as separate tools:

```toml [mise.toml]
[tool_alias]
dhall-json = "github:dhall-lang/dhall-haskell"
dhall-lsp = "github:dhall-lang/dhall-haskell"

[tools]
dhall-json = { version = "v1.42.2", matching = "dhall-json" }
dhall-lsp = { version = "latest", matching = "dhall-lsp-server" }
```

Each alias has its own version request and
[GitHub asset filter](/dev-tools/backends/github.html#matching), which helps
with repositories that release several tools independently.

## Version aliases {#aliased-versions}

A version alias gives a name to a version request. This keeps a team's Node.js
release series in one place:

```toml [mise.toml]
[tool_alias.node.versions]
project-lts = "24"

[tools]
node = "project-lts"
```

`project-lts` resolves as the request `24`, so it selects a 24.x release, not
an exact patch version; use [mise.lock](/dev-tools/mise-lock.html) to record
the resolved version. Changing the alias changes the request for every
declaration that uses it.

Some backends provide aliases of their own, such as Node.js `lts` and
`lts-jod`, and Java `lts`. You do not need to redefine them. Legacy asdf
plugins provide theirs through a `bin/list-aliases` script that prints one
alias and version per line.

## Manage aliases from the command line {#manage-aliases}

[`mise tool-alias ls`](/cli/tool-alias/ls.html) lists aliases, including the
ones backends provide:

```sh
mise tool-alias ls node
```

```text
node  lts          24
node  lts-argon    4
node  lts-boron    6
...
```

[`mise tool-alias set`](/cli/tool-alias/set.html) and
[`mise tool-alias unset`](/cli/tool-alias/unset.html) edit your global config.
Edit `mise.toml` directly for project aliases.

```sh
mise tool-alias set node project-lts 24  # version alias
mise tool-alias set mytool github:owner/repo  # backend alias
mise tool-alias unset node project-lts
```

## Templates {#templates}

Alias values support [templates](/templates.html). For example, read the
release series from an environment variable, with a default:

```toml [mise.toml]
[tool_alias.node.versions]
project-lts = "{{ env.PROJECT_NODE_VERSION | default(value='24') }}"
```

Set `PROJECT_NODE_VERSION` before running mise. Do not compute a tool's
version by running that same tool from its alias template: mise may resolve
the version before the tool is installed, or re-enter itself through a shim.
