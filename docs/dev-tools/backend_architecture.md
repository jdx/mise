# Backend Architecture

A backend resolves a tool's versions, installs it, and supplies its executable
paths and environment. The [registry](/registry.html) maps short names such as
`node` and `ripgrep` to backends. Start with those names; choose an explicit
backend when you need a particular distribution or a tool outside the registry.

## What are Backends?

In `github:BurntSushi/ripgrep`, `github` is the backend and
`BurntSushi/ripgrep` identifies the upstream project. These are separate from
the version request after `@`:

```sh
mise ls-remote github:BurntSushi/ripgrep
mise use github:BurntSushi/ripgrep@latest
mise exec -- rg --version
```

Installing a tool through a backend does not add a new entry to mise's registry.
Explicit backend syntax works directly in your own configuration.

## The Backend Trait System

Built-in backends implement the Rust
[`Backend` trait](https://github.com/jdx/mise/blob/main/src/backend/mod.rs).
The installation flow uses that interface to:

1. List versions or resolve a request such as a prefix or channel.
2. Identify installation dependencies and tool options.
3. Download or build the requested version and perform the verification supported
   by that distribution.
4. Record the installation and expose executable paths and environment variables.

Version strings are not necessarily semantic versions. Backends can support date
releases, vendor prefixes, tags, and rolling channels; the backend determines what
`latest` means. A lockfile records a concrete resolution. See
[version ordering](/dev-tools/#version-ordering) and [mise.lock](/dev-tools/mise-lock.html).

For extension APIs, see [tool plugins](/tool-plugin-development.html) and
[backend plugins](/backend-plugin-development.html). Their hook interfaces are
separate from the internal Rust trait.

## Backend Types

| Distribution method          | Examples                                            | What to check                                                                  |
| ---------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------ |
| Built-in language support    | Node.js, Python, Java, Rust                         | Language-specific options, system libraries, and any build tools required      |
| Signed release manifests     | `packslip:`                                         | Published manifest, trusted signer, and supported platform                     |
| Registry-described downloads | `aqua:`                                             | Package entry and its per-version download/verification rules                  |
| Forge releases               | `github:`, `gitlab:`, `forgejo:`                    | Matching release assets for the target platform                                |
| Direct artifacts             | `http:`, `s3:`                                      | Artifact location, authentication, platform mapping, and integrity information |
| Language packages            | `npm:`, `pipx:`, `cargo:`, `gem:`, `go:`, `dotnet:` | Required runtime or toolchain and package-manager behavior                     |
| Other package sources        | `conda:`, `pkgx:`, `spm:`                           | Backend-specific platform support and dependencies                             |
| External plugins             | asdf, vfox tool plugins, backend plugins            | Plugin code, prerequisites, and supported platforms                            |

The [backend reference](/dev-tools/backends/) lists available backends and their
options. Built-in language guides are under **Languages** in the sidebar.

## How Backend Selection Works

An explicit identifier such as `core:node` expresses a backend choice. A short
name such as `node` also depends on configuration and local state:

- `[tool_alias]` and `[plugins]` can select another source.
- A matching lockfile entry can preserve the backend used for that resolution.
- An installed external plugin can override a registry shorthand, including a
  built-in language tool. Disabled backends and existing installations also affect
  this choice.
- Otherwise, the registry supplies the preferred available backend, which can
  depend on the requested version and platform.

Use `mise tool <name>` to inspect the effective backend instead of inferring it
from the tool's short name. The
[resolution implementation](https://github.com/jdx/mise/blob/main/src/cli/args/backend_arg.rs)
contains the detailed precedence rules.

### Environment Variable Overrides

`MISE_BACKENDS_<TOOL>` overrides the backend for that identifier. Convert the
name to uppercase and replace hyphens with underscores. For example, in a POSIX
shell:

```sh
MISE_BACKENDS_NODE=core:node mise tool node
```

An exported override affects subsequent commands and can override configuration
choices. Check your environment when two machines resolve the same shorthand
differently.

### Registry System

Inspect a registry mapping with `mise registry node`. To commit a backend choice
without changing the registry, use an [alias](/dev-tools/aliases.html):

```toml [mise.toml]
[tool_alias]
node = "core:node"

[tools]
node = "24"
```

## Backend Capabilities Comparison

Verification and platform support depend on both the backend and the particular
tool. A backend supporting Windows does not imply every package it installs has
a Windows release. Likewise, downloading a checksum does not establish the
publisher's identity unless that checksum is authenticated.

Consult each backend's verification options and the
[security guide](/security.html). For example, packslip verifies signed manifests,
and aqua can apply the verification methods declared by its registry entry.
External plugin code runs locally and must be trusted along with the tool itself.

## When to Use Each Backend

Start with a registry shorthand for supported tools. For another source:

- Use `packslip:` when the publisher provides signed release manifests.
- Use `aqua:` when its registry describes the required tool and releases.
- Use a forge backend for release assets, or `http:` or `s3:` for artifacts
  you distribute directly.
- Use a language package backend when you need that ecosystem's package and can
  supply its runtime or build dependencies.
- Use a plugin when installation or environment setup needs custom logic.

::: warning Deprecated backend
`ubi:` is deprecated. Use the corresponding `github:` or `gitlab:` backend and
review its options when migrating; see the [ubi migration guide](/dev-tools/backends/ubi.html).
:::

## Backend Dependencies

Backends may need another tool during installation. Declare the required tools
alongside the package so mise can install them in order:

```toml [mise.toml]
[tools]
node = "24"
"npm:prettier" = "3"
```

A dependency relationship does not automatically add missing tools to your
configuration. A matching configured tool is installed first; an unconfigured
dependency may be satisfied by a suitable executable on the existing `PATH`.
Otherwise installation fails. See [tool dependencies](/dev-tools/#tool-dependencies)
for explicit `depends` declarations.

## Configuration and Overrides

### Disable Backends

Use the global settings file to prevent installation through selected backends:

```toml [~/.config/mise/config.toml]
[settings]
disable_backends = ["asdf", "vfox"]
```

This is an installation restriction, not an uninstall operation. Existing tools
can still report the backend that installed them.

### Force Backend for Tool

An explicit identifier can be used directly as a tool key:

```toml [mise.toml]
[tools]
"core:node" = "24"
"aqua:BurntSushi/ripgrep" = "latest"
```

### Backend-Specific Settings

Read the selected backend's reference before adding options. For example, this
selects an optional extra from a Python package:

```toml [mise.toml]
[tools]
python = "3.14"
uv = "latest"
"pipx:black" = { version = "latest", extras = ["jupyter"] }
```

The [pipx backend](/dev-tools/backends/pipx.html) can use uv or pipx. Backend options
are not interchangeable with options for another distribution of the same tool.

## Troubleshooting Backend Issues

### Debug Backend Selection

```sh
mise tool node       # effective backend and tool information
mise plugins ls      # external plugins that may override defaults
mise config ls       # configuration files contributing to this directory
mise ls --current    # selected versions and their sources
mise doctor          # installation and activation diagnostics
```

If selection is correct but installation fails, check the backend's prerequisites,
platform support, and authentication requirements. `MISE_DEBUG=1 mise install node` adds diagnostic output; review logs for credentials before sharing them.
