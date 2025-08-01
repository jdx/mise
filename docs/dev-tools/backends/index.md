# Backends

Backends are package managers or ecosystems that mise uses to install [tools](/dev-tools/index.html) and [plugins](/plugins.html). Each backend can install and manage multiple tools from its ecosystem. For example, the `npm` backend can install many different tools like `npm:prettier`, or the `pipx` backend can install tools like `pipx:black`. This allows mise to support a wide variety of tools and languages by leveraging different package managers and their ecosystems.

When you run the [`mise use`](/cli/use.html) command, mise will determine the appropriate backend to use based on the tool you are trying to manage. The backend will then handle the installation, configuration, and any other necessary steps to ensure the tool is ready to use.

For more details on how backends fit into mise's overall design, see the [backend architecture documentation](/dev-tools/backend_architecture.html).

Below is a list of the available backends in mise:

- [asdf](/dev-tools/backends/asdf) (provide tools through [plugins](/plugins.html))
- [aqua](/dev-tools/backends/aqua)
- [cargo](/dev-tools/backends/cargo)
- [dotnet](/dev-tools/backends/dotnet) <Badge type="warning" text="experimental" />
- [gem](/dev-tools/backends/gem) <Badge type="warning" text="experimental" />
- [github](/dev-tools/backends/github) <Badge type="warning" text="experimental" />
- [gitlab](/dev-tools/backends/gitlab) <Badge type="warning" text="experimental" />
- [go](/dev-tools/backends/go) <Badge type="warning" text="experimental" />
- [http](/dev-tools/backends/http) <Badge type="warning" text="experimental" />
- [npm](/dev-tools/backends/npm)
- [pipx](/dev-tools/backends/pipx)
- [spm](/dev-tools/backends/spm) <Badge type="warning" text="experimental" />
- [ubi](/dev-tools/backends/ubi)
- [vfox](/dev-tools/backends/vfox) (provide tools through [plugins](/plugins.html)) <Badge type="warning" text="experimental" />
- [custom backends](/backend-plugin-development) (build your own backend with a plugin which itself provides many tools) <Badge type="warning" text="experimental" />
