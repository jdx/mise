---
editLink: false
---

# Registry

<script setup>
import Registry from '/components/registry.vue';
</script>

In general, the preferred backend to use for new tools is the following:

- [aqua](./dev-tools/backends/aqua.html) - offers the most features and security while not requiring plugins
- [ubi](./dev-tools/backends/ubi.html) - very simple to use
- [pipx](./dev-tools/backends/pipx.html) - only for python tools, requires python to be installed but this generally would always be the case for python tools
- [npm](./dev-tools/backends/npm.html) - only for node tools, requires node to be installed but this generally would always be the case for node tools
- [vfox](./dev-tools/backends/vfox.html) - only for tools that have unique installation requirements or need to modify env vars
- [asdf](./dev-tools/backends/asdf.html) - only for tools that have unique installation requirements or need to modify env vars, doesn't support windows
- [go](./dev-tools/backends/go.html) - only for go tools, requires go to be installed to compile. Because go tools can be distributed as a single binary, aqua/ubi are definitely preferred.
- [cargo](./dev-tools/backends/cargo.html) - only for rust tools, requires rust to be installed to compile. Because rust tools can be distributed as a single binary, aqua/ubi are definitely preferred.
- [dotnet](./dev-tools/backends/dotnet.html) - only for dotnet tools, requires dotnet to be installed to compile. Because dotnet tools can be distributed as a single binary, aqua/ubi are definitely preferred.

However, each tool can define its own priority if it has more than 1 backend it supports. You can disable a backend with `mise settings disable_backends=asdf`.
And it will be skipped. See [Aliases](/dev-tools/aliases.html) for a way to set a default backend for a tool.

You can also specify the full name for a tool using `mise use aqua:1password/cli` if you want to use a specific backend.

<Registry />
