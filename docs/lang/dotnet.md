---
description: "Install .NET SDKs and runtimes with mise, selected by global.json or isolated per version."
---

# .NET

mise installs .NET SDKs, or only a .NET runtime, with Microsoft's
`dotnet-install` script. By default all SDKs share one `DOTNET_ROOT`, and .NET
picks an SDK using the project's `global.json`.

## Quick start

Install the newest .NET 10 SDK for the current project and check which SDK
.NET selects:

```sh
mise use dotnet@10
mise exec -- dotnet --version
mise exec -- dotnet --list-sdks
```

Use `mise use -g dotnet@10` for a personal default. To install another SDK
without changing the project's version request, run `mise install`:

```sh
mise install dotnet@8
```

In the default shared mode, `dotnet --list-sdks` lists every SDK you installed
through mise, and .NET, not `mise.toml`, decides which one runs. For a project
that must build with a particular SDK, add a [`global.json`](#version-files) or
enable [isolated mode](#isolated-mode) before installing.

To install .NET global tools such as `dotnet-ef`, use the
[`dotnet:` backend](/dev-tools/backends/dotnet.html), for example
`mise use dotnet:dotnet-ef`.

## Choosing a version

| Request                             | Installs                             |
| ----------------------------------- | ------------------------------------ |
| `dotnet@10`                         | The newest .NET 10 SDK               |
| `dotnet@8.0.425`                    | That SDK release                     |
| `dotnet[runtime=dotnet]@8.0.14`     | Only the .NET runtime 8.0.14         |
| `dotnet[runtime=aspnetcore]@8.0.14` | Only the ASP.NET Core runtime 8.0.14 |

mise lists SDK versions from its
[versions host](/configuration/settings.html#use_versions_host), or from
Microsoft's release metadata when `use_versions_host` is off. List them with
`mise ls-remote dotnet`. `dotnet-core` is an alias for `dotnet`. See
[runtime-only installs](#runtime-only-installs) for the runtime form.

## Version files

mise can read the SDK version from `global.json`. Enable it for .NET.
`settings add` appends to any tools already enabled:

```sh
mise settings add idiomatic_version_file_enable_tools dotnet
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

For example, this file requests an exact SDK and disables .NET's roll-forward:

```json [global.json]
{
  "sdk": {
    "version": "8.0.425",
    "rollForward": "disable"
  }
}
```

Run `mise install`, then `mise exec -- dotnet --version` from the project.
mise reads `sdk.version` to install that SDK. .NET itself applies
`rollForward` and the rest of its SDK selection policy; see Microsoft's
[`global.json` reference](https://learn.microsoft.com/en-us/dotnet/core/tools/global-json).
When `global.json` is the project's version source, do not also keep a
different `dotnet` version in `mise.toml`.

## Shared and isolated installs {#isolated-mode}

By default mise installs every SDK into one shared directory, which is how .NET
expects SDKs to coexist. mise links its own install directory for each version
to that shared root, so `mise ls` still tracks the versions. Without
`global.json`, .NET uses the newest installed SDK, whatever version `mise.toml`
names.

To give each SDK version its own directory, as mise does for most tools, enable
isolated mode before installing:

```sh
mise settings dotnet.isolated=true
```

Changing the setting does not move SDKs that are already installed in the
shared root, so choose the mode before you install the versions you need.

|                      | Shared (default)        | Isolated                                        |
| -------------------- | ----------------------- | ----------------------------------------------- |
| `dotnet --list-sdks` | Every installed SDK     | The selected SDK only                           |
| Install location     | The shared root         | `~/.local/share/mise/installs/dotnet/<version>` |
| Multi-targeting      | Works without switching | Requires switching versions                     |

The shared root is [`dotnet.dotnet_root`](/lang/dotnet.html#dotnet.dotnet_root) when set. If it
is unset, mise uses the `DOTNET_ROOT` already in your environment, and then
`~/.local/share/mise/dotnet-root`.

## Runtime-only installs

To run .NET applications without building them, install only a runtime with
the `runtime` option:

```sh
mise use "dotnet[runtime=dotnet]@8.0.14"
mise exec -- dotnet --list-runtimes
```

| `runtime` value  | Framework                      | Use for                 |
| ---------------- | ------------------------------ | ----------------------- |
| `dotnet`         | `Microsoft.NETCore.App`        | Console apps, libraries |
| `aspnetcore`     | `Microsoft.AspNetCore.App`     | ASP.NET Core web apps   |
| `windowsdesktop` | `Microsoft.WindowsDesktop.App` | WPF and WinForms apps   |

The version in a runtime-only request is a .NET runtime version, not an SDK
version: `8.0.14` means .NET Runtime 8.0.14 (see the
[.NET release notes](https://github.com/dotnet/core/tree/main/release-notes)).
It must be exact. `dotnet[runtime=dotnet]@8` does not work, because a prefix
resolves against SDK versions. A runtime-only install has no SDK, so
`dotnet build`, `dotnet publish` and `dotnet --version` are unavailable.

You can install an SDK for development next to a runtime for a
production-like environment:

```toml [mise.toml]
[tools]
dotnet = ["10", { version = "8.0.14", runtime = "dotnet" }]
```

## Environment variables

mise sets these variables for the selected .NET and puts `DOTNET_ROOT` on
`PATH`:

| Variable                      | Value                                                                                                       |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `DOTNET_ROOT`                 | The shared root, or the version's install directory in isolated mode                                        |
| `DOTNET_MULTILEVEL_LOOKUP`    | `0`, so .NET does not look for SDKs outside `DOTNET_ROOT`                                                   |
| `DOTNET_CLI_TELEMETRY_OPTOUT` | `1` or `0`, only when [`dotnet.cli_telemetry_optout`](/lang/dotnet.html#dotnet.cli_telemetry_optout) is set |

## How mise installs .NET

For each install, mise downloads the current `dotnet-install` script from
`dot.net` (`dotnet-install.sh`, or `dotnet-install.ps1` on Windows) and runs it
with the requested version and install directory. The script downloads the
SDK or runtime itself. After an SDK install, mise runs `dotnet --list-sdks` and
checks that the new SDK is listed. Uninstalling a version from the shared root
removes only that SDK or runtime directory.

An installed plugin named `dotnet` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

### `runtime`

Installs only the named runtime instead of an SDK. See
[runtime-only installs](#runtime-only-installs).

### `install_env`

Sets environment variables for the `dotnet-install` script and the
`dotnet --list-sdks` check. It does not reach mise's own download of the
script, so set a proxy with `https_proxy` in the environment that runs mise.
Both that download and the script's downloads then use it (see the
[FAQ](/faq.html#how-do-i-use-mise-with-http-proxies)).

To opt out of .NET CLI telemetry, set
[`dotnet.cli_telemetry_optout`](/lang/dotnet.html#dotnet.cli_telemetry_optout) instead. Other
generic options are described in [tool options](/dev-tools/#tool-options).

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="dotnet" :level="3" />
