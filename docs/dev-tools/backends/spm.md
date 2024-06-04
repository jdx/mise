# SPM Backend <Badge type="warning" text="experimental" />

You may install executables managed by [Swift Package Manager](https://www.swift.org/documentation/package-manager) directly from Github releases.

The code for this is inside of the mise repository at [`./src/backend/spm.rs`](https://github.com/jdx/mise/blob/main/src/backend/spm.rs).

## Dependencies

This relies on having `swift` installed. You can install it according to the [instructions](https://www.swift.org/install).

## Usage

The following installs the latest version of `tuist`
and sets it as the active version on PATH:

```sh
$ mise use -g spm:tuist/tuist
$ tuist --help
OVERVIEW: Generate, build and test your Xcode projects.

USAGE: tuist <subcommand>
...
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"spm:tuist/tuist" = "latest"
```

### Supported Ubi Syntax

| Description                                   | Usage                                                                                                   |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Github shorthand for latest release version   | `spm:tuist/tuist`                                                                                       |
| Github shorthand for specific release version | `spm:tuist/tuist@4.15.0`                                                                                |
| Github url for latest release version         | `spm:https://github.com/tuist/tuist.git`                                                                |
| Github url for specific release version       | `spm:https://github.com/tuist/tuist.git@4.15.0`                                                         |

Other syntax may work but is unsupported and untested.
