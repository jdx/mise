# Backends

A backend tells mise where to find a tool's versions and how to install them.
Use a registry shorthand such as `ripgrep` for the default selection, or specify
a backend explicitly, such as `github:BurntSushi/ripgrep`.

## Choose an installation source

Start by checking the [registry](/registry.html):

```sh
mise registry ripgrep
mise ls-remote ripgrep
mise use ripgrep
mise exec -- rg --version
```

`mise use` installs the tool and records it in the project's `mise.toml`.
Add `-g` for your global configuration. You can use an explicit backend even
when a tool has no registry shorthand; a registry submission is not required.

| Source                      | Backends                                                                                                                                                      | What to check                                                                            |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Signed publisher manifests  | [Packslip](./packslip.html)                                                                                                                                   | The publisher must provide Packslip releases and a verifiable signer.                    |
| Curated binary recipes      | [Aqua](./aqua.html)                                                                                                                                           | The aqua registry must have an entry for the tool and platform. No aqua CLI is required. |
| Release assets              | [GitHub](./github.html), [GitLab](./gitlab.html), [Forgejo](./forgejo.html)                                                                                   | Releases must include an installable asset for your platform.                            |
| Direct downloads            | [HTTP](./http.html), [S3](./s3.html)                                                                                                                          | Supply download URLs and, for version discovery, a version source.                       |
| Language packages           | [Cargo](./cargo.html), [Go](./go.html), [npm](./npm.html), [pipx](./pipx.html), [gem](./gem.html), [.NET](./dotnet.html), [Swift Package Manager](./spm.html) | Read the backend's runtime and build prerequisites.                                      |
| Binary package ecosystems   | [Conda](./conda.html), [pkgx](./pkgx.html) (experimental)                                                                                                     | Packages and their runtime dependencies must support your platform.                      |
| Plugin-defined installation | [vfox](./vfox.html), [asdf](./asdf.html) (legacy)                                                                                                             | Review the plugin and its dependencies before installation.                              |
| Legacy release installer    | [ubi](./ubi.html) (deprecated)                                                                                                                                | Migrate existing configurations to the appropriate release backend.                      |

Built-in language support is documented in the [language guides](/lang/node.html).
Plugin authors can also create [custom backends](/backend-plugin-development.html)
that manage a family of tools.

## Verify the result

A listed version does not guarantee that its publisher ships an artifact for your
OS and architecture. Check installation and the actual executable with
`mise exec -- <command> --version`. If selection fails, the backend's guide
explains its asset names, authentication, and platform options.

For reproducible installations, record concrete versions and supported-platform
checksums in [mise.lock](/dev-tools/mise-lock.html). Verification coverage differs
by backend; see [security](/security.html) and the individual guide before relying
on a particular signature or provenance check.

See [backend architecture](/dev-tools/backend_architecture.html) for selection,
installation dependencies, and the implementation lifecycle.
