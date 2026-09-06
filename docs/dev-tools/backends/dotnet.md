# .NET Tool Backend

The `dotnet:` backend installs command-line tool packages from NuGet using
`dotnet tool install`. The unprefixed `dotnet` tool installs the SDK; see the
[.NET language guide](/lang/dotnet.html) for SDK selection and `global.json`.

## Dependencies

Install a .NET SDK and the runtime required by the selected tool package.
A newer SDK alone does not guarantee that an older tool can run: .NET's runtime
selection rules still apply. Use `mise exec -- dotnet --list-runtimes` to inspect
what is installed.

## Usage

This example pairs .NET 8 with a GitVersion release that includes a .NET 8 tool:

```sh
mise use dotnet@8 dotnet:GitVersion.Tool@6.0.5
mise exec -- dotnet-gitversion /version
```

Both entries are written to the **project's** `mise.toml`:

```toml
[tools]
dotnet = "8"
"dotnet:GitVersion.Tool" = "6.0.5"
```

Add `-g` to `mise use` for global configuration. To choose another release, run
`mise ls-remote dotnet:GitVersion.Tool` and check that release's runtime
requirements. `mise use dotnet:GitVersion.Tool` records a `latest` request.

mise installs each tool into its own directory with `--tool-path`; it does not
create or update a project's `.config/dotnet-tools.json` manifest.

## Private feeds

`dotnet.registry_url` selects the NuGet service index used for version discovery.
The `dotnet` CLI handles installation separately, using its NuGet configuration
and credentials. Configure the installation source in `NuGet.Config` as well;
changing the discovery endpoint alone does not add a source to the CLI.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="dotnet" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `dotnet` backend—these
go in `[tools]` in `mise.toml`.

### `install_env`

Set environment variables for the `dotnet tool install` command:

```toml
[tools]
"dotnet:GitVersion.Tool" = { version = "latest", install_env = { DOTNET_CLI_TELEMETRY_OPTOUT = "1" } }
```

### `prerelease`

By default, NuGet pre-release versions are excluded from `mise ls-remote` and from `latest` resolution. Set `prerelease = true` to include them:

```toml
[tools]
"dotnet:GitVersion.Tool" = { version = "latest", prerelease = true }
```

The legacy `dotnet.package_flags = ["prerelease"]` setting is deprecated. Prefer the per-tool `prerelease = true` option, or the global `prereleases` setting when every tool should include pre-release versions. Because `dotnet.package_flags` is global, remove it before relying on per-tool `prerelease = false` opt-outs.

## Troubleshooting

- **SDK not found:** check `mise exec -- dotnet --info` and any `global.json` that constrains SDK selection.
- **Required framework missing:** install a compatible runtime/SDK or select a tool release that targets the runtime you have.
- **Package not found:** verify that the package is a .NET tool and that both discovery and installation can access its feed.

Implementation: [`src/backend/dotnet.rs`](https://github.com/jdx/mise/blob/main/src/backend/dotnet.rs).
