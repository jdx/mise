# .NET

The core .NET plugin installs .NET SDKs using Microsoft's official install script. All SDK versions are
installed side-by-side under a shared `DOTNET_ROOT` directory, matching .NET's native multi-version model.
This means `dotnet --list-sdks` will see every version you've installed through mise.

Unlike most tools, the SDKs don't live inside `~/.local/share/mise/installs` because they share a
common root. mise symlinks the install path to `DOTNET_ROOT` and sets environment variables so the
correct SDK is picked up.

::: info
This plugin manages the **.NET SDK** itself. To install .NET global tools (e.g., `dotnet-ef`),
use the [`dotnet` backend](/dev-tools/backends/dotnet.html) with `dotnet:ToolName` syntax.
:::

## Usage

Use the latest .NET SDK:

```sh
mise use -g dotnet@latest
dotnet --version
```

Use a specific version:

```sh
mise use -g dotnet@8.0.400
dotnet --version
```

Install multiple SDKs side-by-side for multi-targeting:

```sh
mise use dotnet@8
mise use dotnet@9
dotnet --list-sdks
```

## `global.json` support

mise recognizes `global.json` as an idiomatic version file. If your project contains a `global.json`
with an SDK version, mise will automatically use it:

```json
{
  "sdk": {
    "version": "8.0.100"
  }
}
```

Enable idiomatic version file support:

```sh
mise settings set idiomatic_version_file_enable_tools dotnet
```

## Isolated Mode

By default, all SDK versions share a single `DOTNET_ROOT` directory. This matches .NET's native
side-by-side model and means `dotnet --list-sdks` shows every installed version.

If you prefer the traditional mise approach where each version gets its own directory, enable
isolated mode:

```sh
mise settings set dotnet.isolated true
```

In isolated mode each SDK version is installed under `~/.local/share/mise/installs/dotnet/<version>/`,
just like most other mise-managed tools. `dotnet --list-sdks` will only report the currently active
version.

|                      | Shared (default)       | Isolated                     |
| -------------------- | ---------------------- | ---------------------------- |
| `dotnet --list-sdks` | All installed versions | Active version only          |
| Install location     | `DOTNET_ROOT`          | `installs/dotnet/<version>/` |
| Multi-targeting      | Works out of the box   | Requires switching versions  |

## Environment Variables

The plugin sets the following environment variables:

| Variable                      | Value                                                      |
| ----------------------------- | ---------------------------------------------------------- |
| `DOTNET_ROOT`                 | Shared SDK install directory (or install path if isolated) |
| `DOTNET_MULTILEVEL_LOOKUP`    | `0`                                                        |
| `DOTNET_CLI_TELEMETRY_OPTOUT` | Only set when `dotnet.cli_telemetry_optout` is configured  |

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="dotnet" :level="3" />
