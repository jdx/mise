# Dotnet backend

The code for this is inside the mise repository at [`./src/backend/dotnet.rs`](https://github.com/jdx/mise/blob/main/src/backend/dotnet.rs).

::: tip Important
The dotnet backend requires having the .NET runtime installed. You can install it using mise:

```sh
# Install the latest version
mise use dotnet

# Or install a specific version (8, 9, etc.)
mise use dotnet@8
mise use dotnet@9
```

This will install the .NET runtime, which is required for dotnet tools to work properly.
:::

## Usage

The following installs the latest version of [GitVersion.Tool](https://gitversion.net/) and
sets it as the active version on PATH:

```sh
$ mise use dotnet:GitVersion.Tool@5.12.0
$ dotnet-gitversion /version
5.12.0+Branch.support-5.x.Sha.3f75764963eb3d7956dcd5a40488c074dd9faf9e
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"dotnet:GitVersion.Tool" = "5.12.0"
```

```sh
$ mise use dotnet:GitVersion.Tool
$ dotnet-gitversion /version
6.1.0+Branch.main.Sha.8856e3041dbb768118a55a31ad4e465ae70c6767
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"dotnet:GitVersion.Tool" = "latest"
```

### Supported Dotnet Syntax

| Description                           | Usage                           |
| ------------------------------------- | ------------------------------- |
| Dotnet shorthand latest version       | `dotnet:GitVersion.Tool`        |
| Dotnet shorthand for specific version | `dotnet:GitVersion.Tool@5.12.0` |

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="dotnet" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `dotnet` backend—these
go in `[tools]` in `mise.toml`.
