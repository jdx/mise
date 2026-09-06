# Swift

`mise` can be used to manage multiple versions of [`swift`](https://swift.org/) on the same system. Swift is supported on macOS and Linux.

## Usage

Install Swift for the current project and check the selected toolchain:

```sh
mise use swift@latest
mise exec -- swift --version
```

Use `mise use -g swift@latest` for a personal default. In an existing Swift package
with `Package.swift`, run `mise exec -- swift build` to build it.

On Linux, Swift archives target specific distributions and require compatible
system libraries. mise records the selected distribution in lockfile options;
use a lock entry built for your target distribution. The Swift core plugin does
not currently support Windows.

See [a mise guide for Swift developers](https://tuist.dev/blog/2025/02/04/mise) for how to use `mise` with `swift`.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `swift` backend.
These options go in the `[tools]` section of `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `swift` backend:

```toml
[tools]
swift = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="swift" :level="3" />
