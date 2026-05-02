---
editLink: false
---

# Registry

<script setup>
import Registry from '/components/registry.vue';
</script>

List of all [tools](#tools) aliased by default in `mise`.

You can use these shorthands with `mise use`. This allows you to use a tool without needing to know the full name. For example, to use the `aws-cli` tool, you can do the following:

```shell
mise use aws-cli
```

instead of

```shell
mise use aqua:aws/aws-cli
```

If a tool is not available in the registry, you can install it by its full name. [github](./dev-tools/backends/github.html) and [aqua](./dev-tools/backends/aqua.html) give you for example access to almost all programs available on GitHub.

## Backends

In addition to built-in [core tools](/core-tools.html), `mise` supports a variety of [backends](/dev-tools/backends/) to install tools.

Backends fall into the following acceptance tiers for new registry entries:

**Tier 1 — preferred, routinely accepted:**

- [aqua](./dev-tools/backends/aqua.html) - offers the most features and security while not requiring plugins
- [github](./dev-tools/backends/github.html) - for tools that are not available in the aqua registry, but are available on GitHub
- [gitlab](./dev-tools/backends/gitlab.html) - for tools that are not available in the aqua registry, but are available on GitLab

**Tier 2 — high bar, but lower than tier 3:**

- [conda](./dev-tools/backends/conda.html) - potentially accepted for tools that can't reasonably be supported via aqua/github. The bar is lower than tier 3 because mise's conda backend does not require a separately-installed package manager — packages are fetched and extracted directly from anaconda.org with no `conda`/`mamba`/`micromamba` needed on PATH.

**Tier 3 — very high bar, rarely accepted:**

- [pipx](./dev-tools/backends/pipx.html) - only for python tools, requires `python` on PATH
- [npm](./dev-tools/backends/npm.html) - only for node tools, requires `node` on PATH
- [gem](./dev-tools/backends/gem.html) - only for ruby tools, requires `ruby` on PATH
- [go](./dev-tools/backends/go.html) - only for go tools, requires `go` to be installed to compile. Because go tools can be distributed as a single binary, aqua/github are definitely preferred.
- [cargo](./dev-tools/backends/cargo.html) - only for rust tools, requires `cargo` to be installed to compile. Because rust tools can be distributed as a single binary, aqua/github are definitely preferred.
- [dotnet](./dev-tools/backends/dotnet.html) - only for dotnet tools, requires `dotnet` to be installed to compile. Because dotnet tools can be distributed as a single binary, aqua/github are definitely preferred.

These all depend on a separately-installed runtime/toolchain on PATH, which is fragile — `npm`/`pipx`/`gem` in particular silently bind tools to whichever `node`/`python`/`ruby` happened to be on PATH at install time.

**Not accepted:**

- New `vfox` and `asdf` tools are not accepted for supply-chain security reasons — use [`aqua`](./dev-tools/backends/aqua.html) (preferred) or [`github`](./dev-tools/backends/github.html) instead.
- The `ubi` backend is deprecated and is not accepted for new registry entries.

Users can still install via any backend themselves with explicit syntax (`mise use vfox:owner/repo`, `mise use cargo:name`, etc.) — they just don't get a registry shorthand for it.

### Backends Priority

Each tool can define its own priority if it has more than one backend it supports. If you would like to disable a backend, you can do so with the following command:

```shell
mise settings disable_backends=asdf
```

This will disable the [asdf](./dev-tools/backends/asdf.html) backend. See [Aliases](/dev-tools/aliases.html) for a way to set a default backend for a tool. Note that the `asdf` backend is disabled by default on Windows.

You can also specify the full name for a tool using `mise use aqua:1password/cli` if you want to use a specific backend.

### Environment Variable Overrides

You can override the backend for any tool using environment variables with the pattern `MISE_BACKENDS_<TOOL>`. This takes the highest priority and overrides any registry or alias configuration:

```shell
# Use vfox backend for php
export MISE_BACKENDS_PHP='vfox:mise-plugins/vfox-php'
mise install php@latest
```

The tool name in the environment variable should be in SHOUTY_SNAKE_CASE (uppercase with underscores). For example, `my-tool` becomes `MISE_BACKENDS_MY_TOOL`.

Source: <https://github.com/jdx/mise/blob/main/registry/>

## Tools {#tools}

Note that [`mise registry`](/cli/registry.html) can be used to list all tools in the registry. [`mise use`](/cli/use.html) without any arguments will show a `tui` to select a tool to install.

<Registry />
