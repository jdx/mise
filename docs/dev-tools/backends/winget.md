# Winget Backend

The code for this is inside the mise repository at [`./src/backend/winget.rs`](https://github.com/jdx/mise/blob/main/src/backend/winget.rs).

::: tip Important
The winget backend is **Windows-only** and requires the [Windows Package Manager](https://github.com/microsoft/winget-cli) (`winget`) to be installed.

On Windows 10 (1709+) and Windows 11, winget is typically pre-installed via the App Installer package from the Microsoft Store.
:::

## Dependencies

This relies on having `winget` installed on your Windows system. winget is the official Windows package manager from Microsoft.

## Usage

The following example installs [AWS CLI](https://aws.amazon.com/cli/) via winget:

**Note:** This example was updated to use a more practical package. See [mise discussion #8311](https://github.com/jdx/mise/discussions/8311) and [aqua-registry issue #14093](https://github.com/aquaproj/aqua-registry/issues/14093) for more context.

The latest version of AWS CLI will be installed and set as the active version on PATH:

```sh
$ mise use winget:Amazon.AWSCLI
$ aws --version
aws-cli/2.15.0 Python/3.11.7 Windows/11.0.22621 exe/AMD64.prod.exe
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"winget:Amazon.AWSCLI" = "latest"
```

### Specifying a version

```sh
mise use winget:Amazon.AWSCLI@2.15.0
```

```toml
[tools]
"winget:Amazon.AWSCLI" = "2.15.0"
```

### Supported Winget Syntax

| Description                           | Usage                         |
| ------------------------------------- | ----------------------------- |
| Winget shorthand latest version       | `winget:Amazon.AWSCLI`        |
| Winget shorthand for specific version | `winget:Amazon.AWSCLI@2.15.0` |

::: tip Package IDs
Winget package IDs follow the format used in the [winget-pkgs](https://github.com/microsoft/winget-pkgs) repository.
You can search for package IDs using `winget search <name>`.
:::

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `winget` backend—these
go in `[tools]` in `mise.toml`.
