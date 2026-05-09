# SPM Backend <Badge type="warning" text="experimental" />

You may install executables managed by [Swift Package Manager](https://www.swift.org/documentation/package-manager) directly from GitHub or GitLab releases.

The code for this is inside of the mise repository at [`./src/backend/spm.rs`](https://github.com/jdx/mise/blob/main/src/backend/spm.rs).

When a release publishes a SwiftPM artifact bundle (`*.artifactbundle.zip`), mise will use the prebuilt executable from the bundle when it matches the current Swift target triple. If no matching bundle is available, mise falls back to building the package from source unless artifact bundles are explicitly required.

## Dependencies

This relies on having `swift` installed. You can either install it [manually](https://www.swift.org/install) or [with mise](/lang/swift).

> [!NOTE]
> If you have Xcode installed and selected in your system via `xcode-select`, Swift is already available through the toolchain embedded in the Xcode installation.

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

If the release provides only a SwiftPM artifact bundle, mise can install the bundle directly:

```sh
mise use -g spm:giginet/swift-testing-revolutionary@0.4.0
swift-testing-revolutionary --help
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"spm:giginet/swift-testing-revolutionary" = "0.4.0"
```

### Supported Syntax

| Description                                   | Usage                                           |
| --------------------------------------------- | ----------------------------------------------- |
| GitHub shorthand for latest release version   | `spm:tuist/tuist`                               |
| GitHub shorthand for specific release version | `spm:tuist/tuist@4.15.0`                        |
| GitHub url for latest release version         | `spm:https://github.com/tuist/tuist.git`        |
| GitHub url for specific release version       | `spm:https://github.com/tuist/tuist.git@4.15.0` |

Other syntax may work but is unsupported and untested.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the backend — these
go in `[tools]` in `mise.toml`.

### `provider`

Set the provider type to use for fetching assets and release information. Either `github` or `gitlab` (default is `github`).
Ensure the `provider` is set to the correct type if you use shorthand notation and `api_url` for self-hosted repositories
as the type probably cannot be derived correctly from the URL.

```toml
[tools]
"spm:patricklorran/ios-settings" = { version = "latest", provider = "gitlab" }
```

### `api_url`

Set the URL for the provider's API. This is useful when using a self-hosted instance.

```toml
[tools]
"spm:acme/my-tool" = { version = "latest", provider = "gitlab", api_url = "https://gitlab.acme.com/api/v4" }
```

### `artifactbundle`

Control whether SwiftPM artifact bundles are used. When unset, mise tries a matching
`*.artifactbundle.zip` release asset first and falls back to building from source if no matching
bundle is available.

Set `artifactbundle = true` to require an artifact bundle for a tool. If no bundle matches the
current Swift target triple, installation fails instead of falling back to a source build.

Set `artifactbundle = false` to skip artifact bundles and always build from source.

```toml
[tools]
"spm:giginet/swift-testing-revolutionary" = { version = "0.4.0", artifactbundle = true }
"spm:tuist/tuist" = { version = "latest", artifactbundle = false }
```

### `artifactbundle_asset`

Select a specific artifact bundle release asset. This is required when a release contains multiple
`*.artifactbundle.zip` assets.

```toml
[tools]
"spm:giginet/swift-testing-revolutionary" = { version = "0.4.0", artifactbundle_asset = "swift-testing-revolutionary.artifactbundle.zip" }
```

### `filter_bins`

Restrict which executable products are installed from the package or artifact bundle. When unset,
every executable product declared in `Package.swift` is built and symlinked into `bin/`, or every
matching executable artifact from an artifact bundle is symlinked into `bin/`.

Useful when a package ships helper executables (e.g. test harnesses) that you don't want on your
`PATH`. For source builds, filtering happens before `swift build`, so unwanted products are never
built.

Accepts a TOML array or a comma-separated string. If any listed name does not match an executable
product in the package, installation fails with a clear error.

```toml
[tools]
"spm:swiftlang/swiftly" = { version = "latest", filter_bins = ["swiftly"] }
# or
"spm:swiftlang/swiftly" = { version = "latest", filter_bins = "swiftly" }
```

## Settings

### `spm.artifactbundle_only`

Set `spm.artifactbundle_only = true` to require SwiftPM artifact bundles for all `spm:` installs.
This mirrors `cargo.binstall_only`: mise will fail if no matching artifact bundle is available
instead of compiling from source.

```toml
[settings]
spm.artifactbundle_only = true
```

This can also be set with `MISE_SPM_ARTIFACTBUNDLE_ONLY=1`.
