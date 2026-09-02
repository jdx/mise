# Backends

Backends are package managers or ecosystems that mise uses to install [tools](/dev-tools/index.html) and [plugins](/plugins.html). Each backend can install and manage multiple tools from its ecosystem. For example, the `npm` backend can install tools like `npm:prettier`, and the `pipx` backend can install tools like `pipx:black`. This lets mise support a wide variety of tools and languages by leveraging existing package managers and their ecosystems.

When you run [`mise use`](/cli/use.html), mise determines the appropriate backend based on the tool you are managing. The backend then handles installation, configuration, and any other steps needed to make the tool ready to use.

For more details on how backends fit into mise's overall design, see the [backend architecture documentation](/dev-tools/backend_architecture.html).

Below is a list of the available backends in mise:

- [asdf](/dev-tools/backends/asdf) (provides tools through [plugins](/plugins.html))
- [aqua](/dev-tools/backends/aqua)
- [cargo](/dev-tools/backends/cargo)
- [conda](/dev-tools/backends/conda)
- [dotnet](/dev-tools/backends/dotnet)
- [forgejo](/dev-tools/backends/forgejo)
- [gem](/dev-tools/backends/gem)
- [github](/dev-tools/backends/github)
- [gitlab](/dev-tools/backends/gitlab)
- [go](/dev-tools/backends/go)
- [http](/dev-tools/backends/http)
- [npm](/dev-tools/backends/npm)
- [pipx](/dev-tools/backends/pipx)
- [pkgx](/dev-tools/backends/pkgx) <Badge type="warning" text="experimental" />
- [s3](/dev-tools/backends/s3)
- [spm](/dev-tools/backends/spm)
- [ubi](/dev-tools/backends/ubi)
- [vfox](/dev-tools/backends/vfox) (provides tools through [plugins](/plugins.html))
- [custom backends](/backend-plugin-development) (build your own backend with a plugin that itself provides many tools)
