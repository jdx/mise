# Backends

Backends are the way mise installs [tools](/dev-tools/) and [plugins](/plugins.html). Each backend is responsible for managing the installation and usage of a specific type of tool or package manager. This allows mise to support a wide variety of tools and languages by leveraging different backends.

When you run the [`mise use`](/cli/use.html) command, mise will determine the appropriate backend to use based on the tool you are trying to manage. The backend will then handle the installation, configuration, and any other necessary steps to ensure the tool is ready to use.

Below is a list of the available backends in mise:

- [asdf](/dev-tools/backends/asdf) (provide tools through [plugins](/plugins.html))
- [aqua](/dev-tools/backends/aqua)
- [cargo](/dev-tools/backends/cargo)
- [dotnet](/dev-tools/backends/dotnet) <Badge type="warning" text="experimental" />
- [gem](/dev-tools/backends/gem) <Badge type="warning" text="experimental" />
- [go](/dev-tools/backends/go) <Badge type="warning" text="experimental" />
- [npm](/dev-tools/backends/npm)
- [pipx](/dev-tools/backends/pipx)
- [spm](/dev-tools/backends/spm) <Badge type="warning" text="experimental" />
- [ubi](/dev-tools/backends/ubi)
- [vfox](/dev-tools/backends/vfox) (provide tools through [plugins](/plugins.html)) <Badge type="warning" text="experimental" />

::: tip
If you'd like to contribute a new backend to mise, they're not difficult to write.
See [`./src/backend/`](https://github.com/jdx/mise/tree/main/src/backend) for examples.
:::
