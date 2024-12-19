# Dotnet backend

The code for this is inside the mise repository at [`./src/backend/dotnet.rs`](https://github.com/jdx/mise/blob/main/src/backend/dotnet.rs).

## Usage

The following installs the latest version of [eza](https://crates.io/crates/eza) and
sets it as the active version on PATH:

```sh
$ mise use -g dotnet:GitVersion.Tool@5.12.0
$ dotnet-gitversion /version
5.12.0+Branch.support-5.x.Sha.3f75764963eb3d7956dcd5a40488c074dd9faf9e
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"dotnet:GitVersion.Tool" = "5.12.0"
```

### Supported Pipx Syntax

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
