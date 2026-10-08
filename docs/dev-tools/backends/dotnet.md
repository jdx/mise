---
description: "Install .NET command-line tools from NuGet with dotnet tool install, each into its own directory."
---

# dotnet backend

The `dotnet` backend installs .NET command-line tool packages from
[NuGet](https://www.nuget.org/) with `dotnet tool install`. The unprefixed
`dotnet` tool installs the SDK itself; see the [.NET guide](/lang/dotnet.html)
for SDK versions and `global.json`.

## Requirements

<span id="dependencies"></span>

Install a .NET SDK, plus the runtime the tool package targets. A newer SDK does
not guarantee that an older tool runs, because .NET's runtime selection rules
still apply. Run `mise exec -- dotnet --list-runtimes` to see what is installed.
When `dotnet` is in your config, mise installs it before your .NET tools.

## Usage

Install .NET 8 and a GitVersion release built for it:

```sh
mise use dotnet@8 dotnet:GitVersion.Tool@6.0.5
mise exec -- dotnet-gitversion /version
```

This writes both entries to `mise.toml`. Add `-g` to `mise use` for your global
config.

```toml
[tools]
dotnet = "8"
"dotnet:GitVersion.Tool" = "6.0.5"
```

To choose another release, run `mise ls-remote dotnet:GitVersion.Tool` and
check that release's runtime requirements. `mise use dotnet:GitVersion.Tool`
without a version records `latest`.

mise installs each tool into its own directory with `--tool-path`. It does not
create or update a project's `.config/dotnet-tools.json` manifest.

## Private feeds

mise lists versions from the NuGet service index in
[`dotnet.registry_url`](/configuration/settings.html#dotnet.registry_url), but
`dotnet tool install` downloads the package using its own `NuGet.Config` and
credentials. For a private feed, add it to `NuGet.Config` and point
`dotnet.registry_url` at its service index; changing the setting alone does not
add an install source.

## Tool options

Set these on the tool's entry in `[tools]`, or inline, as in
`'dotnet:GitVersion.Tool[prerelease=true]'`. Options every backend accepts are
described under [tool options](/dev-tools/#tool-options).

### `install_env`

Set environment variables for `dotnet tool install`:

```toml
[tools]
"dotnet:GitVersion.Tool" = { version = "latest", install_env = { DOTNET_NOLOGO = "1" } }
```

### `prerelease`

By default, NuGet prerelease versions are left out of `mise ls-remote` and
`latest`. Set `prerelease = true` to include them:

```toml
[tools]
"dotnet:GitVersion.Tool" = { version = "latest", prerelease = true }
```

To include prereleases for every tool, set the
[`prereleases`](/configuration/settings.html#prereleases) setting instead.

## Settings

Two settings apply to `dotnet:` tools. The other `dotnet.*` settings configure
the .NET SDK; see the [.NET guide](/lang/dotnet.html#settings).

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="dotnet" :keys="['registry_url', 'package_flags']" :level="3" />

mise 2026.11.0 and later warn when `dotnet.package_flags` is set, and mise
2027.11.0 removes it.

## Troubleshooting

| Symptom                    | What to check                                                                                            |
| -------------------------- | -------------------------------------------------------------------------------------------------------- |
| SDK not found              | Run `mise exec -- dotnet --info` and check any `global.json` that constrains SDK selection.              |
| Required framework missing | Install a runtime or SDK the tool supports, or choose a tool release that targets the runtime you have.  |
| Package not found          | Check that the package is a .NET tool and that both version listing and installation can reach its feed. |

Implementation: [`src/backend/dotnet.rs`](https://github.com/jdx/mise/blob/main/src/backend/dotnet.rs).
