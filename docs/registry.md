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

If a tool is not available in the registry, you can install it by its full name. [ubi](./dev-tools/backends/ubi.html) and [aqua](./dev-tools/backends/aqua.html) give you for example access to almost all programs available on GitHub.

## Backends

In addition to built-in [core tools](/core-tools.html), `mise` supports a variety of [backends](/dev-tools/backends/) to install tools.

In general, the preferred [backend](/dev-tools/backends/) to use for new tools is the following:

- [aqua](./dev-tools/backends/aqua.html) - offers the most features and security while not requiring plugins
- [ubi](./dev-tools/backends/ubi.html) - Universal Binary Installer, offers a simple way to install tools from any GitHub/GitLab repo
- [pipx](./dev-tools/backends/pipx.html) - only for python tools, requires python to be installed but this generally would always be the case for python tools
- [npm](./dev-tools/backends/npm.html) - only for node tools, requires node to be installed but this generally would always be the case for node tools
- [vfox](./dev-tools/backends/vfox.html) - only for tools that have unique installation requirements or need to modify env vars
- [asdf](./dev-tools/backends/asdf.html) - only for tools that have unique installation requirements or need to modify env vars, doesn't support windows
- [go](./dev-tools/backends/go.html) - only for go tools, requires go to be installed to compile. Because go tools can be distributed as a single binary, aqua/ubi are definitely preferred.
- [cargo](./dev-tools/backends/cargo.html) - only for rust tools, requires rust to be installed to compile. Because rust tools can be distributed as a single binary, aqua/ubi are definitely preferred.
- [dotnet](./dev-tools/backends/dotnet.html) - only for dotnet tools, requires dotnet to be installed to compile. Because dotnet tools can be distributed as a single binary, aqua/ubi are definitely preferred.

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

Source: <https://github.com/jdx/mise/blob/main/registry.toml>

## Tools {#tools}

Note that [`mise registry`](/cli/registry.html) can be used to list all tools in the registry. [`mise use`](/cli/use.html) without any arguments will show a `tui` to select a tool to install.

<Registry />
