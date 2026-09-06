# Tool Aliases

Use `[tool_alias]` to give a tool a different backend or name a version request.
Put shared aliases in the project's `mise.toml`; use
`~/.config/mise/config.toml` for personal defaults. Teammates need the alias
definition as well as the tool declaration.

::: warning Renamed configuration key
`[alias]` was renamed to `[tool_alias]`. The old key still works but is deprecated.
For command shortcuts such as `alias ll='ls -la'`, use
[Shell Aliases](/shell-aliases.html).
:::

## Aliased Backends

A backend alias changes where mise obtains a tool. For example, explicitly choose
mise's built-in Node.js backend:

```toml [mise.toml]
[tool_alias]
node = "core:node"

[tools]
node = "24"
```

Inspect the result with `mise tool node`, then run `mise install` to install the
selected version. See [backend selection](/dev-tools/backend_architecture.html#how-backend-selection-works)
if an installed plugin or environment override selects a different source than expected.

Aliases can also select separate release assets from the same GitHub repository:

```toml [mise.toml]
[tool_alias]
dhall-json = "github:dhall-lang/dhall-haskell"
dhall-lsp = "github:dhall-lang/dhall-haskell"

[tools]
dhall-json = { version = "v1.42.2", matching = "dhall-json" }
dhall-lsp = { version = "latest", matching = "dhall-lsp-server" }
```

Each alias has its own version request and
[GitHub asset filter](/dev-tools/backends/github.html#matching). This matters for
repositories that release several tools independently.

## Aliased Versions

A version alias gives a stable name to a version request. Keep a team's Node.js
release series in one place:

```toml [mise.toml]
[tool_alias.node.versions]
project-lts = "24"

[tools]
node = "project-lts"
```

`project-lts` resolves as a request for Node.js 24. It is not an exact patch pin;
use [mise.lock](/dev-tools/mise-lock.html) to record the resolved version. Changing
the alias changes the request for every declaration that uses it.

The built-in Node.js backend already supplies aliases such as `lts` and named
LTS releases. You don't need to redefine those. Legacy asdf plugin authors can
provide their own aliases through `bin/list-aliases`, with one alias and version
per line:

```bash
#!/usr/bin/env bash
printf '%s\n' 'recommended 2.0' 'legacy 1.0'
```

## Templates

Alias values support [templates](/templates.html). For example, allow an explicit
environment override with a default release series:

```toml
[tool_alias.node.versions]
project-lts = "{{ env.PROJECT_NODE_VERSION | default(value='24') }}"
```

Set `PROJECT_NODE_VERSION` before invoking mise. Avoid computing a tool's version
by invoking that same tool from its alias template: version resolution may happen
before the tool is available, or re-enter mise through a shim.
